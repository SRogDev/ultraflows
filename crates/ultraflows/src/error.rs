use thiserror::Error;
use ultraflows_store::StoreError;

/// Errors a workflow function can return, and runtime errors.
///
/// `Suspended` and `Cancelled` are *control flow*, not failures: they
/// must never consume a retry attempt. Only `Failed` does.
#[derive(Debug, Error)]
pub enum FlowError {
    /// The workflow suspended on a wait. The executor marks it Waiting
    /// and does not count an attempt against the retry policy.
    #[error("suspended on wait")]
    Suspended,
    /// Cancellation was requested and observed at a step boundary.
    #[error("cancelled")]
    Cancelled,
    /// A step or the workflow itself failed with this message. This is
    /// the only variant that consumes a retry attempt.
    #[error("failed: {0}")]
    Failed(String),
    #[error("store error: {0}")]
    Store(#[from] StoreError),
}

impl FlowError {
    pub fn failed(msg: impl Into<String>) -> Self {
        FlowError::Failed(msg.into())
    }
}
