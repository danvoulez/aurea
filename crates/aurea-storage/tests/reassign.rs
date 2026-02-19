use std::time::Duration;

use aurea_core::WorkUnit;
use aurea_storage::{EnqueueResult, RedbStore};
use serde_json::json;
use uuid::Uuid;

#[test]
fn expired_lease_is_reassigned() {
    let path = std::env::temp_dir().join(format!("aurea-storage-{}.redb", Uuid::new_v4()));
    let _ = std::fs::remove_file(&path);

    let store = RedbStore::open(&path).expect("open redb");

    let enqueued = store
        .enqueue_work_idempotent(WorkUnit::new(
            "tenant".to_string(),
            "echo:test".to_string(),
            Some("idem-1".to_string()),
            json!({"value": 1}),
        ))
        .expect("enqueue work");
    assert!(matches!(enqueued, EnqueueResult::Enqueued { .. }));

    let first = store
        .lease_next(1)
        .expect("lease first")
        .expect("job exists");

    std::thread::sleep(Duration::from_millis(5));

    let moved = store
        .reassign_expired_leases()
        .expect("reassign expired leases");
    assert_eq!(moved, 1);

    let second = store
        .lease_next(1000)
        .expect("lease second")
        .expect("job exists again");
    assert_eq!(second.seq, first.seq);
    assert!(second.attempt >= 2);

    let metrics = store.queue_metrics().expect("queue metrics");
    assert_eq!(metrics.reassigns_total, 1);

    let _ = std::fs::remove_file(&path);
}
