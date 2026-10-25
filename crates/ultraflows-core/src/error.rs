use crate::types::RunStatus;
use thiserror::Error;

/// Errors of the pure core. No I/O, no storage, no async runtime here.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CoreError {
    #[error("invalid run transition: {from:?} -> {to:?}")]
    InvalidTransition { from: RunStatus, to: RunStatus },
}
