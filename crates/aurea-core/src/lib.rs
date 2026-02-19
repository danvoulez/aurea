use std::collections::BTreeMap;

use blake3::Hasher;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use thiserror::Error;
use uuid::Uuid;

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
    pub payload: Value,
    pub submitted_at: DateTime<Utc>,
}

impl WorkUnit {
    pub fn new(tenant: String, topic: String, idem_key: Option<String>, payload: Value) -> Self {
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum NumNorm {
    Strict,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
pub struct CanonProfile {
    pub null_strip: bool,
    pub num_norm: NumNorm,
}

impl Default for CanonProfile {
    fn default() -> Self {
        Self {
            null_strip: false,
            num_norm: NumNorm::Strict,
        }
    }
}

#[derive(Debug, Error)]
pub enum CanonError {
    #[error("value contains disallowed string marker: {0}")]
    DisallowedString(String),
    #[error("string contains disallowed lone surrogate escape: {0}")]
    LoneSurrogateEscape(String),
    #[error("found non-finite number")]
    NonFiniteNumber,
    #[error("failed to serialize canonical JSON: {0}")]
    Serialize(#[from] serde_json::Error),
}

pub fn canonical_json_string(value: &Value) -> Result<String, CanonError> {
    canonical_json_string_with_profile(value, CanonProfile::default())
}

pub fn canonical_json_string_with_profile(
    value: &Value,
    profile: CanonProfile,
) -> Result<String, CanonError> {
    let canon = canonicalize_value(value, profile)?;
    Ok(serde_json::to_string(&canon)?)
}

pub fn cid_for<T: Serialize>(value: &T) -> Result<String, CanonError> {
    let raw = serde_json::to_value(value)?;
    let canon = canonical_json_string(&raw)?;
    let mut hasher = Hasher::new();
    hasher.update(canon.as_bytes());
    Ok(hasher.finalize().to_hex().to_string())
}

pub fn canonicalize_value(value: &Value, profile: CanonProfile) -> Result<Value, CanonError> {
    match value {
        Value::Object(obj) => {
            let mut pairs: Vec<_> = obj.iter().collect();
            pairs.sort_by(|(a, _), (b, _)| a.cmp(b));

            let mut out = Map::new();
            for (key, val) in pairs {
                if profile.null_strip && val.is_null() {
                    continue;
                }
                let canon = canonicalize_value(val, profile)?;
                out.insert(key.clone(), canon);
            }
            Ok(Value::Object(out))
        }
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(canonicalize_value(item, profile)?);
            }
            Ok(Value::Array(out))
        }
        Value::String(s) => {
            let normalized = normalize_string_escapes(s)?;
            validate_string(&normalized)?;
            Ok(Value::String(normalized))
        }
        Value::Number(n) => Ok(Value::Number(normalize_number(n)?)),
        _ => Ok(value.clone()),
    }
}

fn normalize_number(num: &Number) -> Result<Number, CanonError> {
    if let Some(v) = num.as_i64() {
        return Ok(Number::from(v));
    }
    if let Some(v) = num.as_u64() {
        return Ok(Number::from(v));
    }

    let float = num.as_f64().ok_or(CanonError::NonFiniteNumber)?;
    if !float.is_finite() {
        return Err(CanonError::NonFiniteNumber);
    }

    if float == 0.0 {
        return Ok(Number::from(0));
    }

    if float.fract() == 0.0 {
        let integer = float as i128;
        if integer >= i64::MIN as i128 && integer <= i64::MAX as i128 {
            return Ok(Number::from(integer as i64));
        }
    }

    Number::from_f64(float).ok_or(CanonError::NonFiniteNumber)
}

fn validate_string(s: &str) -> Result<(), CanonError> {
    if matches!(s, "NaN" | "Infinity" | "-Infinity") {
        return Err(CanonError::DisallowedString(s.to_string()));
    }
    Ok(())
}

fn normalize_string_escapes(s: &str) -> Result<String, CanonError> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;

    while i < chars.len() {
        if chars[i] != '\\' {
            out.push(chars[i]);
            i += 1;
            continue;
        }

        i += 1;
        if i >= chars.len() {
            out.push('\\');
            break;
        }

        match chars[i] {
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            '/' => out.push('/'),
            'b' => out.push('\u{0008}'),
            'f' => out.push('\u{000C}'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'u' => {
                let cp = parse_u_escape(&chars, i + 1)?;
                i += 4;

                if (0xD800..=0xDBFF).contains(&cp) {
                    if i + 2 >= chars.len() || chars[i + 1] != '\\' || chars[i + 2] != 'u' {
                        return Err(CanonError::LoneSurrogateEscape(s.to_string()));
                    }
                    let cp2 = parse_u_escape(&chars, i + 3)?;
                    if !(0xDC00..=0xDFFF).contains(&cp2) {
                        return Err(CanonError::LoneSurrogateEscape(s.to_string()));
                    }
                    i += 6;

                    let high = (cp as u32) - 0xD800;
                    let low = (cp2 as u32) - 0xDC00;
                    let scalar = 0x10000 + ((high << 10) | low);
                    if let Some(ch) = char::from_u32(scalar) {
                        out.push(ch);
                    } else {
                        return Err(CanonError::LoneSurrogateEscape(s.to_string()));
                    }
                    continue;
                }

                if (0xDC00..=0xDFFF).contains(&cp) {
                    return Err(CanonError::LoneSurrogateEscape(s.to_string()));
                }

                if let Some(ch) = char::from_u32(cp as u32) {
                    out.push(ch);
                } else {
                    return Err(CanonError::LoneSurrogateEscape(s.to_string()));
                }
            }
            other => {
                out.push('\\');
                out.push(other);
            }
        }
        i += 1;
    }

    Ok(out)
}

fn parse_u_escape(chars: &[char], start: usize) -> Result<u16, CanonError> {
    if start + 4 > chars.len() {
        return Err(CanonError::LoneSurrogateEscape(
            chars.iter().collect::<String>(),
        ));
    }

    let mut out = 0u16;
    for ch in &chars[start..start + 4] {
        out <<= 4;
        out |= ch
            .to_digit(16)
            .ok_or_else(|| CanonError::LoneSurrogateEscape(chars.iter().collect::<String>()))?
            as u16;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
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
        assert_eq!(cid.len(), 64);
    }
}
