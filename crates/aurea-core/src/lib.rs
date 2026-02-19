use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod nrf;

pub use nrf::canon::{
    CanonError, canonical_json_string, canonical_json_string_with_profile, to_nrf_bytes,
};
pub use nrf::hash::cid_of;
pub use nrf::types::{CanonProfile, NumNorm};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkStatus {
    Accepted,
    Assigned,
    Progress,
    Done,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkUnit {
    pub id: Uuid,
    pub tenant: String,
    pub topic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idem_key: Option<String>,
    pub payload: serde_json::Value,
    pub submitted_at: DateTime<Utc>,
}

impl WorkUnit {
    pub fn new(
        tenant: String,
        topic: String,
        idem_key: Option<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            tenant,
            topic,
            idem_key,
            payload,
            submitted_at: Utc::now(),
        }
    }

    pub fn plan_hash(&self) -> Result<String, CanonError> {
        cid_for(&self.payload)
    }

    pub fn effective_idem_key(&self) -> Result<String, CanonError> {
        self.idem_key
            .clone()
            .map(Ok)
            .unwrap_or_else(|| self.plan_hash())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyEntry {
    pub rule: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub cid: String,
    pub path: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptSignature {
    pub alg: String,
    pub kid: String,
    pub public_key: String,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsignedReceipt {
    pub work_id: Uuid,
    pub tenant: String,
    pub topic: String,
    pub status: WorkStatus,
    pub idem_key: String,
    pub plan_hash: String,
    pub policy_trace: Vec<PolicyEntry>,
    pub stage_time_ms: BTreeMap<String, u64>,
    pub artifacts: Vec<ArtifactRef>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub cid: String,
    pub work_id: Uuid,
    pub tenant: String,
    pub topic: String,
    pub status: WorkStatus,
    pub idem_key: String,
    pub plan_hash: String,
    pub policy_trace: Vec<PolicyEntry>,
    pub stage_time_ms: BTreeMap<String, u64>,
    pub artifacts: Vec<ArtifactRef>,
    pub created_at: DateTime<Utc>,
    pub signature: ReceiptSignature,
}

impl Receipt {
    pub fn unsigned(&self) -> UnsignedReceipt {
        UnsignedReceipt {
            work_id: self.work_id,
            tenant: self.tenant.clone(),
            topic: self.topic.clone(),
            status: self.status,
            idem_key: self.idem_key.clone(),
            plan_hash: self.plan_hash.clone(),
            policy_trace: self.policy_trace.clone(),
            stage_time_ms: self.stage_time_ms.clone(),
            artifacts: self.artifacts.clone(),
            created_at: self.created_at,
        }
    }

    pub fn computed_cid(&self) -> Result<String, CanonError> {
        cid_for(&self.unsigned())
    }

    pub fn cid_matches(&self) -> Result<bool, CanonError> {
        Ok(self.cid == self.computed_cid()?)
    }
}

pub fn cid_for<T: Serialize>(value: &T) -> Result<String, CanonError> {
    let raw = serde_json::to_value(value)?;
    let bytes = to_nrf_bytes(raw, CanonProfile::default())?;
    Ok(cid_of(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::Value;
    use std::fs;
    use std::path::PathBuf;

    #[derive(Debug, Deserialize)]
    struct OkVector {
        #[serde(rename = "in")]
        input: Value,
        out: String,
    }

    #[derive(Debug, Deserialize)]
    struct ErrVector {
        #[serde(rename = "in")]
        input: Value,
        error: String,
    }

    #[derive(Debug, Deserialize)]
    struct ProfileVector {
        profile: CanonProfile,
        #[serde(rename = "in")]
        input: Value,
        out: String,
    }

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qa/fixtures/canon")
    }

    #[test]
    fn vectors_ok_match_expected_output() {
        let content = fs::read_to_string(fixtures_dir().join("jcs_vectors_ok.json")).unwrap();
        let vectors: Vec<OkVector> = serde_json::from_str(&content).unwrap();

        for v in vectors {
            let got = canonical_json_string(&v.input).unwrap();
            assert_eq!(got, v.out);
        }
    }

    #[test]
    fn vectors_err_fail_with_schema_invalid_shape() {
        let content = fs::read_to_string(fixtures_dir().join("jcs_vectors_err.json")).unwrap();
        let vectors: Vec<ErrVector> = serde_json::from_str(&content).unwrap();

        for v in vectors {
            let got = canonical_json_string(&v.input);
            assert!(got.is_err(), "expected error tag {}", v.error);
        }
    }

    #[test]
    fn profile_null_strip_matches_expected_output() {
        let content = fs::read_to_string(fixtures_dir().join("profile_null_strip.json")).unwrap();
        let vectors: Vec<ProfileVector> = serde_json::from_str(&content).unwrap();

        for v in vectors {
            let got = canonical_json_string_with_profile(&v.input, v.profile).unwrap();
            assert_eq!(got, v.out);
        }
    }

    #[test]
    fn profile_num_norm_matches_expected_output() {
        let content = fs::read_to_string(fixtures_dir().join("profile_num_norm.json")).unwrap();
        let vectors: Vec<ProfileVector> = serde_json::from_str(&content).unwrap();

        for v in vectors {
            let got = canonical_json_string_with_profile(&v.input, v.profile).unwrap();
            assert_eq!(got, v.out);
        }
    }

    #[test]
    fn receipt_cid_round_trip() {
        let unsigned = UnsignedReceipt {
            work_id: Uuid::new_v4(),
            tenant: "t1".to_string(),
            topic: "echo:test".to_string(),
            status: WorkStatus::Done,
            idem_key: "ik".to_string(),
            plan_hash: "ph".to_string(),
            policy_trace: vec![],
            stage_time_ms: BTreeMap::new(),
            artifacts: vec![],
            created_at: Utc::now(),
        };
        let cid = cid_for(&unsigned).unwrap();
        assert_eq!(cid.len(), 52);
    }
}
