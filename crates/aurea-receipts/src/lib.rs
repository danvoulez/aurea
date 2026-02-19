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

    #[test]
    fn anchor_is_stable_for_same_set() {
        let items = vec!["c1".to_string(), "c2".to_string(), "c3".to_string()];
        let a1 = anchor_day("2026-02-19", &items);
        let a2 = anchor_day("2026-02-19", &items);
        assert_eq!(a1.root, a2.root);
        assert_eq!(a1.count, 3);
    }
}
