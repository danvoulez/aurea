use serde::{Deserialize, Serialize};

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
