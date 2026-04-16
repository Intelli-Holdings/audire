use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParaError {
    #[error("invalid state: {0}")]
    InvalidState(String),

    #[error("missing BYOK key for provider: {0}")]
    MissingKey(String),

    #[error("audio error: {0}")]
    Audio(String),

    #[error("asr error: {0}")]
    Asr(String),

    #[error("db error: {0}")]
    Db(String),

    #[error("keyvault error: {0}")]
    KeyVault(String),

    #[error("llm disabled at build time (feature flag not enabled)")]
    LlmDisabled,

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, ParaError>;

// Serialize as a string for Tauri IPC error responses.
// Tauri v2 requires command error types to implement Serialize.
impl serde::Serialize for ParaError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl From<anyhow::Error> for ParaError {
    fn from(e: anyhow::Error) -> Self {
        ParaError::Other(e.to_string())
    }
}
