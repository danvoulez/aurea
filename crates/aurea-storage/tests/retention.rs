use std::collections::BTreeMap;

use aurea_core::{Receipt, ReceiptSignature, WorkStatus, WorkUnit};
use aurea_storage::{EnqueueResult, RedbStore};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

#[test]
fn purge_receipts_removes_receipt_and_idem_record() {
    let path =
        std::env::temp_dir().join(format!("aurea-storage-retention-{}.redb", Uuid::new_v4()));
    let _ = std::fs::remove_file(&path);

    let store = RedbStore::open(&path).expect("open redb");

    let receipt = Receipt {
        cid: "b3-test-receipt-cid".to_string(),
        work_id: Uuid::new_v4(),
        tenant: "tenant".to_string(),
        topic: "echo:test".to_string(),
        status: WorkStatus::Done,
        idem_key: "idem-1".to_string(),
        plan_hash: "plan-1".to_string(),
        policy_trace: vec![],
        stage_time_ms: BTreeMap::new(),
        artifacts: vec![],
        created_at: Utc::now(),
        signature: ReceiptSignature {
            alg: "ed25519".to_string(),
            kid: "kid-1".to_string(),
            public_key: "pk".to_string(),
            signature: "sig".to_string(),
        },
    };
    store.put_receipt(&receipt).expect("insert receipt");

    let duplicate = store
        .enqueue_work_idempotent(WorkUnit::new(
            "tenant".to_string(),
            "echo:test".to_string(),
            Some("idem-1".to_string()),
            json!({"x": 1}),
        ))
        .expect("enqueue duplicate");
    assert!(matches!(duplicate, EnqueueResult::DuplicateReceipt { .. }));

    let report = store
        .purge_receipts(std::slice::from_ref(&receipt))
        .expect("purge receipt");
    assert_eq!(report.deleted_receipts, 1);
    assert_eq!(report.deleted_idem_keys, 1);
    assert!(
        store
            .get_receipt(&receipt.cid)
            .expect("get receipt")
            .is_none()
    );

    let after_purge = store
        .enqueue_work_idempotent(WorkUnit::new(
            "tenant".to_string(),
            "echo:test".to_string(),
            Some("idem-1".to_string()),
            json!({"x": 1}),
        ))
        .expect("enqueue after purge");
    assert!(matches!(after_purge, EnqueueResult::Enqueued { .. }));

    let _ = std::fs::remove_file(&path);
}
