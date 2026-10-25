use crate::error::CoreError;
use crate::types::RunStatus;

/// The only legal transitions (I1). Anything else is a bug, not a
/// "best effort" — callers get an error, the run keeps its state.
pub fn can_transition(from: RunStatus, to: RunStatus) -> bool {
    use RunStatus::*;
    matches!(
        (from, to),
        (Pending, Running)
            | (Pending, Cancelled)
            | (Pending, Expired)
            | (PendingVersion, Pending)
            | (PendingVersion, Cancelled)
            | (Running, Waiting)
            | (Running, Completed)
            | (Running, Failed)
            | (Running, Pending) // scheduled retry
            | (Running, Cancelled)
            | (Waiting, Running) // timer wake claimed directly
            | (Waiting, Pending) // approval completed / rescheduled
            | (Waiting, Cancelled)
    )
}

pub fn check_transition(from: RunStatus, to: RunStatus) -> Result<(), CoreError> {
    if can_transition(from, to) {
        Ok(())
    } else {
        Err(CoreError::InvalidTransition { from, to })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use RunStatus::*;

    #[test]
    fn terminal_states_have_no_outgoing_transitions() {
        for from in [Completed, Failed, Cancelled, Expired] {
            for to in [
                Pending,
                PendingVersion,
                Running,
                Waiting,
                Completed,
                Failed,
                Cancelled,
                Expired,
            ] {
                assert!(!can_transition(from, to), "{from:?} -> {to:?}");
            }
        }
    }

    #[test]
    fn happy_path_and_retry_transitions_are_legal() {
        assert!(can_transition(Pending, Running));
        assert!(can_transition(Running, Waiting));
        assert!(can_transition(Waiting, Pending));
        assert!(can_transition(Running, Pending));
        assert!(can_transition(Running, Completed));
        assert!(can_transition(Running, Failed));
        assert!(can_transition(Pending, Cancelled));
        assert!(can_transition(Waiting, Cancelled));
        assert!(can_transition(Running, Cancelled));
        assert!(can_transition(PendingVersion, Pending));
        assert!(can_transition(PendingVersion, Cancelled));
        assert!(can_transition(Pending, Expired));
        assert!(!can_transition(PendingVersion, Running));
        assert!(!can_transition(Pending, PendingVersion));
    }

    #[test]
    fn illegal_transitions_are_rejected() {
        assert!(!can_transition(Waiting, Completed));
        assert!(!can_transition(Pending, Completed));
        assert!(!can_transition(Pending, Waiting));
        assert!(!can_transition(Completed, Pending));
        assert_eq!(
            check_transition(Waiting, Completed),
            Err(CoreError::InvalidTransition {
                from: Waiting,
                to: Completed
            })
        );
    }
}
