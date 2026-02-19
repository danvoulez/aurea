use std::fs;
use std::path::Path;

use aurea_core::cid_of;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const MAGIC: &[u8; 8] = b"VCXPACK1";
const HEADER_LEN: usize = 8 + (8 * 3);
const TRAILER_LEN_BYTES: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackInput {
    pub path: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackEntry {
    pub path: String,
    pub offset: u64,
    pub size: u64,
    pub sha: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackManifest {
    pub version: String,
    pub created_at: String,
    pub entry_count: usize,
    pub entries: Vec<PackEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackIndex {
    pub entries: Vec<PackEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackTrailer {
    pub manifest_offset: u64,
    pub manifest_len: u64,
    pub index_offset: u64,
    pub index_len: u64,
    pub data_offset: u64,
    pub data_len: u64,
    pub manifest_sha: String,
    pub index_sha: String,
    pub data_sha: String,
    pub pack_cid: String,
}

#[derive(Debug, Clone)]
pub struct PackBundle {
    pub manifest: PackManifest,
    pub index: PackIndex,
    pub trailer: PackTrailer,
    pub data: Vec<u8>,
    pub preamble: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackWriteResult {
    pub pack_cid: String,
    pub manifest: PackManifest,
    pub index: PackIndex,
    pub trailer: PackTrailer,
    pub bytes_written: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackVerifyResult {
    pub ok: bool,
    pub pack_cid: Option<String>,
    pub entries: usize,
    pub reason: Option<String>,
}

#[derive(Debug, Error)]
pub enum PackError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to serialize pack json: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("invalid pack format: {0}")]
    InvalidFormat(String),
}

pub fn write_pack(path: &Path, inputs: &[PackInput]) -> Result<PackWriteResult, PackError> {
    let mut entries = Vec::with_capacity(inputs.len());
    let mut data = Vec::new();
    let mut offset = 0u64;

    for input in inputs {
        let size = input.bytes.len() as u64;
        let sha = blake3_hex(&input.bytes);
        entries.push(PackEntry {
            path: input.path.clone(),
            offset,
            size,
            sha,
        });

        data.extend_from_slice(&input.bytes);
        offset = offset.saturating_add(size);
    }

    let manifest = PackManifest {
        version: "vcx-pack.v1".to_string(),
        created_at: Utc::now().to_rfc3339(),
        entry_count: entries.len(),
        entries: entries.clone(),
    };
    let index = PackIndex { entries };

    let manifest_bytes = serde_json::to_vec(&manifest)?;
    let index_bytes = serde_json::to_vec(&index)?;

    let manifest_offset = HEADER_LEN as u64;
    let manifest_len = manifest_bytes.len() as u64;
    let index_offset = manifest_offset + manifest_len;
    let index_len = index_bytes.len() as u64;
    let data_offset = index_offset + index_len;
    let data_len = data.len() as u64;

    let mut preamble =
        Vec::with_capacity(HEADER_LEN + manifest_bytes.len() + index_bytes.len() + data.len());
    preamble.extend_from_slice(MAGIC);
    preamble.extend_from_slice(&manifest_len.to_le_bytes());
    preamble.extend_from_slice(&index_len.to_le_bytes());
    preamble.extend_from_slice(&data_len.to_le_bytes());
    preamble.extend_from_slice(&manifest_bytes);
    preamble.extend_from_slice(&index_bytes);
    preamble.extend_from_slice(&data);

    let pack_cid = cid_of(&preamble);
    let trailer = PackTrailer {
        manifest_offset,
        manifest_len,
        index_offset,
        index_len,
        data_offset,
        data_len,
        manifest_sha: blake3_hex(&manifest_bytes),
        index_sha: blake3_hex(&index_bytes),
        data_sha: blake3_hex(&data),
        pack_cid: pack_cid.clone(),
    };

    let trailer_bytes = serde_json::to_vec(&trailer)?;
    let trailer_len = trailer_bytes.len() as u64;

    let mut out = preamble;
    out.extend_from_slice(&trailer_len.to_le_bytes());
    out.extend_from_slice(&trailer_bytes);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, &out)?;

    Ok(PackWriteResult {
        pack_cid,
        manifest,
        index,
        trailer,
        bytes_written: out.len() as u64,
    })
}

pub fn read_pack(path: &Path) -> Result<PackBundle, PackError> {
    let bytes = fs::read(path)?;
    if bytes.len() < HEADER_LEN + TRAILER_LEN_BYTES {
        return Err(PackError::InvalidFormat("pack too small".to_string()));
    }

    if &bytes[..8] != MAGIC {
        return Err(PackError::InvalidFormat("magic mismatch".to_string()));
    }

    let manifest_len = u64_from(&bytes[8..16]) as usize;
    let index_len = u64_from(&bytes[16..24]) as usize;
    let data_len = u64_from(&bytes[24..32]) as usize;

    let manifest_start = HEADER_LEN;
    let manifest_end = manifest_start + manifest_len;
    let index_start = manifest_end;
    let index_end = index_start + index_len;
    let data_start = index_end;
    let data_end = data_start + data_len;

    if data_end + TRAILER_LEN_BYTES > bytes.len() {
        return Err(PackError::InvalidFormat(
            "declared section sizes exceed file length".to_string(),
        ));
    }

    let trailer_len_start = data_end;
    let trailer_len = u64_from(&bytes[trailer_len_start..trailer_len_start + 8]) as usize;
    let trailer_start = trailer_len_start + 8;
    let trailer_end = trailer_start + trailer_len;
    if trailer_end != bytes.len() {
        return Err(PackError::InvalidFormat(
            "trailer length does not match file size".to_string(),
        ));
    }

    let manifest_bytes = &bytes[manifest_start..manifest_end];
    let index_bytes = &bytes[index_start..index_end];
    let data = bytes[data_start..data_end].to_vec();
    let trailer_bytes = &bytes[trailer_start..trailer_end];

    let manifest: PackManifest = serde_json::from_slice(manifest_bytes)?;
    let index: PackIndex = serde_json::from_slice(index_bytes)?;
    let trailer: PackTrailer = serde_json::from_slice(trailer_bytes)?;

    let preamble = bytes[..data_end].to_vec();

    Ok(PackBundle {
        manifest,
        index,
        trailer,
        data,
        preamble,
    })
}

pub fn verify(path: &Path) -> Result<PackVerifyResult, PackError> {
    let bundle = match read_pack(path) {
        Ok(v) => v,
        Err(err) => {
            return Ok(PackVerifyResult {
                ok: false,
                pack_cid: None,
                entries: 0,
                reason: Some(err.to_string()),
            });
        }
    };

    let verify = verify_bundle(&bundle);
    Ok(verify)
}

pub fn pack_cid(path: &Path) -> Result<String, PackError> {
    let bundle = read_pack(path)?;
    Ok(cid_of(&bundle.preamble))
}

fn verify_bundle(bundle: &PackBundle) -> PackVerifyResult {
    if bundle.manifest.entry_count != bundle.index.entries.len() {
        return invalid(
            bundle,
            "manifest entry_count does not match index entries length",
        );
    }

    if bundle.manifest.entries != bundle.index.entries {
        return invalid(bundle, "manifest entries differ from index entries");
    }

    let expected_manifest_offset = HEADER_LEN as u64;
    let expected_manifest_len = serde_json::to_vec(&bundle.manifest)
        .ok()
        .map(|v| v.len() as u64)
        .unwrap_or(0);
    let expected_index_offset = expected_manifest_offset + expected_manifest_len;
    let expected_index_len = serde_json::to_vec(&bundle.index)
        .ok()
        .map(|v| v.len() as u64)
        .unwrap_or(0);
    let expected_data_offset = expected_index_offset + expected_index_len;
    let expected_data_len = bundle.data.len() as u64;

    if bundle.trailer.manifest_offset != expected_manifest_offset
        || bundle.trailer.manifest_len != expected_manifest_len
        || bundle.trailer.index_offset != expected_index_offset
        || bundle.trailer.index_len != expected_index_len
        || bundle.trailer.data_offset != expected_data_offset
        || bundle.trailer.data_len != expected_data_len
    {
        return invalid(bundle, "trailer offsets/lengths mismatch");
    }

    let manifest_bytes = match serde_json::to_vec(&bundle.manifest) {
        Ok(v) => v,
        Err(err) => return invalid(bundle, &format!("manifest serialization failed: {err}")),
    };
    let index_bytes = match serde_json::to_vec(&bundle.index) {
        Ok(v) => v,
        Err(err) => return invalid(bundle, &format!("index serialization failed: {err}")),
    };

    if bundle.trailer.manifest_sha != blake3_hex(&manifest_bytes) {
        return invalid(bundle, "manifest hash mismatch");
    }
    if bundle.trailer.index_sha != blake3_hex(&index_bytes) {
        return invalid(bundle, "index hash mismatch");
    }
    if bundle.trailer.data_sha != blake3_hex(&bundle.data) {
        return invalid(bundle, "data hash mismatch");
    }

    let recomputed_pack_cid = cid_of(&bundle.preamble);
    if bundle.trailer.pack_cid != recomputed_pack_cid {
        return invalid(bundle, "pack cid mismatch");
    }

    for entry in &bundle.index.entries {
        let start = entry.offset as usize;
        let end = start.saturating_add(entry.size as usize);
        if end > bundle.data.len() {
            return invalid(bundle, "entry offset+size exceed data section");
        }

        let slice = &bundle.data[start..end];
        if blake3_hex(slice) != entry.sha {
            return invalid(bundle, &format!("entry hash mismatch: {}", entry.path));
        }
    }

    PackVerifyResult {
        ok: true,
        pack_cid: Some(bundle.trailer.pack_cid.clone()),
        entries: bundle.index.entries.len(),
        reason: None,
    }
}

fn invalid(bundle: &PackBundle, reason: &str) -> PackVerifyResult {
    PackVerifyResult {
        ok: false,
        pack_cid: Some(bundle.trailer.pack_cid.clone()),
        entries: bundle.index.entries.len(),
        reason: Some(reason.to_string()),
    }
}

fn blake3_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn u64_from(bytes: &[u8]) -> u64 {
    let mut out = [0u8; 8];
    out.copy_from_slice(bytes);
    u64::from_le_bytes(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_read_verify_round_trip() {
        let dir = std::env::temp_dir().join(format!("vcx-pack-{}", uuid::Uuid::new_v4()));
        let path = dir.join("sample.vcxpack");

        let out = write_pack(
            &path,
            &[
                PackInput {
                    path: "a.txt".to_string(),
                    bytes: b"hello".to_vec(),
                },
                PackInput {
                    path: "b.json".to_string(),
                    bytes: br#"{"x":1}"#.to_vec(),
                },
            ],
        )
        .expect("write pack");

        let read = read_pack(&path).expect("read pack");
        assert_eq!(read.index.entries.len(), 2);

        let verified = verify(&path).expect("verify pack");
        assert!(verified.ok);
        assert_eq!(verified.pack_cid.as_deref(), Some(out.pack_cid.as_str()));

        let _ = fs::remove_file(path);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn verify_detects_corrupted_data() {
        let dir = std::env::temp_dir().join(format!("vcx-pack-{}", uuid::Uuid::new_v4()));
        let path = dir.join("sample.vcxpack");

        write_pack(
            &path,
            &[PackInput {
                path: "a.txt".to_string(),
                bytes: b"hello".to_vec(),
            }],
        )
        .expect("write pack");

        let bundle = read_pack(&path).expect("read pack");
        let mut bytes = fs::read(&path).expect("read bytes");
        // Corrupt one byte in the data section while preserving file layout.
        let data_start = bundle.trailer.data_offset as usize;
        if data_start < bytes.len() {
            bytes[data_start] ^= 0x55;
        }
        fs::write(&path, bytes).expect("write corrupt bytes");

        let verified = verify(&path).expect("verify pack");
        assert!(!verified.ok);
        assert!(
            verified
                .reason
                .unwrap_or_default()
                .contains("hash mismatch")
        );

        let _ = fs::remove_file(path);
        let _ = fs::remove_dir_all(dir);
    }
}
