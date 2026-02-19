use std::convert::Infallible;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use async_stream::stream;
use aurea_core::{Receipt, WorkStatus, WorkUnit};
use aurea_plugins::{EchoPlugin, PluginRegistry};
use aurea_runtime::{AcceptDisposition, ReceiptVerification, Runtime, RuntimeMetrics};
use aurea_storage::RedbStore;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use chrono::Utc;
use clap::{Parser, Subcommand};
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "aurea", version, about = "AUREA runtime binary")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Serve {
        #[arg(long, default_value = "0.0.0.0:8080")]
        listen: String,
        #[arg(long, default_value = "./aurea.redb")]
        db: String,
        #[arg(long, default_value = "./keys")]
        keys_dir: String,
    },
    Keys {
        #[command(subcommand)]
        command: KeysCommand,
    },
}

#[derive(Subcommand, Debug)]
enum KeysCommand {
    Rotate {
        #[arg(long, default_value = "./keys")]
        keys_dir: String,
    },
    Revoke {
        #[arg(long, default_value = "./keys")]
        keys_dir: String,
        #[arg(long)]
        kid: String,
    },
    List {
        #[arg(long, default_value = "./keys")]
        keys_dir: String,
    },
}

#[derive(Clone)]
struct AppState {
    runtime: Runtime,
    keyring: KeyRing,
}

#[derive(Debug, Deserialize)]
struct SubmitWorkRequest {
    tenant: String,
    topic: String,
    payload: Value,
    idem_key: Option<String>,
}

#[derive(Debug, Serialize)]
struct SubmitWorkResponse {
    status: String,
    work_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    receipt_cid: Option<String>,
    duplicate: bool,
    in_flight: bool,
}

#[derive(Debug, Deserialize)]
struct VerifyReceiptRequest {
    cid: Option<String>,
    receipt: Option<Receipt>,
}

#[derive(Debug, Serialize)]
struct VerifyReceiptResponse {
    ok: bool,
    cid_match: bool,
    signature_valid: bool,
    key_known: bool,
    key_match: bool,
    key_revoked: bool,
}

#[derive(Debug, Deserialize)]
struct StreamQuery {
    topic: Option<String>,
    tenant: Option<String>,
    id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredKey {
    kid: String,
    secret_key: String,
    public_key: String,
    created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum KeyStatus {
    Active,
    Retired,
    Revoked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KeyMetadata {
    kid: String,
    public_key: String,
    created_at: String,
    status: KeyStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    revoked_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct KeyRing {
    active_kid: Option<String>,
    keys: Vec<KeyMetadata>,
}

#[derive(Debug, Clone)]
struct KeyPolicy {
    known: bool,
    key_match: bool,
    revoked: bool,
}

impl KeyPolicy {
    fn ok(&self) -> bool {
        self.known && self.key_match && !self.revoked
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Serve {
            listen,
            db,
            keys_dir,
        } => run_server(listen, db, keys_dir).await,
        Command::Keys { command } => run_keys_command(command),
    }
}

fn run_keys_command(command: KeysCommand) -> Result<()> {
    match command {
        KeysCommand::Rotate { keys_dir } => {
            let (record, _) = rotate_and_activate_key(Path::new(&keys_dir))?;
            println!(
                "rotated signing key: kid={} public_key={}",
                record.kid, record.public_key
            );
        }
        KeysCommand::Revoke { keys_dir, kid } => {
            revoke_key(Path::new(&keys_dir), &kid)?;
            println!("revoked signing key: kid={}", kid);
        }
        KeysCommand::List { keys_dir } => {
            let ring = load_keyring(Path::new(&keys_dir))?.unwrap_or_default();
            println!(
                "active_kid={}",
                ring.active_kid.as_deref().unwrap_or("<none>")
            );
            for key in ring.keys {
                println!(
                    "kid={} status={:?} revoked_at={}",
                    key.kid,
                    key.status,
                    key.revoked_at.as_deref().unwrap_or("-")
                );
            }
        }
    }
    Ok(())
}

async fn run_server(listen: String, db: String, keys_dir: String) -> Result<()> {
    let store = RedbStore::open(&db)?;
    let mut plugins = PluginRegistry::new();
    plugins.register(EchoPlugin);

    let (kid, signing_key, keyring) = load_or_create_key_material(Path::new(&keys_dir))?;
    let runtime = Runtime::new_with_signer(store, plugins, signing_key, kid);
    let _worker = runtime.start_background_worker();

    let state = AppState { runtime, keyring };

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/work", post(submit_work))
        .route("/v1/stream", get(stream_events))
        .route("/v1/receipts/{cid}", get(get_receipt))
        .route("/v1/verify/receipt", post(verify_receipt))
        .route("/v1/metrics", get(metrics))
        .route("/v1/capabilities", get(capabilities))
        .with_state(state);

    let addr: SocketAddr = listen.parse().context("invalid --listen address")?;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("failed to bind listen address")?;

    info!(%addr, db, keys_dir, "aurea server listening");
    axum::serve(listener, app)
        .await
        .context("axum server failed")?;

    Ok(())
}

async fn healthz() -> Json<Value> {
    Json(json!({"ok": true}))
}

async fn submit_work(
    State(state): State<AppState>,
    Json(req): Json<SubmitWorkRequest>,
) -> Result<(HeaderMap, Json<SubmitWorkResponse>), (StatusCode, Json<Value>)> {
    let mut work = WorkUnit::new(req.tenant, req.topic, req.idem_key, req.payload);
    let idem_key = work.effective_idem_key().map_err(internal_error)?;
    work.idem_key = Some(idem_key.clone());

    let accepted = state
        .runtime
        .accept_work(work)
        .await
        .map_err(internal_error)?;

    let mut headers = HeaderMap::new();
    headers.insert("x-aurea-api", HeaderValue::from_static("1.0"));
    headers.insert(
        "x-idempotency-key",
        HeaderValue::from_str(&idem_key).map_err(internal_error)?,
    );

    let response = match accepted.disposition {
        AcceptDisposition::Enqueued => {
            headers.insert("x-idempotent-replay", HeaderValue::from_static("false"));
            SubmitWorkResponse {
                status: "accepted".to_string(),
                work_id: accepted.work_id.to_string(),
                receipt_cid: None,
                duplicate: false,
                in_flight: false,
            }
        }
        AcceptDisposition::DuplicateReceipt { receipt_cid } => {
            headers.insert("x-idempotent-replay", HeaderValue::from_static("true"));
            headers.insert("x-idempotent-in-flight", HeaderValue::from_static("false"));
            headers.insert(
                "x-aurea-receipt-cid",
                HeaderValue::from_str(&receipt_cid).map_err(internal_error)?,
            );
            SubmitWorkResponse {
                status: "duplicate".to_string(),
                work_id: accepted.work_id.to_string(),
                receipt_cid: Some(receipt_cid),
                duplicate: true,
                in_flight: false,
            }
        }
        AcceptDisposition::DuplicateInFlight => {
            headers.insert("x-idempotent-replay", HeaderValue::from_static("true"));
            headers.insert("x-idempotent-in-flight", HeaderValue::from_static("true"));
            SubmitWorkResponse {
                status: "duplicate_in_flight".to_string(),
                work_id: accepted.work_id.to_string(),
                receipt_cid: None,
                duplicate: true,
                in_flight: true,
            }
        }
    };

    Ok((headers, Json(response)))
}

async fn stream_events(
    State(state): State<AppState>,
    Query(query): Query<StreamQuery>,
) -> Sse<impl futures_core::Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.runtime.subscribe_events();

    let stream = stream! {
        loop {
            let Ok(event) = rx.recv().await else {
                break;
            };

            if let Some(tenant) = &query.tenant
                && &event.tenant != tenant
            {
                continue;
            }
            if let Some(topic) = &query.topic
                && &event.topic != topic
            {
                continue;
            }
            if let Some(work_id) = &query.id
                && event.work_id.to_string() != *work_id
            {
                continue;
            }

            let sse_event = Event::default()
                .event(status_name(event.status))
                .json_data(&event)
                .unwrap_or_else(|_| Event::default().event("error").data("serialization_error"));

            yield Ok::<Event, Infallible>(sse_event);
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn get_receipt(
    State(state): State<AppState>,
    AxumPath(cid): AxumPath<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.runtime.get_receipt(&cid).map_err(internal_error)? {
        Some(receipt) => Ok(Json(serde_json::to_value(receipt).map_err(internal_error)?)),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"code":"NOT_FOUND","message":"receipt not found"})),
        )),
    }
}

async fn verify_receipt(
    State(state): State<AppState>,
    Json(req): Json<VerifyReceiptRequest>,
) -> Result<Json<VerifyReceiptResponse>, (StatusCode, Json<Value>)> {
    if let Some(receipt) = req.receipt {
        let check = state
            .runtime
            .verify_receipt(&receipt)
            .map_err(internal_error)?;
        let key_policy = evaluate_key_policy(
            &state.keyring,
            &receipt.signature.kid,
            &receipt.signature.public_key,
        );
        return Ok(Json(map_verification(check, key_policy)));
    }

    if let Some(cid) = req.cid {
        let Some(receipt) = state.runtime.get_receipt(&cid).map_err(internal_error)? else {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({"code":"NOT_FOUND","message":"receipt not found"})),
            ));
        };
        let check = state
            .runtime
            .verify_receipt(&receipt)
            .map_err(internal_error)?;
        let key_policy = evaluate_key_policy(
            &state.keyring,
            &receipt.signature.kid,
            &receipt.signature.public_key,
        );
        return Ok(Json(map_verification(check, key_policy)));
    }

    Err((
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({"code":"SCHEMA_INVALID","message":"provide `cid` or `receipt`"})),
    ))
}

fn map_verification(check: ReceiptVerification, key_policy: KeyPolicy) -> VerifyReceiptResponse {
    VerifyReceiptResponse {
        ok: check.ok && key_policy.ok(),
        cid_match: check.cid_match,
        signature_valid: check.signature_valid,
        key_known: key_policy.known,
        key_match: key_policy.key_match,
        key_revoked: key_policy.revoked,
    }
}

fn evaluate_key_policy(keyring: &KeyRing, kid: &str, public_key: &str) -> KeyPolicy {
    let Some(meta) = keyring.keys.iter().find(|k| k.kid == kid) else {
        return KeyPolicy {
            known: false,
            key_match: false,
            revoked: false,
        };
    };

    KeyPolicy {
        known: true,
        key_match: meta.public_key == public_key,
        revoked: meta.status == KeyStatus::Revoked,
    }
}

async fn metrics(State(state): State<AppState>) -> Result<String, (StatusCode, Json<Value>)> {
    let metrics = state.runtime.metrics_snapshot().map_err(internal_error)?;
    Ok(render_prometheus(&metrics))
}

fn render_prometheus(metrics: &RuntimeMetrics) -> String {
    let mut out = String::new();

    out.push_str("# HELP aurea_queue_depth Current ready queue depth.\n");
    out.push_str("# TYPE aurea_queue_depth gauge\n");
    out.push_str(&format!("aurea_queue_depth {}\n", metrics.queue_depth));

    out.push_str("# HELP aurea_leased_depth Current leased queue depth.\n");
    out.push_str("# TYPE aurea_leased_depth gauge\n");
    out.push_str(&format!("aurea_leased_depth {}\n", metrics.leased_depth));

    out.push_str("# HELP aurea_reassigns_total Total expired lease reassignments.\n");
    out.push_str("# TYPE aurea_reassigns_total counter\n");
    out.push_str(&format!(
        "aurea_reassigns_total {}\n",
        metrics.reassigns_total
    ));

    out.push_str("# HELP aurea_receipts_total Total receipts persisted.\n");
    out.push_str("# TYPE aurea_receipts_total counter\n");
    out.push_str(&format!(
        "aurea_receipts_total {}\n",
        metrics.receipts_total
    ));

    out.push_str("# HELP aurea_plugins_total Current plugin count.\n");
    out.push_str("# TYPE aurea_plugins_total gauge\n");
    out.push_str(&format!("aurea_plugins_total {}\n", metrics.plugins_total));

    out.push_str("# HELP aurea_jobs_total Total jobs by lifecycle status.\n");
    out.push_str("# TYPE aurea_jobs_total counter\n");
    for (status, total) in &metrics.status_totals {
        out.push_str(&format!(
            "aurea_jobs_total{{status=\"{}\"}} {}\n",
            status, total
        ));
    }

    append_histogram(
        &mut out,
        "aurea_ttft_ms",
        "Time to first transition (accepted to assigned) in milliseconds.",
        &metrics.ttft_bucket_counts,
        metrics.ttft_sum_ms,
        metrics.ttft_count,
    );
    append_histogram(
        &mut out,
        "aurea_ttr_ms",
        "Time to result (accepted to done/fail) in milliseconds.",
        &metrics.ttr_bucket_counts,
        metrics.ttr_sum_ms,
        metrics.ttr_count,
    );

    out
}

fn append_histogram(
    out: &mut String,
    name: &str,
    help: &str,
    buckets: &[(u64, u64)],
    sum: u64,
    count: u64,
) {
    out.push_str(&format!("# HELP {name} {help}\n"));
    out.push_str(&format!("# TYPE {name} histogram\n"));
    for (le, bucket_count) in buckets {
        out.push_str(&format!(
            "{name}_bucket{{le=\"{}\"}} {}\n",
            le, bucket_count
        ));
    }
    out.push_str(&format!("{name}_bucket{{le=\"+Inf\"}} {}\n", count));
    out.push_str(&format!("{name}_sum {}\n", sum));
    out.push_str(&format!("{name}_count {}\n", count));
}

async fn capabilities(State(state): State<AppState>) -> Json<Value> {
    Json(json!({"capabilities": state.runtime.capabilities()}))
}

fn status_name(status: WorkStatus) -> &'static str {
    match status {
        WorkStatus::Accepted => "accepted",
        WorkStatus::Assigned => "assigned",
        WorkStatus::Progress => "progress",
        WorkStatus::Done => "done",
        WorkStatus::Fail => "fail",
    }
}

fn load_or_create_key_material(keys_dir: &Path) -> Result<(String, SigningKey, KeyRing)> {
    fs::create_dir_all(keys_dir).context("failed to create keys directory")?;
    let current_path = keys_dir.join("current.json");

    if current_path.exists() {
        let raw = fs::read_to_string(&current_path).context("failed to read current key file")?;
        let record: StoredKey =
            serde_json::from_str(&raw).context("failed to parse current key file")?;
        let signer = signing_key_from_record(&record)?;

        let ring = load_keyring(keys_dir)?.unwrap_or_default();
        let normalized = normalize_keyring_with_active(ring, &record);
        persist_keyring(keys_dir, &normalized)?;

        return Ok((record.kid, signer, normalized));
    }

    let (record, ring) = rotate_and_activate_key(keys_dir)?;
    let signer = signing_key_from_record(&record)?;
    Ok((record.kid, signer, ring))
}

fn rotate_and_activate_key(keys_dir: &Path) -> Result<(StoredKey, KeyRing)> {
    fs::create_dir_all(keys_dir).context("failed to create keys directory")?;

    let signer = SigningKey::generate(&mut OsRng);
    let kid = format!(
        "{}-{:04x}",
        Utc::now().format("%Y%m%d-%H%M%S"),
        rand::random::<u16>()
    );

    let record = StoredKey {
        kid: kid.clone(),
        secret_key: B64.encode(signer.to_bytes()),
        public_key: B64.encode(signer.verifying_key().to_bytes()),
        created_at: Utc::now().to_rfc3339(),
    };

    let mut ring = load_keyring(keys_dir)?.unwrap_or_default();
    for key in &mut ring.keys {
        if key.status == KeyStatus::Active {
            key.status = KeyStatus::Retired;
            key.revoked_at = None;
        }
    }

    ring.keys.push(KeyMetadata {
        kid: kid.clone(),
        public_key: record.public_key.clone(),
        created_at: record.created_at.clone(),
        status: KeyStatus::Active,
        revoked_at: None,
    });
    ring.active_kid = Some(kid.clone());

    let kid_path = keys_dir.join(format!("{kid}.json"));
    let current_path = keys_dir.join("current.json");
    let bytes = serde_json::to_vec_pretty(&record).context("failed to serialize key record")?;

    fs::write(&kid_path, &bytes).with_context(|| format!("failed to write {kid_path:?}"))?;
    fs::write(&current_path, &bytes)
        .with_context(|| format!("failed to write {current_path:?}"))?;
    persist_keyring(keys_dir, &ring)?;

    Ok((record, ring))
}

fn revoke_key(keys_dir: &Path, kid: &str) -> Result<()> {
    let mut ring =
        load_keyring(keys_dir)?.ok_or_else(|| anyhow!("keyring not found, nothing to revoke"))?;

    if ring.active_kid.as_deref() == Some(kid) {
        return Err(anyhow!(
            "cannot revoke active key {}; rotate first and then revoke",
            kid
        ));
    }

    let Some(meta) = ring.keys.iter_mut().find(|k| k.kid == kid) else {
        return Err(anyhow!("kid {} not found in keyring", kid));
    };

    meta.status = KeyStatus::Revoked;
    meta.revoked_at = Some(Utc::now().to_rfc3339());

    persist_keyring(keys_dir, &ring)
}

fn normalize_keyring_with_active(mut ring: KeyRing, active: &StoredKey) -> KeyRing {
    for key in &mut ring.keys {
        if key.kid == active.kid {
            key.status = KeyStatus::Active;
            key.public_key = active.public_key.clone();
            key.created_at = active.created_at.clone();
            key.revoked_at = None;
        } else if key.status == KeyStatus::Active {
            key.status = KeyStatus::Retired;
            key.revoked_at = None;
        }
    }

    if !ring.keys.iter().any(|k| k.kid == active.kid) {
        ring.keys.push(KeyMetadata {
            kid: active.kid.clone(),
            public_key: active.public_key.clone(),
            created_at: active.created_at.clone(),
            status: KeyStatus::Active,
            revoked_at: None,
        });
    }

    ring.active_kid = Some(active.kid.clone());
    ring
}

fn keyring_path(keys_dir: &Path) -> PathBuf {
    keys_dir.join("keyring.json")
}

fn load_keyring(keys_dir: &Path) -> Result<Option<KeyRing>> {
    let path = keyring_path(keys_dir);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("failed to read {path:?}"))?;
    let ring: KeyRing =
        serde_json::from_str(&raw).with_context(|| format!("failed to parse {path:?}"))?;
    Ok(Some(ring))
}

fn persist_keyring(keys_dir: &Path, ring: &KeyRing) -> Result<()> {
    let path = keyring_path(keys_dir);
    let bytes = serde_json::to_vec_pretty(ring).context("failed to serialize keyring")?;
    fs::write(&path, &bytes).with_context(|| format!("failed to write {path:?}"))?;
    Ok(())
}

fn signing_key_from_record(record: &StoredKey) -> Result<SigningKey> {
    let secret = B64
        .decode(record.secret_key.as_bytes())
        .context("invalid base64 secret key")?;
    let key: [u8; 32] = secret
        .as_slice()
        .try_into()
        .map_err(|_| anyhow!("invalid secret key length"))?;
    Ok(SigningKey::from_bytes(&key))
}

fn internal_error(err: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"code":"INTERNAL_ERROR","message": err.to_string()})),
    )
}
