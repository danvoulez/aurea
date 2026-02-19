use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use aurea_artifacts_vcx_pack::{PackInput, verify, write_pack};
use aurea_core::cid_for;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use serde_json::{Value, json};

#[async_trait]
pub trait Plugin: Send + Sync {
    fn name(&self) -> &'static str;
    async fn execute(&self, payload: Value) -> Result<Value>;
}

#[derive(Default, Clone)]
pub struct PluginRegistry {
    plugins: HashMap<String, Arc<dyn Plugin>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<P>(&mut self, plugin: P)
    where
        P: Plugin + 'static,
    {
        self.plugins
            .insert(plugin.name().to_string(), Arc::new(plugin));
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Plugin>> {
        self.plugins.get(name).cloned()
    }

    pub fn names(&self) -> Vec<String> {
        let mut out: Vec<_> = self.plugins.keys().cloned().collect();
        out.sort();
        out
    }
}

pub struct EchoPlugin;

#[async_trait]
impl Plugin for EchoPlugin {
    fn name(&self) -> &'static str {
        "echo"
    }

    async fn execute(&self, payload: Value) -> Result<Value> {
        Ok(payload)
    }
}

pub struct VcxWorkerPlugin;

#[async_trait]
impl Plugin for VcxWorkerPlugin {
    fn name(&self) -> &'static str {
        "vcx"
    }

    async fn execute(&self, payload: Value) -> Result<Value> {
        let inputs = extract_pack_inputs(&payload)?;
        let pack_dir = payload
            .get("pack_dir")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("./packs"));

        std::fs::create_dir_all(&pack_dir)
            .with_context(|| format!("create pack directory: {pack_dir:?}"))?;

        let payload_hash = cid_for(&payload).context("compute payload hash")?;
        let pack_path = pack_dir.join(format!("{payload_hash}.vcxpack"));

        let written = write_pack(&pack_path, &inputs).context("write vcx pack")?;
        let verified = verify(&pack_path).context("verify vcx pack")?;
        if !verified.ok {
            return Err(anyhow!(
                "vcx pack verification failed: {}",
                verified
                    .reason
                    .unwrap_or_else(|| "unknown reason".to_string())
            ));
        }

        Ok(json!({
            "pack": {
                "cid": written.pack_cid,
                "path": pack_path.display().to_string(),
                "entries": written.index.entries.len(),
                "bytes_written": written.bytes_written,
            },
            "artifacts": [
                {
                    "cid": verified.pack_cid,
                    "path": pack_path.display().to_string(),
                    "size_bytes": written.bytes_written,
                }
            ]
        }))
    }
}

fn extract_pack_inputs(payload: &Value) -> Result<Vec<PackInput>> {
    let Some(items) = payload.get("items") else {
        let bytes = serde_json::to_vec_pretty(payload).context("serialize payload fallback")?;
        return Ok(vec![PackInput {
            path: "payload.json".to_string(),
            bytes,
        }]);
    };

    let Value::Array(items) = items else {
        return Err(anyhow!("payload.items must be an array"));
    };

    if items.is_empty() {
        return Err(anyhow!("payload.items cannot be empty"));
    }

    let mut out = Vec::with_capacity(items.len());
    for (idx, item) in items.iter().enumerate() {
        let Value::Object(map) = item else {
            return Err(anyhow!("payload.items[{idx}] must be an object"));
        };

        let path = map
            .get("path")
            .and_then(Value::as_str)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("entry-{idx}.bin"));

        let bytes = if let Some(content) = map.get("content").and_then(Value::as_str) {
            content.as_bytes().to_vec()
        } else if let Some(bytes_b64) = map.get("bytes_b64").and_then(Value::as_str) {
            B64.decode(bytes_b64.as_bytes())
                .with_context(|| format!("invalid base64 for payload.items[{idx}].bytes_b64"))?
        } else {
            serde_json::to_vec(item)
                .with_context(|| format!("serialize fallback payload.items[{idx}]"))?
        };

        out.push(PackInput { path, bytes });
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_pack_inputs_falls_back_to_payload_json() {
        let inputs = extract_pack_inputs(&json!({"x":1})).expect("extract inputs");
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].path, "payload.json");
    }

    #[tokio::test]
    async fn vcx_plugin_emits_artifact() {
        let plugin = VcxWorkerPlugin;
        let pack_dir =
            std::env::temp_dir().join(format!("aurea-vcx-plugin-{}", uuid::Uuid::new_v4()));
        let payload = json!({
            "pack_dir": pack_dir.display().to_string(),
            "items": [
                {"path":"a.txt","content":"hello"}
            ]
        });

        let out = plugin.execute(payload).await.expect("plugin execute");
        let artifacts = out
            .get("artifacts")
            .and_then(Value::as_array)
            .expect("artifacts array");
        assert_eq!(artifacts.len(), 1);

        let _ = std::fs::remove_dir_all(pack_dir);
    }
}
