use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use tracing::{error, info, warn};

#[derive(Parser, Debug)]
#[command(name = "aurea-pmdaemon", about = "AUREA process supervisor")]
struct Cli {
    #[arg(long, default_value = "target/debug/aurea")]
    cmd: String,
    #[arg(long, default_value = "serve")]
    arg0: String,
    #[arg(long, default_value = "0.0.0.0:8080")]
    listen: String,
    #[arg(long, default_value = "./aurea.redb")]
    db: String,
    #[arg(long, default_value = "./keys")]
    keys_dir: String,
    #[arg(long, default_value = "./logs")]
    logs_dir: PathBuf,
    #[arg(long, default_value_t = 8)]
    max_restarts: u32,
    #[arg(long)]
    health_url: Option<String>,
    #[arg(long, default_value_t = 5_000)]
    health_grace_ms: u64,
    #[arg(long, default_value_t = 1_500)]
    health_interval_ms: u64,
    #[arg(long, default_value_t = 2_000)]
    health_timeout_ms: u64,
    #[arg(long, default_value_t = 3)]
    max_consecutive_health_failures: u32,
}

#[derive(Debug, Clone)]
struct MonitorConfig {
    health_url: String,
    health_grace: Duration,
    health_interval: Duration,
    health_timeout: Duration,
    max_consecutive_health_failures: u32,
}

#[derive(Debug)]
enum RunOutcome {
    ExitedSuccess,
    ExitedFailure(ExitStatus),
    Unhealthy { failures: u32, reason: String },
}

#[derive(Debug)]
struct HealthEndpoint {
    host: String,
    port: u16,
    path: String,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    std::fs::create_dir_all(&cli.logs_dir).context("create logs directory")?;

    let health_url = cli
        .health_url
        .clone()
        .unwrap_or_else(|| default_health_url(&cli.listen));
    let monitor = MonitorConfig {
        health_url,
        health_grace: Duration::from_millis(cli.health_grace_ms),
        health_interval: Duration::from_millis(cli.health_interval_ms),
        health_timeout: Duration::from_millis(cli.health_timeout_ms),
        max_consecutive_health_failures: cli.max_consecutive_health_failures,
    };

    info!(
        cmd = %cli.cmd,
        listen = %cli.listen,
        db = %cli.db,
        keys_dir = %cli.keys_dir,
        health_url = %monitor.health_url,
        max_restarts = cli.max_restarts,
        "pmdaemon configured"
    );

    for attempt in 0..=cli.max_restarts {
        rotate_logs(&cli.logs_dir)?;
        let out = std::fs::File::create(cli.logs_dir.join("aurea.out.log"))?;
        let err = std::fs::File::create(cli.logs_dir.join("aurea.err.log"))?;

        info!(attempt, "starting child process");
        let mut child = Command::new(&cli.cmd)
            .arg(&cli.arg0)
            .arg("--listen")
            .arg(&cli.listen)
            .arg("--db")
            .arg(&cli.db)
            .arg("--keys-dir")
            .arg(&cli.keys_dir)
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err))
            .spawn()
            .with_context(|| format!("spawn child command {}", cli.cmd))?;

        let outcome = monitor_child(&mut child, &monitor)?;
        match outcome {
            RunOutcome::ExitedSuccess => {
                info!("child exited cleanly, supervisor exiting");
                return Ok(());
            }
            RunOutcome::ExitedFailure(status) => {
                if attempt == cli.max_restarts {
                    error!(?status, "child failed too many times; giving up");
                    break;
                }
                let backoff = backoff_for_restart(attempt);
                error!(?status, ?backoff, "child crashed; restarting with backoff");
                thread::sleep(backoff);
            }
            RunOutcome::Unhealthy { failures, reason } => {
                if attempt == cli.max_restarts {
                    error!(failures, reason = %reason, "child unhealthy too many times; giving up");
                    break;
                }
                let backoff = backoff_for_restart(attempt);
                error!(failures, reason = %reason, ?backoff, "child unhealthy; restarting with backoff");
                thread::sleep(backoff);
            }
        }
    }

    Ok(())
}

fn monitor_child(child: &mut Child, cfg: &MonitorConfig) -> Result<RunOutcome> {
    let start = Instant::now();
    let mut consecutive_failures = 0u32;

    loop {
        if let Some(status) = child.try_wait().context("poll child status")? {
            if status.success() {
                return Ok(RunOutcome::ExitedSuccess);
            }
            return Ok(RunOutcome::ExitedFailure(status));
        }

        if start.elapsed() >= cfg.health_grace {
            match check_health_once(&cfg.health_url, cfg.health_timeout) {
                Ok(()) => {
                    consecutive_failures = 0;
                }
                Err(err) => {
                    consecutive_failures = consecutive_failures.saturating_add(1);
                    warn!(
                        failures = consecutive_failures,
                        max_failures = cfg.max_consecutive_health_failures,
                        error = %err,
                        "health check failed"
                    );

                    if consecutive_failures >= cfg.max_consecutive_health_failures {
                        terminate_child(child)?;
                        return Ok(RunOutcome::Unhealthy {
                            failures: consecutive_failures,
                            reason: err.to_string(),
                        });
                    }
                }
            }
        }

        thread::sleep(cfg.health_interval);
    }
}

fn terminate_child(child: &mut Child) -> Result<()> {
    if child
        .try_wait()
        .context("poll child before terminate")?
        .is_some()
    {
        return Ok(());
    }

    child.kill().context("kill unhealthy child")?;
    let _ = child.wait().context("wait unhealthy child after kill")?;
    Ok(())
}

fn check_health_once(url: &str, timeout: Duration) -> Result<()> {
    let endpoint = parse_health_url(url)?;
    let address = format_socket_address(&endpoint.host, endpoint.port);
    let socket = address
        .to_socket_addrs()
        .with_context(|| format!("resolve health endpoint {address}"))?
        .next()
        .ok_or_else(|| anyhow!("health endpoint {address} did not resolve"))?;

    let mut stream = TcpStream::connect_timeout(&socket, timeout)
        .with_context(|| format!("connect health endpoint {address}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .context("set read timeout")?;
    stream
        .set_write_timeout(Some(timeout))
        .context("set write timeout")?;

    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        endpoint.path, endpoint.host
    );
    stream
        .write_all(request.as_bytes())
        .context("write health check request")?;

    let mut buf = [0u8; 512];
    let read = stream
        .read(&mut buf)
        .context("read health check response")?;
    if read == 0 {
        return Err(anyhow!("empty health response"));
    }

    let response = String::from_utf8_lossy(&buf[..read]);
    let status_line = response.lines().next().unwrap_or_default();
    if !(status_line.starts_with("HTTP/1.1 200") || status_line.starts_with("HTTP/1.0 200")) {
        return Err(anyhow!("health endpoint returned non-200: {status_line}"));
    }

    Ok(())
}

fn parse_health_url(url: &str) -> Result<HealthEndpoint> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| anyhow!("health_url must start with http://"))?;

    let (host_port, path) = match rest.split_once('/') {
        Some((hp, p)) => (hp, format!("/{p}")),
        None => (rest, "/".to_string()),
    };

    let (host, port_str) = if host_port.starts_with('[') {
        let (host, port) = host_port
            .rsplit_once(":")
            .ok_or_else(|| anyhow!("health_url missing port"))?;
        let host = host
            .strip_prefix('[')
            .and_then(|h| h.strip_suffix(']'))
            .ok_or_else(|| anyhow!("invalid IPv6 host format in health_url"))?;
        (host.to_string(), port)
    } else {
        let (host, port) = host_port
            .rsplit_once(":")
            .ok_or_else(|| anyhow!("health_url missing port"))?;
        (host.to_string(), port)
    };

    let port = port_str
        .parse::<u16>()
        .with_context(|| format!("invalid port in health_url: {port_str}"))?;

    if host.is_empty() {
        return Err(anyhow!("health_url host cannot be empty"));
    }

    Ok(HealthEndpoint { host, port, path })
}

fn format_socket_address(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

fn default_health_url(listen: &str) -> String {
    let port = listen
        .rsplit(':')
        .next()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(8080);
    format!("http://127.0.0.1:{port}/healthz")
}

fn backoff_for_restart(restart_attempt: u32) -> Duration {
    let mut seconds = 1u64;
    for _ in 0..restart_attempt {
        seconds = std::cmp::min(seconds.saturating_mul(2), 30);
    }
    Duration::from_secs(seconds)
}

fn rotate_logs(dir: &std::path::Path) -> Result<()> {
    let out = dir.join("aurea.out.log");
    let err = dir.join("aurea.err.log");
    if out.exists() {
        let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        std::fs::rename(&out, dir.join(format!("aurea.out.{ts}.log")))?;
    }
    if err.exists() {
        let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        std::fs::rename(&err, dir.join(format!("aurea.err.{ts}.log")))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_health_url_uses_listen_port() {
        assert_eq!(
            default_health_url("0.0.0.0:9090"),
            "http://127.0.0.1:9090/healthz"
        );
    }

    #[test]
    fn parse_health_url_ipv4_and_path() {
        let ep = parse_health_url("http://127.0.0.1:8080/healthz").expect("parse health url");
        assert_eq!(ep.host, "127.0.0.1");
        assert_eq!(ep.port, 8080);
        assert_eq!(ep.path, "/healthz");
    }

    #[test]
    fn parse_health_url_ipv6() {
        let ep = parse_health_url("http://[::1]:9000/healthz").expect("parse health url");
        assert_eq!(ep.host, "::1");
        assert_eq!(ep.port, 9000);
    }

    #[test]
    fn backoff_caps_at_30_seconds() {
        assert_eq!(backoff_for_restart(0), Duration::from_secs(1));
        assert_eq!(backoff_for_restart(1), Duration::from_secs(2));
        assert_eq!(backoff_for_restart(2), Duration::from_secs(4));
        assert_eq!(backoff_for_restart(10), Duration::from_secs(30));
    }
}
