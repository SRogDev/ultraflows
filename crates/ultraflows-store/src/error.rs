use thiserror::Error;

/// Storage errors. The runtime maps these into its own error type; the
/// core never sees them.
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("run not found: {0}")]
    NotFound(uuid::Uuid),
    #[error("invalid uuid in database: {0}")]
    InvalidUuid(String),
}
