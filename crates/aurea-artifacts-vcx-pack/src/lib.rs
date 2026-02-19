use blake3::Hasher;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackEntry {
    pub path: String,
    pub sha: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PackManifest {
    pub entries: Vec<PackEntry>,
}

#[derive(Debug, Error)]
pub enum PackError {
    #[error("failed to serialize pack manifest: {0}")]
    Serialize(#[from] serde_json::Error),
}

pub fn pack_cid(manifest: &PackManifest) -> Result<String, PackError> {
    let json = serde_json::to_string(manifest)?;
    let mut hasher = Hasher::new();
    hasher.update(json.as_bytes());
    Ok(hasher.finalize().to_hex().to_string())
}

pub fn verify(manifest: &PackManifest) -> Result<(), PackError> {
    let _ = pack_cid(manifest)?;
    Ok(())
}
