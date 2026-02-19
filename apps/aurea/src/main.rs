use std::collections::HashMap;
use std::convert::Infallible;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use async_stream::stream;
use aurea_core::{Receipt, WorkStatus, WorkUnit, cid_of, to_nrf_bytes};
use aurea_plugins::{EchoPlugin, PluginRegistry};
use aurea_policy::{DefaultPolicy, Policy, PolicyEntry as PolicyTraceEntry, Route};
use aurea_receipts::anchor_day;
use aurea_runtime::{AcceptDisposition, ReceiptVerification, Runtime, RuntimeMetrics};
use aurea_storage::RedbStore;
use axum::extract::{Path as AxumPath, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
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
use tokio::sync::RwLock;
use tracing::info;
use uuid::Uuid;

const MAX_INTENT_PAYLOAD_BYTES: usize = 256 * 1024;
const DUAL_CONTROL_PHRASE: &str = "DUAL_CONTROL";
const TENANT_RATE_LIMIT_PER_MINUTE: u32 = 120;

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
    policy: DefaultPolicy,
    previews: Arc<RwLock<HashMap<String, StoredPreview>>>,
    schemas: Arc<HashMap<String, SchemaSpec>>,
    tenant_rate: Arc<RwLock<HashMap<String, TenantRateWindow>>>,
}

#[derive(Debug, Clone)]
struct TenantRateWindow {
    minute_epoch: i64,
    count: u32,
}

#[derive(Debug, Clone)]
struct StoredPreview {
    intent: Intent,
    plan_hash: String,
    policy_trace: Vec<PolicyTraceEntry>,
    dual_control_required: bool,
    route: Route,
}

#[derive(Debug, Clone)]
struct SchemaSpec {
    topic: &'static str,
    schema: Value,
    required_paths: &'static [&'static str],
}

#[derive(Debug, Deserialize)]
struct SubmitWorkRequest {
    tenant: String,
    topic: String,
    payload: Value,
    idem_key: Option<String>,
    plan_hash: Option<String>,
}

#[derive(Debug, Serialize)]
struct SubmitWorkResponse {
    status: String,
    work_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    receipt_cid: Option<String>,
    duplicate: bool,
    in_flight: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ApiErrorPayload>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    key_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamQuery {
    topic: Option<String>,
    tenant: Option<String>,
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ParseIntentRequest {
    text: String,
    schema_id: Option<String>,
    v: Option<String>,
    topic: Option<String>,
    payload: Option<Value>,
}

#[derive(Debug, Serialize)]
struct ParseIntentResponse {
    intent: Intent,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Intent {
    schema_id: String,
    v: String,
    topic: String,
    payload: Value,
}

#[derive(Debug, Deserialize)]
struct PlanPreviewRequest {
    intent: Intent,
    #[serde(default)]
    repair_attempt: u8,
}

#[derive(Debug, Serialize)]
struct PlanPreviewResponse {
    dag: Value,
    plan_hash: String,
    policy_trace: Vec<PolicyTraceEntry>,
    slos: Value,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
    route: Route,
    dual_control_required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    repair_request: Option<RepairRequest>,
}

#[derive(Debug, Serialize)]
struct RepairRequest {
    missing: Vec<String>,
    hints: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct OcCommitRequest {
    plan_hash: String,
    idem_key: Option<String>,
    confirm_phrase: Option<String>,
    tenant: Option<String>,
}

#[derive(Debug, Serialize)]
struct OcCommitResponse {
    status: String,
    work_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    receipt_cid: Option<String>,
    duplicate: bool,
    in_flight: bool,
}

#[derive(Debug, Deserialize)]
struct ExportRequest {
    format: String,
    topic: Option<String>,
}

#[derive(Debug, Serialize)]
struct ExportResponse {
    status: String,
    format: String,
    records: usize,
    path: String,
}

#[derive(Debug, Serialize, Clone)]
struct ApiErrorPayload {
    code: String,
    message: String,
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

    let state = AppState {
        runtime,
        keyring,
        policy: DefaultPolicy,
        previews: Arc::new(RwLock::new(HashMap::new())),
        schemas: Arc::new(default_schemas()),
        tenant_rate: Arc::new(RwLock::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/work", post(submit_work))
        .route("/v1/stream", get(stream_events))
        .route("/v1/receipts/{cid}", get(get_receipt))
        .route("/v1/verify/receipt", post(verify_receipt))
        .route("/v1/anchors/{day}", get(anchor_for_day))
        .route("/v1/metrics", get(metrics))
        .route("/v1/export", post(export_data))
        .route("/v1/capabilities", get(capabilities))
        .route("/v1/schema/{schema_id}/{v}", get(get_schema))
        .route("/v1/oc/parse_intent", post(parse_intent))
        .route("/v1/oc/plan_preview", post(plan_preview))
        .route("/v1/oc/commit", post(oc_commit))
        .layer(middleware::from_fn(with_standard_headers))
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

async fn with_standard_headers(req: Request, next: Next) -> Response {
    let request_id = req
        .headers()
        .get("x-request-id")
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_str(&Uuid::new_v4().to_string()).unwrap());

    let mut response = next.run(req).await;
    response
        .headers_mut()
        .insert("x-aurea-api", HeaderValue::from_static("1.0"));
    response.headers_mut().insert("x-request-id", request_id);
    response
}

async fn healthz() -> Json<Value> {
    Json(json!({"ok": true}))
}

async fn submit_work(
    State(state): State<AppState>,
    Json(req): Json<SubmitWorkRequest>,
) -> Result<(HeaderMap, Json<SubmitWorkResponse>), (StatusCode, HeaderMap, Json<Value>)> {
    if let Some(retry_after) = check_tenant_rate_limit(&state, &req.tenant).await {
        let mut headers = HeaderMap::new();
        headers.insert(
            "retry-after",
            HeaderValue::from_str(&retry_after.to_string()).unwrap(),
        );
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            headers,
            api_error(
                "POLICY_BLOCKED",
                "tenant rate limit exceeded for current minute window",
                Some(
                    json!({"tenant": req.tenant, "limit_per_minute": TENANT_RATE_LIMIT_PER_MINUTE}),
                ),
            ),
        ));
    }

    let mut work = WorkUnit::new(req.tenant, req.topic, req.idem_key, req.payload);
    let plan_hash = req
        .plan_hash
        .unwrap_or(work.plan_hash().map_err(internal_error_h)?);
    let idem_key = work.idem_key.clone().unwrap_or(plan_hash.clone());
    work.idem_key = Some(idem_key.clone());

    let accepted = state
        .runtime
        .accept_work(work)
        .await
        .map_err(internal_error_h)?;

    let mut headers = HeaderMap::new();
    headers.insert(
        "x-idempotency-key",
        HeaderValue::from_str(&idem_key).map_err(internal_error_h)?,
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
                error: None,
            }
        }
        AcceptDisposition::DuplicateReceipt { receipt_cid } => {
            headers.insert("x-idempotent-replay", HeaderValue::from_static("true"));
            headers.insert("x-idempotent-in-flight", HeaderValue::from_static("false"));
            headers.insert(
                "x-aurea-receipt-cid",
                HeaderValue::from_str(&receipt_cid).map_err(internal_error_h)?,
            );
            SubmitWorkResponse {
                status: "duplicate".to_string(),
                work_id: accepted.work_id.to_string(),
                receipt_cid: Some(receipt_cid),
                duplicate: true,
                in_flight: false,
                error: Some(ApiErrorPayload {
                    code: "IDEM_DUPLICATE".to_string(),
                    message: "identical submission already completed; returning previous receipt"
                        .to_string(),
                }),
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
                error: Some(ApiErrorPayload {
                    code: "IDEM_DUPLICATE".to_string(),
                    message: "identical submission is already running".to_string(),
                }),
            }
        }
    };

    Ok((headers, Json(response)))
}

async fn check_tenant_rate_limit(state: &AppState, tenant: &str) -> Option<u32> {
    let now = Utc::now();
    let minute_epoch = now.timestamp() / 60;

    let mut map = state.tenant_rate.write().await;
    let slot = map.entry(tenant.to_string()).or_insert(TenantRateWindow {
        minute_epoch,
        count: 0,
    });

    if slot.minute_epoch != minute_epoch {
        slot.minute_epoch = minute_epoch;
        slot.count = 0;
    }

    if slot.count >= TENANT_RATE_LIMIT_PER_MINUTE {
        let next_minute = (minute_epoch + 1) * 60;
        let wait = (next_minute - now.timestamp()).max(1) as u32;
        return Some(wait);
    }

    slot.count += 1;
    None
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
            api_error("NOT_FOUND", "receipt not found", Some(json!({"cid": cid}))),
        )),
    }
}

async fn verify_receipt(
    State(state): State<AppState>,
    Json(req): Json<VerifyReceiptRequest>,
) -> Result<Json<VerifyReceiptResponse>, (StatusCode, Json<Value>)> {
    let receipt = if let Some(receipt) = req.receipt {
        receipt
    } else if let Some(cid) = req.cid {
        state
            .runtime
            .get_receipt(&cid)
            .map_err(internal_error)?
            .ok_or_else(|| {
                (
                    StatusCode::NOT_FOUND,
                    api_error("NOT_FOUND", "receipt not found", Some(json!({"cid": cid}))),
                )
            })?
    } else {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            api_error(
                "SCHEMA_INVALID",
                "provide `cid` or `receipt`",
                Some(json!({"fields": ["cid", "receipt"]})),
            ),
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

    Ok(Json(map_verification(
        check,
        key_policy,
        &receipt.signature.kid,
    )))
}

fn map_verification(
    check: ReceiptVerification,
    key_policy: KeyPolicy,
    kid: &str,
) -> VerifyReceiptResponse {
    let reason = if !check.cid_match {
        Some("receipt cid mismatch".to_string())
    } else if !check.signature_valid {
        Some("invalid receipt signature".to_string())
    } else if !key_policy.known {
        Some("key_id not found in keyring".to_string())
    } else if !key_policy.key_match {
        Some("public key mismatch for key_id".to_string())
    } else if key_policy.revoked {
        Some("key_id is revoked".to_string())
    } else {
        None
    };

    VerifyReceiptResponse {
        ok: check.ok && key_policy.ok(),
        cid_match: check.cid_match,
        signature_valid: check.signature_valid,
        key_known: key_policy.known,
        key_match: key_policy.key_match,
        key_revoked: key_policy.revoked,
        key_id: Some(kid.to_string()),
        reason,
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

async fn anchor_for_day(
    State(state): State<AppState>,
    AxumPath(day): AxumPath<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let receipts = state.runtime.list_receipts().map_err(internal_error)?;
    let mut cids = receipts
        .iter()
        .filter(|r| r.created_at.date_naive().to_string() == day)
        .map(|r| r.cid.clone())
        .collect::<Vec<_>>();
    cids.sort();

    let anchor = anchor_day(&day, &cids);
    Ok(Json(json!({
        "date": anchor.date,
        "root": anchor.root,
        "count": anchor.count,
        "generated_at": anchor.generated_at,
    })))
}

async fn metrics(State(state): State<AppState>) -> Result<String, (StatusCode, Json<Value>)> {
    let metrics = state.runtime.metrics_snapshot().map_err(internal_error)?;
    Ok(render_prometheus(&metrics))
}

fn render_prometheus(metrics: &RuntimeMetrics) -> String {
    let mut out = String::new();

    out.push_str("# HELP queue_depth Current ready queue depth.\n");
    out.push_str("# TYPE queue_depth gauge\n");
    out.push_str(&format!("queue_depth {}\n", metrics.queue_depth));

    out.push_str("# HELP reassigns_total Total expired lease reassignments.\n");
    out.push_str("# TYPE reassigns_total counter\n");
    out.push_str(&format!("reassigns_total {}\n", metrics.reassigns_total));

    out.push_str("# HELP jobs_total Total jobs by lifecycle status.\n");
    out.push_str("# TYPE jobs_total counter\n");
    for (status, total) in &metrics.status_totals {
        out.push_str(&format!("jobs_total{{status=\"{}\"}} {}\n", status, total));
    }

    append_histogram(
        &mut out,
        "ttft_ms",
        "Time to first transition (accepted to assigned) in milliseconds.",
        &metrics.ttft_bucket_counts,
        metrics.ttft_sum_ms,
        metrics.ttft_count,
    );
    append_histogram(
        &mut out,
        "ttr_ms",
        "Time to result (accepted to done/fail) in milliseconds.",
        &metrics.ttr_bucket_counts,
        metrics.ttr_sum_ms,
        metrics.ttr_count,
    );

    let done = metrics.status_totals.get("done").copied().unwrap_or(0);
    let fail = metrics.status_totals.get("fail").copied().unwrap_or(0);
    let total = done + fail;
    let rate = if total == 0 {
        0.0
    } else {
        (fail as f64) / (total as f64)
    };
    out.push_str("# HELP error_rate Failed jobs over completed jobs.\n");
    out.push_str("# TYPE error_rate gauge\n");
    out.push_str(&format!("error_rate {}\n", rate));
    out.push_str(&format!("error_rate{{code=\"runtime_fail\"}} {}\n", rate));

    out.push_str("# HELP stage_time_ms Stage duration sums by stage.\n");
    out.push_str("# TYPE stage_time_ms gauge\n");
    out.push_str(&format!(
        "stage_time_ms{{stage=\"ttft\"}} {}\n",
        metrics.ttft_sum_ms
    ));
    out.push_str(&format!(
        "stage_time_ms{{stage=\"ttr\"}} {}\n",
        metrics.ttr_sum_ms
    ));

    out.push_str("# HELP ux_events_total UX telemetry events.\n");
    out.push_str("# TYPE ux_events_total counter\n");
    for event in [
        "open_plan_card",
        "edit_slot",
        "confirm_commit",
        "cancel",
        "view_receipt",
        "verify_receipt",
    ] {
        out.push_str(&format!("ux_events_total{{event=\"{event}\"}} 0\n"));
    }

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

async fn export_data(
    State(state): State<AppState>,
    Json(req): Json<ExportRequest>,
) -> Result<Json<ExportResponse>, (StatusCode, Json<Value>)> {
    let format = req.format.to_lowercase();
    if !matches!(format.as_str(), "parquet" | "arrow" | "ro-crate") {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            api_error(
                "SCHEMA_INVALID",
                "format must be one of: parquet, arrow, ro-crate",
                Some(json!({"format": req.format})),
            ),
        ));
    }

    let mut receipts = state.runtime.list_receipts().map_err(internal_error)?;
    if let Some(topic) = req.topic {
        receipts.retain(|r| r.topic == topic);
    }

    let export_dir = PathBuf::from("./exports");
    fs::create_dir_all(&export_dir).map_err(internal_error)?;
    let stamp = Utc::now().format("%Y%m%d-%H%M%S");
    let filename = format!("aurea-export-{stamp}.{}.json", format.replace('-', "_"));
    let path = export_dir.join(filename);

    let content = serde_json::to_vec_pretty(&json!({
        "format": format,
        "generated_at": Utc::now().to_rfc3339(),
        "records": receipts.len(),
        "receipts": receipts,
    }))
    .map_err(internal_error)?;
    fs::write(&path, content).map_err(internal_error)?;

    Ok(Json(ExportResponse {
        status: "ok".to_string(),
        format,
        records: state.runtime.list_receipts().map_err(internal_error)?.len(),
        path: path.display().to_string(),
    }))
}

async fn capabilities(State(state): State<AppState>) -> Json<Value> {
    let mut schemas = Vec::new();
    for key in state.schemas.keys() {
        let mut parts = key.split(':');
        let schema_id = parts.next().unwrap_or_default();
        let v = parts.next().unwrap_or("1");
        schemas.push(json!({"schema_id": schema_id, "v": v}));
    }
    schemas.sort_by_key(|a| a.to_string());

    Json(json!({
        "capabilities": state.runtime.capabilities(),
        "oc_actions": ["parse_intent", "plan_preview", "commit"],
        "schemas": schemas,
    }))
}

async fn get_schema(
    State(state): State<AppState>,
    AxumPath((schema_id, v)): AxumPath<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let key = format!("{}:{}", schema_id, v);
    let Some(spec) = state.schemas.get(&key) else {
        return Err((
            StatusCode::NOT_FOUND,
            api_error(
                "NOT_FOUND",
                "schema not found",
                Some(json!({"schema_id": schema_id, "v": v})),
            ),
        ));
    };

    Ok(Json(spec.schema.clone()))
}

async fn parse_intent(
    State(state): State<AppState>,
    Json(req): Json<ParseIntentRequest>,
) -> Result<Json<ParseIntentResponse>, (StatusCode, Json<Value>)> {
    let (schema_id, v) = infer_schema(&req);
    let key = format!("{}:{}", schema_id, v);

    let Some(schema) = state.schemas.get(&key) else {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            api_error(
                "SCHEMA_INVALID",
                "unknown schema_id/version",
                Some(json!({"schema_id": schema_id, "v": v})),
            ),
        ));
    };

    let payload = req
        .payload
        .unwrap_or_else(|| infer_payload_from_text(&req.text));
    ensure_payload_limit(&payload)?;

    let intent = Intent {
        schema_id,
        v,
        topic: req.topic.unwrap_or_else(|| schema.topic.to_string()),
        payload,
    };

    Ok(Json(ParseIntentResponse {
        intent,
        warnings: Vec::new(),
    }))
}

async fn plan_preview(
    State(state): State<AppState>,
    Json(req): Json<PlanPreviewRequest>,
) -> Result<Json<PlanPreviewResponse>, (StatusCode, Json<Value>)> {
    ensure_payload_limit(&req.intent.payload)?;

    let schema_key = format!("{}:{}", req.intent.schema_id, req.intent.v);
    let Some(spec) = state.schemas.get(&schema_key) else {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            api_error(
                "SCHEMA_INVALID",
                "unknown schema_id/version",
                Some(json!({"schema_id": req.intent.schema_id, "v": req.intent.v})),
            ),
        ));
    };

    let missing = missing_paths(&req.intent.payload, spec.required_paths);
    if !missing.is_empty() {
        let repair = RepairRequest {
            hints: missing
                .iter()
                .map(|m| format!("provide required field `{}`", m))
                .collect(),
            missing,
        };

        if req.repair_attempt >= 2 {
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                api_error(
                    "SCHEMA_INVALID",
                    "required fields still missing after repair attempts",
                    Some(json!({"repair_request": repair, "attempt": req.repair_attempt})),
                ),
            ));
        }

        return Ok(Json(PlanPreviewResponse {
            dag: json!({
                "nodes": [],
                "edges": [],
                "reason": "waiting_for_required_slots"
            }),
            plan_hash: "".to_string(),
            policy_trace: Vec::new(),
            slos: default_slos(),
            warnings: vec!["missing required slots".to_string()],
            route: Route::Preferred,
            dual_control_required: false,
            repair_request: Some(repair),
        }));
    }

    let decision = state.policy.evaluate(&json!({
        "topic": req.intent.topic.clone(),
        "payload": req.intent.payload.clone(),
    }));
    if decision.blocked {
        return Err((
            StatusCode::FORBIDDEN,
            api_error(
                "POLICY_BLOCKED",
                "plan blocked during propose stage",
                Some(json!({"policy_trace": decision.trace})),
            ),
        ));
    }

    let dag = plan_dag(&req.intent);
    let dag_bytes =
        to_nrf_bytes(dag.clone(), aurea_core::CanonProfile::default()).map_err(internal_error)?;
    let plan_hash = cid_of(&dag_bytes);

    let preview = PlanPreviewResponse {
        dag,
        plan_hash: plan_hash.clone(),
        policy_trace: decision.trace.clone(),
        slos: default_slos(),
        warnings: Vec::new(),
        route: decision.route.clone(),
        dual_control_required: decision.require_dual_control,
        repair_request: None,
    };

    let mut previews = state.previews.write().await;
    previews.insert(
        plan_hash.clone(),
        StoredPreview {
            intent: req.intent,
            plan_hash,
            policy_trace: decision.trace,
            dual_control_required: decision.require_dual_control,
            route: decision.route,
        },
    );

    Ok(Json(preview))
}

async fn oc_commit(
    State(state): State<AppState>,
    Json(req): Json<OcCommitRequest>,
) -> Result<Json<OcCommitResponse>, (StatusCode, Json<Value>)> {
    let preview = {
        let previews = state.previews.read().await;
        previews.get(&req.plan_hash).cloned()
    }
    .ok_or_else(|| {
        (
            StatusCode::CONFLICT,
            api_error(
                "PLAN_CONFLICT",
                "plan_hash is unknown or expired; regenerate plan_preview",
                Some(json!({"plan_hash": req.plan_hash})),
            ),
        )
    })?;

    if preview.dual_control_required && req.confirm_phrase.as_deref() != Some(DUAL_CONTROL_PHRASE) {
        return Err((
            StatusCode::FORBIDDEN,
            api_error(
                "DUAL_CONTROL_REQUIRED",
                "confirm_phrase must be DUAL_CONTROL for this topic",
                Some(json!({"topic": preview.intent.topic})),
            ),
        ));
    }

    let tenant = req.tenant.unwrap_or_else(|| "default".to_string());
    if let Some(retry_after) = check_tenant_rate_limit(&state, &tenant).await {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            api_error(
                "POLICY_BLOCKED",
                "tenant rate limit exceeded for current minute window",
                Some(json!({"retry_after": retry_after})),
            ),
        ));
    }

    let payload = merge_payload_meta(
        &preview.intent.payload,
        json!({
            "plan_hash": preview.plan_hash,
            "policy_trace": preview.policy_trace,
            "route": preview.route,
            "schema_id": preview.intent.schema_id,
            "schema_v": preview.intent.v,
        }),
    );

    let idem_key = req.idem_key.or_else(|| Some(req.plan_hash.clone()));
    let work = WorkUnit::new(tenant, preview.intent.topic, idem_key, payload);

    let accepted = state
        .runtime
        .accept_work(work)
        .await
        .map_err(internal_error)?;

    let response = match accepted.disposition {
        AcceptDisposition::Enqueued => OcCommitResponse {
            status: "accepted".to_string(),
            work_id: accepted.work_id.to_string(),
            receipt_cid: None,
            duplicate: false,
            in_flight: false,
        },
        AcceptDisposition::DuplicateReceipt { receipt_cid } => OcCommitResponse {
            status: "duplicate".to_string(),
            work_id: accepted.work_id.to_string(),
            receipt_cid: Some(receipt_cid),
            duplicate: true,
            in_flight: false,
        },
        AcceptDisposition::DuplicateInFlight => OcCommitResponse {
            status: "duplicate_in_flight".to_string(),
            work_id: accepted.work_id.to_string(),
            receipt_cid: None,
            duplicate: true,
            in_flight: true,
        },
    };

    Ok(Json(response))
}

fn merge_payload_meta(payload: &Value, meta: Value) -> Value {
    if let Value::Object(existing) = payload {
        let mut cloned = existing.clone();
        cloned.insert("_aurea_meta".to_string(), meta);
        Value::Object(cloned)
    } else {
        json!({"payload": payload, "_aurea_meta": meta})
    }
}

fn infer_schema(req: &ParseIntentRequest) -> (String, String) {
    let schema_id = if let Some(schema_id) = &req.schema_id {
        schema_id.clone()
    } else {
        let lower = req.text.to_ascii_lowercase();
        if lower.contains("vcx") || lower.contains("transcode") {
            "vcx.batch_transcode".to_string()
        } else if lower.contains("hdl") {
            "hdl.sim".to_string()
        } else {
            "science.run".to_string()
        }
    };

    let v = req.v.clone().unwrap_or_else(|| "1".to_string());
    (schema_id, v)
}

fn infer_payload_from_text(text: &str) -> Value {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(obj)) => Value::Object(obj),
        _ => json!({"prompt": text}),
    }
}

fn ensure_payload_limit(payload: &Value) -> Result<(), (StatusCode, Json<Value>)> {
    let size = serde_json::to_vec(payload).map_err(internal_error)?.len();

    if size > MAX_INTENT_PAYLOAD_BYTES {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            api_error(
                "SCHEMA_INVALID",
                "Intent.payload exceeds 256 KiB",
                Some(json!({"limit_bytes": MAX_INTENT_PAYLOAD_BYTES, "actual_bytes": size})),
            ),
        ));
    }

    Ok(())
}

fn missing_paths(payload: &Value, required: &[&str]) -> Vec<String> {
    let Value::Object(map) = payload else {
        return required.iter().map(|s| s.to_string()).collect();
    };

    required
        .iter()
        .filter_map(|path| {
            if map.contains_key(*path) {
                None
            } else {
                Some((*path).to_string())
            }
        })
        .collect()
}

fn plan_dag(intent: &Intent) -> Value {
    json!({
        "@type": "aurea/plan_preview.v1",
        "schema_id": intent.schema_id,
        "schema_v": intent.v,
        "topic": intent.topic,
        "nodes": [
            {"id": "validate", "kind": "schema_check"},
            {"id": "policy", "kind": "policy_eval"},
            {"id": "execute", "kind": "runtime_commit"}
        ],
        "edges": [
            {"from": "validate", "to": "policy"},
            {"from": "policy", "to": "execute"}
        ],
        "payload": intent.payload,
    })
}

fn default_slos() -> Value {
    json!({
        "ttft_p95_ms_max": 4000,
        "ttr_p95_ms_max": 9000,
        "error_rate_max": 0.02
    })
}

fn default_schemas() -> HashMap<String, SchemaSpec> {
    let mut out = HashMap::new();

    out.insert(
        "science.run:1".to_string(),
        SchemaSpec {
            topic: "science:commit",
            required_paths: &["seed", "image", "inputs", "params"],
            schema: json!({
                "$id": "science.run",
                "type": "object",
                "required": ["seed", "image", "inputs", "params"],
                "properties": {
                    "seed": {"type": "integer"},
                    "image": {"type": "string"},
                    "inputs": {"type": "array"},
                    "params": {"type": "object"}
                },
                "x-llm": {"repair": {"max_attempts": 2}},
                "x-ui": {"confirm_phrase": "Conferi e confirmo o plano."}
            }),
        },
    );

    out.insert(
        "vcx.batch_transcode:1".to_string(),
        SchemaSpec {
            topic: "vcx:commit",
            required_paths: &["codec", "width", "height", "bitrate"],
            schema: json!({
                "$id": "vcx.batch_transcode",
                "type": "object",
                "required": ["codec", "width", "height", "bitrate"],
                "properties": {
                    "codec": {"type": "string"},
                    "width": {"type": "integer"},
                    "height": {"type": "integer"},
                    "bitrate": {"type": "integer"}
                },
                "x-llm": {"repair": {"max_attempts": 2}},
                "x-ui": {"confirm_phrase": "Conferi e confirmo o plano."}
            }),
        },
    );

    out.insert(
        "hdl.sim:1".to_string(),
        SchemaSpec {
            topic: "hdl:commit",
            required_paths: &["top", "cycles", "asserts"],
            schema: json!({
                "$id": "hdl.sim",
                "type": "object",
                "required": ["top", "cycles", "asserts"],
                "properties": {
                    "top": {"type": "string"},
                    "cycles": {"type": "integer"},
                    "asserts": {"type": "array"}
                },
                "x-llm": {"repair": {"max_attempts": 2}},
                "x-ui": {"confirm_phrase": "Conferi e confirmo o plano."}
            }),
        },
    );

    out
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

fn api_error(code: &str, message: &str, details: Option<Value>) -> Json<Value> {
    Json(json!({
        "error": {
            "code": code,
            "message": message,
            "details": details,
        }
    }))
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
        api_error("INTERNAL_ERROR", &err.to_string(), None),
    )
}

fn internal_error_h(err: impl std::fmt::Display) -> (StatusCode, HeaderMap, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        HeaderMap::new(),
        api_error("INTERNAL_ERROR", &err.to_string(), None),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_paths_detects_absent_fields() {
        let payload = json!({"a": 1});
        let missing = missing_paths(&payload, &["a", "b"]);
        assert_eq!(missing, vec!["b"]);
    }

    #[test]
    fn merge_payload_meta_preserves_original_object() {
        let payload = json!({"x": 1});
        let merged = merge_payload_meta(&payload, json!({"plan_hash": "abc"}));
        let Value::Object(map) = merged else {
            panic!("expected object payload");
        };
        assert_eq!(map.get("x").unwrap(), &json!(1));
        assert!(map.contains_key("_aurea_meta"));
    }

    #[test]
    fn infer_schema_from_text() {
        let req = ParseIntentRequest {
            text: "please run vcx transcode".to_string(),
            schema_id: None,
            v: None,
            topic: None,
            payload: None,
        };
        let (schema_id, v) = infer_schema(&req);
        assert_eq!(schema_id, "vcx.batch_transcode");
        assert_eq!(v, "1");
    }
}
