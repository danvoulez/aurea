use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

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
