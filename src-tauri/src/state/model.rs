use serde::{Deserialize, Serialize};

// Shared model identifiers for saved settings and live session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelId {
    #[serde(rename = "qwen38-27b")]
    Qwen38_27b,
    #[serde(rename = "glm53-flash")]
    Glm53Flash,
}

impl Default for ModelId {
    fn default() -> Self {
        Self::Qwen38_27b
    }
}

impl ModelId {
    pub fn cli_id(self) -> &'static str {
        match self {
            Self::Qwen38_27b => "qwen38-27b",
            Self::Glm53Flash => "glm53-flash",
        }
    }

    pub fn tray_name(self) -> &'static str {
        match self {
            Self::Qwen38_27b => "Qwen3.8 TPU",
            Self::Glm53Flash => "GLM-5.3 TPU",
        }
    }

    pub fn served_name(self) -> &'static str {
        match self {
            Self::Qwen38_27b => "qwen3.8-27b",
            Self::Glm53Flash => "glm-5.3-flash",
        }
    }

    pub fn parse_cli_id(value: &str) -> Option<Self> {
        match value {
            "qwen38-27b" => Some(Self::Qwen38_27b),
            "glm53-flash" => Some(Self::Glm53Flash),
            _ => None,
        }
    }
}
