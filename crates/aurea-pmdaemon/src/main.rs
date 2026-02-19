use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use tracing::{error, info};

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
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    std::fs::create_dir_all(&cli.logs_dir).context("create logs directory")?;

    let mut backoff = Duration::from_secs(1);
    for attempt in 0..=cli.max_restarts {
        rotate_logs(&cli.logs_dir)?;
        let out = std::fs::File::create(cli.logs_dir.join("aurea.out.log"))?;
        let err = std::fs::File::create(cli.logs_dir.join("aurea.err.log"))?;

        info!(attempt, "starting child process");
        let status = Command::new(&cli.cmd)
            .arg(&cli.arg0)
            .arg("--listen")
            .arg(&cli.listen)
            .arg("--db")
            .arg(&cli.db)
            .arg("--keys-dir")
            .arg(&cli.keys_dir)
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err))
            .status()
            .with_context(|| format!("spawn child command {}", cli.cmd))?;

        if status.success() {
            info!("child exited cleanly, supervisor exiting");
            return Ok(());
        }

        if attempt == cli.max_restarts {
            error!("child failed too many times; giving up");
            break;
        }

        error!(?status, ?backoff, "child crashed; restarting with backoff");
        thread::sleep(backoff);
        backoff = std::cmp::min(backoff.saturating_mul(2), Duration::from_secs(30));
    }

    Ok(())
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
