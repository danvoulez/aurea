use std::time::Duration;

use async_trait::async_trait;
use aurea_core::{WorkStatus, WorkUnit};
use aurea_plugins::{Plugin, PluginRegistry};
use aurea_runtime::{AcceptDisposition, Runtime, RuntimeConfig};
use aurea_storage::RedbStore;
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use serde_json::{Value, json};
use tokio::time::{sleep, timeout};
use uuid::Uuid;

struct SlowPlugin;

#[async_trait]
impl Plugin for SlowPlugin {
    fn name(&self) -> &'static str {
        "slow"
    }

    async fn execute(&self, payload: Value) -> anyhow::Result<Value> {
        sleep(Duration::from_millis(300)).await;
        Ok(payload)
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn duplicate_in_flight_then_duplicate_receipt() {
    let path = std::env::temp_dir().join(format!("aurea-runtime-{}.redb", Uuid::new_v4()));
    let _ = std::fs::remove_file(&path);

    let store = RedbStore::open(&path).expect("open redb");
    let mut registry = PluginRegistry::new();
    registry.register(SlowPlugin);

    let signer = SigningKey::generate(&mut OsRng);
    let runtime = Runtime::new_with_signer_and_config(
        store,
        registry,
        signer,
        "test-kid".to_string(),
        RuntimeConfig {
            lease_ttl_ms: 5_000,
            worker_tick_ms: 20,
        },
    );

    let worker = runtime.start_background_worker();
    let mut events = runtime.subscribe_events();

    let first = runtime
        .accept_work(WorkUnit::new(
            "demo".to_string(),
            "slow:test".to_string(),
            Some("fixed-idem".to_string()),
            json!({"x": 1}),
        ))
        .await
        .expect("submit first work");
    assert!(matches!(first.disposition, AcceptDisposition::Enqueued));

    let second = runtime
        .accept_work(WorkUnit::new(
            "demo".to_string(),
            "slow:test".to_string(),
            Some("fixed-idem".to_string()),
            json!({"x": 1}),
        ))
        .await
        .expect("submit duplicate work while in flight");
    assert!(matches!(
        second.disposition,
        AcceptDisposition::DuplicateInFlight
    ));

    let done_cid = timeout(Duration::from_secs(5), async {
        loop {
            let evt = events.recv().await.expect("sse event");
            if evt.status == WorkStatus::Done {
                return evt.receipt_cid.expect("done event has receipt_cid");
            }
        }
    })
    .await
    .expect("timed out waiting for done event");

    let third = runtime
        .accept_work(WorkUnit::new(
            "demo".to_string(),
            "slow:test".to_string(),
            Some("fixed-idem".to_string()),
            json!({"x": 1}),
        ))
        .await
        .expect("submit duplicate work after completion");

    match third.disposition {
        AcceptDisposition::DuplicateReceipt { receipt_cid } => {
            assert_eq!(receipt_cid, done_cid);
        }
        other => panic!("expected DuplicateReceipt, got {other:?}"),
    }

    worker.abort();
    let _ = std::fs::remove_file(&path);
}
