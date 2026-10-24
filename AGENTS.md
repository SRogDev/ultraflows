# AGENTS.md — working agreement for UltraFlows

- **Mentoring protocol (owner's rule):** this is an engineering
  apprenticeship project. Explain before implementing; for genuine
  architectural decisions present ≥3 options with trade-offs and let
  Roger choose; ask him to reason before revealing the preferred answer.
  No code beyond what was agreed. Small, reviewable commits.
- **Decisions live in `docs/adr/`.** Anything unspecified defaults to
  Temporal's documented semantics (ADR-0007) — record the adoption
  where it is used.
- **Honesty rules:** never claim durable/reliable/fast/production-ready
  without test or benchmark evidence in the repo. Attempts are
  at-least-once; recorded steps are exactly-once; external effects are
  exactly-once only via propagated idempotency keys.
- **Layering:** `ultraflows-core` never depends on Tokio, I/O or a
  database. Storage only through the `Store` trait (ADR-0003).
  Workflow code is deterministic; side effects live in activities.
- **Quality gates:** `cargo fmt --check`, `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo test --workspace` — all green
  before a commit. New dependencies need a stated problem (ADR-0005).
- **Docs:** every significant change ships a README touch-up in the same
  change; update `STATUS.md` when a milestone completes.
- **Git:** local-only while the owner's GitHub account is suspended —
  zero GitHub calls. Local branches per logical change become PRs later.
