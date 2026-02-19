use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use aurea_core::{
    PolicyEntry, Receipt, ReceiptSignature, UnsignedReceipt, WorkStatus, WorkUnit, cid_for,
};
use aurea_plugins::PluginRegistry;
use aurea_storage::{EnqueueResult, QueuedJob, RedbStore};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::{debug, error};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamEvent {
    pub at: DateTime<Utc>,
    pub tenant: String,
    pub topic: String,
    pub work_id: Uuid,
    pub status: WorkStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt_cid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone)]
pub enum AcceptDisposition {
    Enqueued,
    DuplicateReceipt { receipt_cid: String },
    DuplicateInFlight,
}

#[derive(Debug, Clone)]
pub struct AcceptedWork {
    pub work_id: Uuid,
    pub disposition: AcceptDisposition,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReceiptVerification {
    pub ok: bool,
    pub cid_match: bool,
    pub signature_valid: bool,
}

#[derive(Clone)]
pub struct Runtime {
    store: RedbStore,
    plugins: PluginRegistry,
    events_tx: broadcast::Sender<StreamEvent>,
    signer: Arc<SigningKey>,
    kid: String,
    lease_ttl_ms: u64,
    worker_tick_ms: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct RuntimeConfig {
    pub lease_ttl_ms: u64,
    pub worker_tick_ms: u64,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            lease_ttl_ms: 15_000,
            worker_tick_ms: 150,
        }
    }
}

impl Runtime {
    pub fn new(store: RedbStore, plugins: PluginRegistry) -> Self {
        let signer = SigningKey::generate(&mut OsRng);
        let kid = Utc::now().format("%Y%m%d-%H%M%S").to_string();
        Self::new_with_signer_and_config(store, plugins, signer, kid, RuntimeConfig::default())
    }

    pub fn new_with_signer(
        store: RedbStore,
        plugins: PluginRegistry,
        signer: SigningKey,
        kid: String,
    ) -> Self {
        Self::new_with_signer_and_config(store, plugins, signer, kid, RuntimeConfig::default())
    }

    pub fn new_with_signer_and_config(
        store: RedbStore,
        plugins: PluginRegistry,
        signer: SigningKey,
        kid: String,
        config: RuntimeConfig,
    ) -> Self {
        let (events_tx, _) = broadcast::channel(2048);
        Self {
            store,
            plugins,
            events_tx,
            signer: Arc::new(signer),
            kid,
            lease_ttl_ms: config.lease_ttl_ms,
            worker_tick_ms: config.worker_tick_ms,
        }
    }

    pub fn start_background_worker(&self) -> tokio::task::JoinHandle<()> {
        let runtime = self.clone();
        tokio::spawn(async move {
            loop {
                if let Err(err) = runtime.tick_once().await {
                    error!(error = %err, "runtime tick failed");
                }
                tokio::time::sleep(Duration::from_millis(runtime.worker_tick_ms)).await;
            }
        })
    }

    pub async fn accept_work(&self, mut work: WorkUnit) -> Result<AcceptedWork> {
        let idem_key = work
            .effective_idem_key()
            .context("failed to compute idem_key")?;
        work.idem_key = Some(idem_key);

        let outcome = self
            .store
            .enqueue_work_idempotent(work.clone())
            .context("enqueue idempotent failed")?;

        match outcome {
            EnqueueResult::Enqueued { seq: _, work_id } => {
                self.store.increment_status_counter(WorkStatus::Accepted)?;
                self.emit_event(StreamEvent {
                    at: Utc::now(),
                    tenant: work.tenant,
                    topic: work.topic,
                    work_id,
                    status: WorkStatus::Accepted,
                    receipt_cid: None,
                    detail: None,
                });
                Ok(AcceptedWork {
                    work_id,
                    disposition: AcceptDisposition::Enqueued,
                })
            }
            EnqueueResult::DuplicateReceipt {
                work_id,
                receipt_cid,
            } => Ok(AcceptedWork {
                work_id,
                disposition: AcceptDisposition::DuplicateReceipt { receipt_cid },
            }),
            EnqueueResult::DuplicateInFlight { work_id } => Ok(AcceptedWork {
                work_id,
                disposition: AcceptDisposition::DuplicateInFlight,
            }),
        }
    }

    async fn tick_once(&self) -> Result<()> {
        let reassigned = self.store.reassign_expired_leases()?;
        if reassigned > 0 {
            debug!(reassigned, "reassigned expired leases");
        }

        let Some(job) = self.store.lease_next(self.lease_ttl_ms)? else {
            return Ok(());
        };

        self.store.increment_status_counter(WorkStatus::Assigned)?;
        self.emit_event(StreamEvent {
            at: Utc::now(),
            tenant: job.work.tenant.clone(),
            topic: job.work.topic.clone(),
            work_id: job.work.id,
            status: WorkStatus::Assigned,
            receipt_cid: None,
            detail: None,
        });

        self.store.increment_status_counter(WorkStatus::Progress)?;
        self.emit_event(StreamEvent {
            at: Utc::now(),
            tenant: job.work.tenant.clone(),
            topic: job.work.topic.clone(),
            work_id: job.work.id,
            status: WorkStatus::Progress,
            receipt_cid: None,
            detail: Some("plugin execution started".to_string()),
        });

        let execute_result = self.execute_job(&job).await;
        let (status, detail) = match execute_result {
            Ok(()) => (WorkStatus::Done, None),
            Err(err) => (WorkStatus::Fail, Some(err.to_string())),
        };

        let mut policy_trace = vec![PolicyEntry {
            rule: "baseline_accept".to_string(),
            ok: true,
            detail: Some("work accepted into runtime".to_string()),
        }];
        if let Some(d) = detail.clone() {
            policy_trace.push(PolicyEntry {
                rule: "runtime_execute".to_string(),
                ok: false,
                detail: Some(d),
            });
        }

        let assigned_at = job.leased_at.unwrap_or_else(Utc::now);
        let done_at = Utc::now();
        let ttft_ms = (assigned_at - job.accepted_at).num_milliseconds().max(0) as u64;
        let ttr_ms = (done_at - job.accepted_at).num_milliseconds().max(0) as u64;

        let receipt = self.sign_receipt(&job, status, policy_trace, ttft_ms, ttr_ms, done_at)?;
        self.store.put_receipt(&receipt)?;
        self.store.complete_leased(job.seq)?;
        self.store.observe_timings(ttft_ms, ttr_ms)?;
        self.store.increment_status_counter(status)?;

        self.emit_event(StreamEvent {
            at: Utc::now(),
            tenant: job.work.tenant,
            topic: job.work.topic,
            work_id: job.work.id,
            status,
            receipt_cid: Some(receipt.cid.clone()),
            detail,
        });

        Ok(())
    }

    async fn execute_job(&self, job: &QueuedJob) -> Result<()> {
        let plugin_name = job.work.topic.split(':').next().unwrap_or("echo");
        let plugin = self
            .plugins
            .get(plugin_name)
            .ok_or_else(|| anyhow!("plugin not found: {plugin_name}"))?;
        let _ = plugin.execute(job.work.payload.clone()).await?;
        Ok(())
    }

    fn sign_receipt(
        &self,
        job: &QueuedJob,
        status: WorkStatus,
        policy_trace: Vec<PolicyEntry>,
        ttft_ms: u64,
        ttr_ms: u64,
        created_at: DateTime<Utc>,
    ) -> Result<Receipt> {
        let idem_key = job
            .work
            .idem_key
            .clone()
            .ok_or_else(|| anyhow!("queued job is missing idem_key"))?;
        let plan_hash = job.work.plan_hash()?;

        let mut stage_time_ms = BTreeMap::new();
        stage_time_ms.insert("ttft_ms".to_string(), ttft_ms);
        stage_time_ms.insert("ttr_ms".to_string(), ttr_ms);

        let unsigned = UnsignedReceipt {
            work_id: job.work.id,
            tenant: job.work.tenant.clone(),
            topic: job.work.topic.clone(),
            status,
            idem_key,
            plan_hash,
            policy_trace,
            stage_time_ms,
            artifacts: vec![],
            created_at,
        };

        let cid = cid_for(&unsigned)?;
        let sig = self.signer.sign(cid.as_bytes());
        let vk = self.signer.verifying_key();

        let signature = ReceiptSignature {
            alg: "ed25519".to_string(),
            kid: self.kid.clone(),
            public_key: B64.encode(vk.to_bytes()),
            signature: B64.encode(sig.to_bytes()),
        };

        Ok(Receipt {
            cid,
            work_id: unsigned.work_id,
            tenant: unsigned.tenant,
            topic: unsigned.topic,
            status: unsigned.status,
            idem_key: unsigned.idem_key,
            plan_hash: unsigned.plan_hash,
            policy_trace: unsigned.policy_trace,
            stage_time_ms: unsigned.stage_time_ms,
            artifacts: unsigned.artifacts,
            created_at: unsigned.created_at,
            signature,
        })
    }

    fn emit_event(&self, event: StreamEvent) {
        let _ = self.events_tx.send(event);
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<StreamEvent> {
        self.events_tx.subscribe()
    }

    pub fn get_receipt(&self, cid: &str) -> Result<Option<Receipt>> {
        self.store.get_receipt(cid)
    }

    pub fn verify_receipt(&self, receipt: &Receipt) -> Result<ReceiptVerification> {
        let cid_match = receipt.cid_matches()?;
        let signature_valid = verify_signature(receipt)?;

        Ok(ReceiptVerification {
            ok: cid_match && signature_valid,
            cid_match,
            signature_valid,
        })
    }

    pub fn verify_receipt_by_cid(&self, cid: &str) -> Result<Option<ReceiptVerification>> {
        let Some(receipt) = self.store.get_receipt(cid)? else {
            return Ok(None);
        };
        Ok(Some(self.verify_receipt(&receipt)?))
    }

    pub fn metrics_snapshot(&self) -> Result<RuntimeMetrics> {
        let metrics = self.store.queue_metrics()?;
        Ok(RuntimeMetrics {
            queue_depth: metrics.queue_depth,
            leased_depth: metrics.leased_depth,
            reassigns_total: metrics.reassigns_total,
            receipts_total: metrics.receipts_total,
            plugins_total: self.plugins.names().len(),
            status_totals: metrics.status_totals,
            ttft_sum_ms: metrics.ttft_sum_ms,
            ttft_count: metrics.ttft_count,
            ttr_sum_ms: metrics.ttr_sum_ms,
            ttr_count: metrics.ttr_count,
            ttft_bucket_counts: metrics.ttft_bucket_counts,
            ttr_bucket_counts: metrics.ttr_bucket_counts,
        })
    }

    pub fn capabilities(&self) -> Vec<String> {
        self.plugins.names()
    }
}

fn verify_signature(receipt: &Receipt) -> Result<bool> {
    if receipt.signature.alg != "ed25519" {
        return Ok(false);
    }

    let vk_bytes = B64
        .decode(receipt.signature.public_key.as_bytes())
        .context("invalid base64 public key")?;
    let sig_bytes = B64
        .decode(receipt.signature.signature.as_bytes())
        .context("invalid base64 signature")?;

    let vk_array: [u8; 32] = vk_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow!("invalid public key length"))?;
    let sig_array: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow!("invalid signature length"))?;

    let verifying_key = VerifyingKey::from_bytes(&vk_array).context("invalid verifying key")?;
    let signature = Signature::from_bytes(&sig_array);

    Ok(verifying_key
        .verify(receipt.cid.as_bytes(), &signature)
        .is_ok())
}

#[derive(Debug, Clone)]
pub struct RuntimeMetrics {
    pub queue_depth: usize,
    pub leased_depth: usize,
    pub reassigns_total: u64,
    pub receipts_total: usize,
    pub plugins_total: usize,
    pub status_totals: BTreeMap<String, u64>,
    pub ttft_sum_ms: u64,
    pub ttft_count: u64,
    pub ttr_sum_ms: u64,
    pub ttr_count: u64,
    pub ttft_bucket_counts: Vec<(u64, u64)>,
    pub ttr_bucket_counts: Vec<(u64, u64)>,
}
