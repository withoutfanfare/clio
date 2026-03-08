use thiserror::Error;

/// All errors produced by the Clio core library.
#[derive(Debug, Error)]
pub enum ClioError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Migration error: {0}")]
    Migration(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Memory not found: {0}")]
    NotFound(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Export error: {0}")]
    Export(String),

    #[error("Import error: {0}")]
    Import(String),
}

impl From<rusqlite::Error> for ClioError {
    fn from(err: rusqlite::Error) -> Self {
        ClioError::Storage(err.to_string())
    }
}

impl From<serde_json::Error> for ClioError {
    fn from(err: serde_json::Error) -> Self {
        ClioError::Validation(format!("JSON error: {err}"))
    }
}

pub type Result<T> = std::result::Result<T, ClioError>;
