use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use aurea_core::{Receipt, WorkStatus, WorkUnit};
use chrono::{DateTime, Duration, Utc};
use redb::{Database, ReadOnlyTable, ReadableTable, Table, TableDefinition};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const READY_JOBS: TableDefinition<u64, &[u8]> = TableDefinition::new("ready_jobs");
const LEASED_JOBS: TableDefinition<u64, &[u8]> = TableDefinition::new("leased_jobs");
const RECEIPTS: TableDefinition<&str, &[u8]> = TableDefinition::new("receipts");
const IDEM_KEYS: TableDefinition<&str, &[u8]> = TableDefinition::new("idem_keys");
const META: TableDefinition<&str, u64> = TableDefinition::new("meta");

const META_NEXT_SEQ: &str = "next_job_seq";
const META_REASSIGNS_TOTAL: &str = "reassigns_total";
const META_TTFT_SUM_MS: &str = "ttft_sum_ms";
const META_TTFT_COUNT: &str = "ttft_count";
const META_TTR_SUM_MS: &str = "ttr_sum_ms";
const META_TTR_COUNT: &str = "ttr_count";

const TTFT_BUCKETS_MS: [u64; 7] = [100, 250, 500, 1000, 2000, 4000, 10000];
const TTR_BUCKETS_MS: [u64; 7] = [500, 1000, 2000, 4000, 9000, 20000, 60000];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedJob {
    pub seq: u64,
    pub work: WorkUnit,
    pub attempt: u32,
    pub accepted_at: DateTime<Utc>,
    pub leased_at: Option<DateTime<Utc>>,
    pub lease_expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IdemRecord {
    tenant: String,
    topic: String,
    idem_key: String,
    work_id: Uuid,
    status: String,
    receipt_cid: Option<String>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub enum EnqueueResult {
    Enqueued { seq: u64, work_id: Uuid },
    DuplicateReceipt { work_id: Uuid, receipt_cid: String },
    DuplicateInFlight { work_id: Uuid },
}

#[derive(Debug, Clone)]
pub struct QueueMetrics {
    pub queue_depth: usize,
    pub leased_depth: usize,
    pub reassigns_total: u64,
    pub receipts_total: usize,
    pub status_totals: BTreeMap<String, u64>,
    pub ttft_sum_ms: u64,
    pub ttft_count: u64,
    pub ttr_sum_ms: u64,
    pub ttr_count: u64,
    pub ttft_bucket_counts: Vec<(u64, u64)>,
    pub ttr_bucket_counts: Vec<(u64, u64)>,
}

#[derive(Clone)]
pub struct RedbStore {
    db: Arc<Database>,
}

impl RedbStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let db = if path.exists() {
            Database::open(path).context("failed to open redb")?
        } else {
            Database::create(path).context("failed to create redb")?
        };

        let this = Self { db: Arc::new(db) };
        this.init_tables()?;
        Ok(this)
    }

    fn init_tables(&self) -> Result<()> {
        let write = self
            .db
            .begin_write()
            .context("failed to begin redb write tx")?;
        write
            .open_table(READY_JOBS)
            .context("failed to open ready_jobs table")?;
        write
            .open_table(LEASED_JOBS)
            .context("failed to open leased_jobs table")?;
        write
            .open_table(RECEIPTS)
            .context("failed to open receipts table")?;
        write
            .open_table(IDEM_KEYS)
            .context("failed to open idem_keys table")?;
        write
            .open_table(META)
            .context("failed to open meta table")?;
        write.commit().context("failed to commit init tx")?;
        Ok(())
    }

    pub fn enqueue_work_idempotent(&self, work: WorkUnit) -> Result<EnqueueResult> {
        let idem_key = work
            .idem_key
            .clone()
            .ok_or_else(|| anyhow!("work.idem_key must be set before enqueue"))?;
        let idem_lookup = idem_lookup_key(&work.tenant, &work.topic, &idem_key);

        let write = self
            .db
            .begin_write()
            .context("failed to begin enqueue tx")?;

        let existing = {
            let idem = write
                .open_table(IDEM_KEYS)
                .context("open idem_keys failed")?;
            match idem
                .get(idem_lookup.as_str())
                .context("read idem key failed")?
            {
                Some(value) => {
                    let record: IdemRecord = serde_json::from_slice(value.value())
                        .context("deserialize idem record failed")?;
                    Some(record)
                }
                None => None,
            }
        };

        if let Some(record) = existing {
            write.commit().context("commit idem hit tx failed")?;
            if let Some(receipt_cid) = record.receipt_cid {
                return Ok(EnqueueResult::DuplicateReceipt {
                    work_id: record.work_id,
                    receipt_cid,
                });
            }
            return Ok(EnqueueResult::DuplicateInFlight {
                work_id: record.work_id,
            });
        }

        let seq = {
            let mut meta = write.open_table(META).context("open meta failed")?;
            let current = meta
                .get(META_NEXT_SEQ)
                .context("read next seq failed")?
                .map(|g| g.value())
                .unwrap_or(1);
            meta.insert(META_NEXT_SEQ, current + 1)
                .context("write next seq failed")?;
            current
        };

        let job = QueuedJob {
            seq,
            work: work.clone(),
            attempt: 0,
            accepted_at: Utc::now(),
            leased_at: None,
            lease_expires_at: None,
        };
        let job_bytes = serde_json::to_vec(&job).context("serialize queued job failed")?;

        {
            let mut ready = write
                .open_table(READY_JOBS)
                .context("open ready_jobs failed")?;
            ready
                .insert(seq, job_bytes.as_slice())
                .context("insert ready job failed")?;
        }

        {
            let record = IdemRecord {
                tenant: work.tenant.clone(),
                topic: work.topic.clone(),
                idem_key,
                work_id: work.id,
                status: "queued".to_string(),
                receipt_cid: None,
                updated_at: Utc::now(),
            };
            let record_bytes =
                serde_json::to_vec(&record).context("serialize idem record failed")?;
            let mut idem = write
                .open_table(IDEM_KEYS)
                .context("open idem_keys failed")?;
            idem.insert(idem_lookup.as_str(), record_bytes.as_slice())
                .context("insert idem record failed")?;
        }

        write.commit().context("commit enqueue tx failed")?;
        Ok(EnqueueResult::Enqueued {
            seq,
            work_id: work.id,
        })
    }

    pub fn lease_next(&self, lease_ttl_ms: u64) -> Result<Option<QueuedJob>> {
        let write = self.db.begin_write().context("begin lease tx failed")?;
        let first = {
            let ready = write
                .open_table(READY_JOBS)
                .context("open ready_jobs failed")?;
            let mut iter = ready.iter().context("iterate ready jobs failed")?;
            match iter
                .next()
                .transpose()
                .context("read ready iterator entry failed")?
            {
                Some((key, value)) => Some((key.value(), value.value().to_vec())),
                None => None,
            }
        };

        let Some((seq, job_bytes)) = first else {
            write.commit().context("commit empty lease tx failed")?;
            return Ok(None);
        };

        {
            let mut ready = write
                .open_table(READY_JOBS)
                .context("open ready_jobs failed")?;
            ready.remove(seq).context("remove ready job failed")?;
        }

        let mut job: QueuedJob =
            serde_json::from_slice(&job_bytes).context("deserialize ready queued job failed")?;
        let now = Utc::now();
        job.attempt += 1;
        job.leased_at = Some(now);
        job.lease_expires_at = Some(now + Duration::milliseconds(lease_ttl_ms as i64));

        let bytes = serde_json::to_vec(&job).context("serialize leased job failed")?;
        {
            let mut leased = write
                .open_table(LEASED_JOBS)
                .context("open leased_jobs failed")?;
            leased
                .insert(seq, bytes.as_slice())
                .context("insert leased job failed")?;
        }

        write.commit().context("commit lease tx failed")?;
        Ok(Some(job))
    }

    pub fn complete_leased(&self, seq: u64) -> Result<()> {
        let write = self.db.begin_write().context("begin complete tx failed")?;
        {
            let mut leased = write
                .open_table(LEASED_JOBS)
                .context("open leased_jobs failed")?;
            leased.remove(seq).context("remove leased job failed")?;
        }
        write.commit().context("commit complete tx failed")?;
        Ok(())
    }

    pub fn reassign_expired_leases(&self) -> Result<u64> {
        let write = self.db.begin_write().context("begin reassign tx failed")?;
        let now = Utc::now();
        let mut to_move = Vec::new();

        {
            let leased = write
                .open_table(LEASED_JOBS)
                .context("open leased_jobs failed")?;
            for entry in leased.iter().context("iterate leased jobs failed")? {
                let (key, value) = entry.context("read leased iterator entry failed")?;
                let seq = key.value();
                let job: QueuedJob = serde_json::from_slice(value.value())
                    .context("deserialize leased job failed")?;
                if job.lease_expires_at.is_some_and(|expires| expires <= now) {
                    to_move.push((seq, job));
                }
            }
        }

        if to_move.is_empty() {
            write.commit().context("commit no-op reassign tx failed")?;
            return Ok(0);
        }

        let reassigned = to_move.len() as u64;
        {
            let mut leased = write
                .open_table(LEASED_JOBS)
                .context("open leased_jobs failed")?;
            let mut ready = write
                .open_table(READY_JOBS)
                .context("open ready_jobs failed")?;

            for (seq, mut job) in to_move {
                leased.remove(seq).context("remove expired lease failed")?;
                job.leased_at = None;
                job.lease_expires_at = None;
                let bytes = serde_json::to_vec(&job).context("serialize reassigned job failed")?;
                ready
                    .insert(seq, bytes.as_slice())
                    .context("reinsert ready job failed")?;
            }
        }

        {
            let mut meta = write.open_table(META).context("open meta failed")?;
            inc_counter(&mut meta, META_REASSIGNS_TOTAL, reassigned)?;
        }

        write.commit().context("commit reassign tx failed")?;
        Ok(reassigned)
    }

    pub fn put_receipt(&self, receipt: &Receipt) -> Result<()> {
        let write = self.db.begin_write().context("begin receipt tx failed")?;

        {
            let bytes = serde_json::to_vec(receipt).context("serialize receipt failed")?;
            let mut table = write.open_table(RECEIPTS).context("open receipts failed")?;
            table
                .insert(receipt.cid.as_str(), bytes.as_slice())
                .context("insert receipt failed")?;
        }

        {
            let key = idem_lookup_key(&receipt.tenant, &receipt.topic, &receipt.idem_key);
            let mut idem = write
                .open_table(IDEM_KEYS)
                .context("open idem_keys failed")?;
            let existing = idem
                .get(key.as_str())
                .context("read idem record failed")?
                .map(|v| serde_json::from_slice::<IdemRecord>(v.value()))
                .transpose()
                .context("deserialize idem record failed")?;

            let mut record = existing.unwrap_or(IdemRecord {
                tenant: receipt.tenant.clone(),
                topic: receipt.topic.clone(),
                idem_key: receipt.idem_key.clone(),
                work_id: receipt.work_id,
                status: "queued".to_string(),
                receipt_cid: None,
                updated_at: Utc::now(),
            });

            record.status = match receipt.status {
                WorkStatus::Done => "done".to_string(),
                WorkStatus::Fail => "fail".to_string(),
                WorkStatus::Accepted => "accepted".to_string(),
                WorkStatus::Assigned => "assigned".to_string(),
                WorkStatus::Progress => "progress".to_string(),
            };
            record.receipt_cid = Some(receipt.cid.clone());
            record.updated_at = Utc::now();

            let bytes = serde_json::to_vec(&record).context("serialize idem record failed")?;
            idem.insert(key.as_str(), bytes.as_slice())
                .context("upsert idem record failed")?;
        }

        write.commit().context("commit receipt tx failed")?;
        Ok(())
    }

    pub fn get_receipt(&self, cid: &str) -> Result<Option<Receipt>> {
        let read = self.db.begin_read().context("begin read tx failed")?;
        let table = read.open_table(RECEIPTS).context("open receipts failed")?;
        let Some(bytes) = table.get(cid).context("read receipt by cid failed")? else {
            return Ok(None);
        };
        let receipt: Receipt =
            serde_json::from_slice(bytes.value()).context("deserialize receipt failed")?;
        Ok(Some(receipt))
    }

    pub fn increment_status_counter(&self, status: WorkStatus) -> Result<()> {
        let write = self
            .db
            .begin_write()
            .context("begin status counter tx failed")?;
        {
            let mut meta = write.open_table(META).context("open meta failed")?;
            inc_counter(&mut meta, status_meta_key(status), 1)?;
        }
        write.commit().context("commit status counter tx failed")?;
        Ok(())
    }

    pub fn observe_timings(&self, ttft_ms: u64, ttr_ms: u64) -> Result<()> {
        let write = self.db.begin_write().context("begin timings tx failed")?;
        {
            let mut meta = write.open_table(META).context("open meta failed")?;
            inc_counter(&mut meta, META_TTFT_SUM_MS, ttft_ms)?;
            inc_counter(&mut meta, META_TTFT_COUNT, 1)?;
            inc_counter(&mut meta, META_TTR_SUM_MS, ttr_ms)?;
            inc_counter(&mut meta, META_TTR_COUNT, 1)?;
            observe_histogram(&mut meta, "ttft", &TTFT_BUCKETS_MS, ttft_ms)?;
            observe_histogram(&mut meta, "ttr", &TTR_BUCKETS_MS, ttr_ms)?;
        }
        write.commit().context("commit timings tx failed")?;
        Ok(())
    }

    pub fn queue_metrics(&self) -> Result<QueueMetrics> {
        let read = self
            .db
            .begin_read()
            .context("begin metrics read tx failed")?;

        let ready = read
            .open_table(READY_JOBS)
            .context("open ready_jobs failed")?;
        let leased = read
            .open_table(LEASED_JOBS)
            .context("open leased_jobs failed")?;
        let receipts = read.open_table(RECEIPTS).context("open receipts failed")?;
        let meta = read.open_table(META).context("open meta failed")?;

        let queue_depth = ready.iter().context("iterate ready jobs failed")?.count();
        let leased_depth = leased.iter().context("iterate leased jobs failed")?.count();
        let receipts_total = receipts.iter().context("iterate receipts failed")?.count();
        let reassigns_total = meta_get_read(&meta, META_REASSIGNS_TOTAL)?;

        let mut status_totals = BTreeMap::new();
        for status in [
            WorkStatus::Accepted,
            WorkStatus::Assigned,
            WorkStatus::Progress,
            WorkStatus::Done,
            WorkStatus::Fail,
        ] {
            status_totals.insert(
                status_label(status).to_string(),
                meta_get_read(&meta, status_meta_key(status))?,
            );
        }

        let ttft_sum_ms = meta_get_read(&meta, META_TTFT_SUM_MS)?;
        let ttft_count = meta_get_read(&meta, META_TTFT_COUNT)?;
        let ttr_sum_ms = meta_get_read(&meta, META_TTR_SUM_MS)?;
        let ttr_count = meta_get_read(&meta, META_TTR_COUNT)?;

        let mut ttft_bucket_counts = Vec::with_capacity(TTFT_BUCKETS_MS.len());
        for le in TTFT_BUCKETS_MS {
            let key = bucket_key("ttft", le);
            ttft_bucket_counts.push((le, meta_get_read(&meta, &key)?));
        }

        let mut ttr_bucket_counts = Vec::with_capacity(TTR_BUCKETS_MS.len());
        for le in TTR_BUCKETS_MS {
            let key = bucket_key("ttr", le);
            ttr_bucket_counts.push((le, meta_get_read(&meta, &key)?));
        }

        Ok(QueueMetrics {
            queue_depth,
            leased_depth,
            reassigns_total,
            receipts_total,
            status_totals,
            ttft_sum_ms,
            ttft_count,
            ttr_sum_ms,
            ttr_count,
            ttft_bucket_counts,
            ttr_bucket_counts,
        })
    }
}

fn status_meta_key(status: WorkStatus) -> &'static str {
    match status {
        WorkStatus::Accepted => "jobs_total_accepted",
        WorkStatus::Assigned => "jobs_total_assigned",
        WorkStatus::Progress => "jobs_total_progress",
        WorkStatus::Done => "jobs_total_done",
        WorkStatus::Fail => "jobs_total_fail",
    }
}

fn status_label(status: WorkStatus) -> &'static str {
    match status {
        WorkStatus::Accepted => "accepted",
        WorkStatus::Assigned => "assigned",
        WorkStatus::Progress => "progress",
        WorkStatus::Done => "done",
        WorkStatus::Fail => "fail",
    }
}

fn idem_lookup_key(tenant: &str, topic: &str, idem_key: &str) -> String {
    format!("{tenant}\u{001F}{topic}\u{001F}{idem_key}")
}

fn bucket_key(prefix: &str, le: u64) -> String {
    format!("hist_{prefix}_le_{le}")
}

fn meta_get_read(meta: &ReadOnlyTable<&str, u64>, key: &str) -> Result<u64> {
    Ok(meta
        .get(key)
        .context("read meta key failed")?
        .map(|v| v.value())
        .unwrap_or(0))
}

fn inc_counter(meta: &mut Table<&str, u64>, key: &str, delta: u64) -> Result<()> {
    let current = meta
        .get(key)
        .context("read meta key failed")?
        .map(|v| v.value())
        .unwrap_or(0);
    meta.insert(key, current.saturating_add(delta))
        .context("write meta key failed")?;
    Ok(())
}

fn observe_histogram(
    meta: &mut Table<&str, u64>,
    prefix: &str,
    buckets: &[u64],
    value: u64,
) -> Result<()> {
    for le in buckets {
        if value <= *le {
            let key = bucket_key(prefix, *le);
            inc_counter(meta, &key, 1)?;
        }
    }
    Ok(())
}
