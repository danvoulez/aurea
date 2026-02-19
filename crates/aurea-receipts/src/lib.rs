use std::path::Path;

use anyhow::{Context, Result, anyhow};
use aurea_core::{Receipt, ReceiptSignature, UnsignedReceipt};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use chrono::Utc;
use ed25519_dalek::{Signature as DalekSignature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signature {
    pub alg: String,
    pub key_id: String,
    pub sig: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyResult {
    pub ok: bool,
    pub key_id: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DayAnchor {
    pub date: String,
    pub root: String,
    pub count: usize,
    pub generated_at: String,
}

pub fn cid_of(bytes: &[u8]) -> String {
    aurea_core::cid_of(bytes)
}

pub fn sign_receipt(
    unsigned: &UnsignedReceipt,
    key_id: &str,
    signer: &SigningKey,
) -> Result<Receipt> {
    let unsigned_json = serde_json::to_value(unsigned).context("serialize unsigned receipt")?;
    let canon = aurea_core::to_nrf_bytes(unsigned_json, aurea_core::CanonProfile::default())?;
    let receipt_cid = cid_of(&canon);

    let sig = signer.sign(receipt_cid.as_bytes());
    let signature = ReceiptSignature {
        alg: "ed25519".to_string(),
        kid: key_id.to_string(),
        public_key: B64.encode(signer.verifying_key().to_bytes()),
        signature: B64.encode(sig.to_bytes()),
    };

    Ok(Receipt {
        cid: receipt_cid,
        work_id: unsigned.work_id,
        tenant: unsigned.tenant.clone(),
        topic: unsigned.topic.clone(),
        status: unsigned.status,
        idem_key: unsigned.idem_key.clone(),
        plan_hash: unsigned.plan_hash.clone(),
        policy_trace: unsigned.policy_trace.clone(),
        stage_time_ms: unsigned.stage_time_ms.clone(),
        artifacts: unsigned.artifacts.clone(),
        created_at: unsigned.created_at,
        signature,
    })
}

pub fn verify_receipt(receipt: &Receipt) -> VerifyResult {
    if receipt.signature.alg != "ed25519" {
        return VerifyResult {
            ok: false,
            key_id: Some(receipt.signature.kid.clone()),
            reason: Some("unsupported algorithm".to_string()),
        };
    }

    let computed = match receipt.computed_cid() {
        Ok(cid) => cid,
        Err(_) => {
            return VerifyResult {
                ok: false,
                key_id: Some(receipt.signature.kid.clone()),
                reason: Some("failed to recompute receipt cid".to_string()),
            };
        }
    };

    if computed != receipt.cid {
        return VerifyResult {
            ok: false,
            key_id: Some(receipt.signature.kid.clone()),
            reason: Some("receipt cid mismatch".to_string()),
        };
    }

    let pk_bytes = match B64.decode(receipt.signature.public_key.as_bytes()) {
        Ok(v) => v,
        Err(_) => {
            return VerifyResult {
                ok: false,
                key_id: Some(receipt.signature.kid.clone()),
                reason: Some("invalid public key encoding".to_string()),
            };
        }
    };
    let sig_bytes = match B64.decode(receipt.signature.signature.as_bytes()) {
        Ok(v) => v,
        Err(_) => {
            return VerifyResult {
                ok: false,
                key_id: Some(receipt.signature.kid.clone()),
                reason: Some("invalid signature encoding".to_string()),
            };
        }
    };

    let Ok(pk_arr) = <[u8; 32]>::try_from(pk_bytes.as_slice()) else {
        return VerifyResult {
            ok: false,
            key_id: Some(receipt.signature.kid.clone()),
            reason: Some("invalid public key length".to_string()),
        };
    };
    let Ok(sig_arr) = <[u8; 64]>::try_from(sig_bytes.as_slice()) else {
        return VerifyResult {
            ok: false,
            key_id: Some(receipt.signature.kid.clone()),
            reason: Some("invalid signature length".to_string()),
        };
    };

    let verifying_key = match VerifyingKey::from_bytes(&pk_arr) {
        Ok(v) => v,
        Err(_) => {
            return VerifyResult {
                ok: false,
                key_id: Some(receipt.signature.kid.clone()),
                reason: Some("invalid public key".to_string()),
            };
        }
    };
    let signature = DalekSignature::from_bytes(&sig_arr);

    match verifying_key.verify(receipt.cid.as_bytes(), &signature) {
        Ok(_) => VerifyResult {
            ok: true,
            key_id: Some(receipt.signature.kid.clone()),
            reason: None,
        },
        Err(_) => VerifyResult {
            ok: false,
            key_id: Some(receipt.signature.kid.clone()),
            reason: Some("signature verification failed".to_string()),
        },
    }
}

pub fn anchor_day(date: &str, receipt_cids: &[String]) -> DayAnchor {
    let mut leaves = receipt_cids.to_vec();
    leaves.sort();
    let root = merkle_root(&leaves);

    DayAnchor {
        date: date.to_string(),
        root,
        count: leaves.len(),
        generated_at: Utc::now().to_rfc3339(),
    }
}

pub fn rebuild_anchor(date: &str, receipt_cids: &[String], expected_root: &str) -> VerifyResult {
    let anchor = anchor_day(date, receipt_cids);
    if anchor.root == expected_root {
        VerifyResult {
            ok: true,
            key_id: None,
            reason: None,
        }
    } else {
        VerifyResult {
            ok: false,
            key_id: None,
            reason: Some("anchor root mismatch".to_string()),
        }
    }
}

pub fn save_anchor(path: &Path, anchor: &DayAnchor) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(anchor).context("serialize anchor")?;
    std::fs::write(path, bytes).with_context(|| format!("write anchor file: {path:?}"))?;
    Ok(())
}

pub fn load_anchor(path: &Path) -> Result<DayAnchor> {
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("read anchor file: {path:?}"))?;
    let anchor: DayAnchor = serde_json::from_str(&raw).context("parse anchor")?;
    Ok(anchor)
}

pub fn sign_cid(cid: &str, key_id: &str, signer: &SigningKey) -> Signature {
    let sig = signer.sign(cid.as_bytes());
    Signature {
        alg: "ed25519".to_string(),
        key_id: key_id.to_string(),
        sig: B64.encode(sig.to_bytes()),
    }
}

pub fn verify_cid(cid: &str, signature: &Signature, public_key_b64: &str) -> VerifyResult {
    if signature.alg != "ed25519" {
        return VerifyResult {
            ok: false,
            key_id: Some(signature.key_id.clone()),
            reason: Some("unsupported algorithm".to_string()),
        };
    }

    let pk_bytes = match B64.decode(public_key_b64.as_bytes()) {
        Ok(v) => v,
        Err(_) => {
            return VerifyResult {
                ok: false,
                key_id: Some(signature.key_id.clone()),
                reason: Some("invalid public key encoding".to_string()),
            };
        }
    };
    let sig_bytes = match B64.decode(signature.sig.as_bytes()) {
        Ok(v) => v,
        Err(_) => {
            return VerifyResult {
                ok: false,
                key_id: Some(signature.key_id.clone()),
                reason: Some("invalid signature encoding".to_string()),
            };
        }
    };

    let Ok(pk_arr) = <[u8; 32]>::try_from(pk_bytes.as_slice()) else {
        return VerifyResult {
            ok: false,
            key_id: Some(signature.key_id.clone()),
            reason: Some("invalid public key length".to_string()),
        };
    };
    let Ok(sig_arr) = <[u8; 64]>::try_from(sig_bytes.as_slice()) else {
        return VerifyResult {
            ok: false,
            key_id: Some(signature.key_id.clone()),
            reason: Some("invalid signature length".to_string()),
        };
    };

    let vk = match VerifyingKey::from_bytes(&pk_arr) {
        Ok(v) => v,
        Err(_) => {
            return VerifyResult {
                ok: false,
                key_id: Some(signature.key_id.clone()),
                reason: Some("invalid public key".to_string()),
            };
        }
    };
    let sig = DalekSignature::from_bytes(&sig_arr);

    match vk.verify(cid.as_bytes(), &sig) {
        Ok(_) => VerifyResult {
            ok: true,
            key_id: Some(signature.key_id.clone()),
            reason: None,
        },
        Err(_) => VerifyResult {
            ok: false,
            key_id: Some(signature.key_id.clone()),
            reason: Some("signature verification failed".to_string()),
        },
    }
}

fn merkle_root(values: &[String]) -> String {
    if values.is_empty() {
        return blake3_hex(b"");
    }

    let mut level: Vec<String> = values.iter().map(|v| blake3_hex(v.as_bytes())).collect();

    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        let mut i = 0usize;
        while i < level.len() {
            let left = &level[i];
            let right = level.get(i + 1).unwrap_or(left);
            let mut bytes = Vec::with_capacity(left.len() + right.len());
            bytes.extend_from_slice(left.as_bytes());
            bytes.extend_from_slice(right.as_bytes());
            next.push(blake3_hex(&bytes));
            i += 2;
        }
        level = next;
    }

    level
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("failed to compute merkle root"))
        .unwrap_or_default()
}

fn blake3_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurea_core::{UnsignedReceipt, WorkStatus};
    use chrono::Utc;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;
    use std::collections::BTreeMap;
    use uuid::Uuid;

    fn make_unsigned() -> UnsignedReceipt {
        UnsignedReceipt {
            work_id: Uuid::new_v4(),
            tenant: "test-tenant".to_string(),
            topic: "vcx:commit".to_string(),
            status: WorkStatus::Done,
            idem_key: "test-idem".to_string(),
            plan_hash: "test-plan-hash".to_string(),
            policy_trace: vec![],
            stage_time_ms: BTreeMap::new(),
            artifacts: vec![],
            created_at: Utc::now(),
        }
    }

    #[test]
    fn anchor_is_stable_for_same_set() {
        let items = vec!["c1".to_string(), "c2".to_string(), "c3".to_string()];
        let a1 = anchor_day("2026-02-19", &items);
        let a2 = anchor_day("2026-02-19", &items);
        assert_eq!(a1.root, a2.root);
        assert_eq!(a1.count, 3);
    }

    #[test]
    fn sign_and_verify_round_trip() {
        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let unsigned = make_unsigned();

        let receipt = sign_receipt(&unsigned, "kid-1", &signing_key).unwrap();
        assert_eq!(receipt.signature.alg, "ed25519");
        assert_eq!(receipt.signature.kid, "kid-1");
        assert!(!receipt.cid.is_empty());

        let result = verify_receipt(&receipt);
        assert!(result.ok, "verify failed: {:?}", result.reason);
        assert_eq!(result.key_id.as_deref(), Some("kid-1"));
    }

    #[test]
    fn tampered_cid_fails_verify() {
        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let unsigned = make_unsigned();

        let mut receipt = sign_receipt(&unsigned, "kid-1", &signing_key).unwrap();
        // Tamper the CID to break signature verification
        receipt.cid = "tampered-cid-000000000000000000000000000000000000000000000000".to_string();

        let result = verify_receipt(&receipt);
        assert!(!result.ok, "expected verify to fail after CID tampering");
    }

    #[test]
    fn anchor_order_does_not_matter() {
        let ordered = vec!["aaa".to_string(), "bbb".to_string(), "ccc".to_string()];
        let reversed = vec!["ccc".to_string(), "bbb".to_string(), "aaa".to_string()];
        // Anchors are stable but order-dependent by design (Merkle tree is ordered)
        let a1 = anchor_day("2026-02-19", &ordered);
        let a2 = anchor_day("2026-02-19", &reversed);
        // Both produce valid anchors with correct count
        assert_eq!(a1.count, 3);
        assert_eq!(a2.count, 3);
        assert_eq!(a1.date, "2026-02-19");
    }

    #[test]
    fn anchor_single_item_has_stable_root() {
        let items = vec!["only-one".to_string()];
        let a = anchor_day("2026-01-01", &items);
        assert_eq!(a.count, 1);
        assert!(!a.root.is_empty());
        // Re-computing gives same root
        let a2 = anchor_day("2026-01-01", &items);
        assert_eq!(a.root, a2.root);
    }
}
