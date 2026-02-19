use serde_json::{Map, Number, Value};
use thiserror::Error;

use super::types::{CanonProfile, NumNorm};

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

pub fn to_nrf_bytes(value: Value, profile: CanonProfile) -> Result<Vec<u8>, CanonError> {
    let canon = canonicalize_value(&value, profile)?;
    Ok(serde_json::to_vec(&canon)?)
}

pub fn canonical_json_string(value: &Value) -> Result<String, CanonError> {
    canonical_json_string_with_profile(value, CanonProfile::default())
}

pub fn canonical_json_string_with_profile(
    value: &Value,
    profile: CanonProfile,
) -> Result<String, CanonError> {
    let bytes = to_nrf_bytes(value.clone(), profile)?;
    Ok(String::from_utf8(bytes).expect("serde_json always emits UTF-8"))
}

fn canonicalize_value(value: &Value, profile: CanonProfile) -> Result<Value, CanonError> {
    match value {
        Value::Object(obj) => {
            let mut pairs: Vec<_> = obj.iter().collect();
            pairs.sort_by(|(a, _), (b, _)| a.cmp(b));

            let mut out = Map::new();
            for (key, val) in pairs {
                if profile.null_strip && val.is_null() {
                    continue;
                }
                out.insert(key.clone(), canonicalize_value(val, profile)?);
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
        Value::Number(n) => Ok(Value::Number(normalize_number(n, profile.num_norm)?)),
        _ => Ok(value.clone()),
    }
}

fn normalize_number(num: &Number, mode: NumNorm) -> Result<Number, CanonError> {
    match mode {
        NumNorm::Strict => {
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
    }
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
