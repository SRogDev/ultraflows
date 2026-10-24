# ADR-0004 — Workflow definitions: code-first Rust

- Status: **Accepted** (Roger, 2026-10-23 — Muse's recommendation)
- Deciders: Roger (approval)

## Decision

Workflows and activities/steps are **code-first Rust**: typed async
functions registered with a worker, in Temporal's workflow/activity
split:

- *Workflow functions* — deterministic orchestration only.
- *Activities/steps* — anything with side effects or non-determinism
  (HTTP calls, AI calls, DB writes), executed at-least-once with
  retries, their results recorded in the run's history.

No declarative/graph definition format in v1. If one is ever needed, it
must compile down to this same model.
