# UltraFlows (temporary name)

A native Rust **durable workflow runtime**: long-running workflows — many
steps, dependencies, waits, retries, schedules, external tool calls and
human approvals — that survive process crashes and restarts, over days,
months or years.

UltraFlows is being built as the future durable-execution layer for
Polygrow, but it is **not coupled to Polygrow**: it knows nothing about
business logic, tenants' companies, agent frameworks or AI models. An AI
call, for the runtime, is just one more external step whose *recorded
result* is durable — the runtime never assumes it is deterministic.

## Status

**Phase 0 — scaffold only.** The repository layout, stack and development
workflow exist. **No runtime behavior is implemented yet**, and nothing in
this repository is durable, reliable or fast until the tests and
benchmarks in `TESTING.md` prove it, phase by phase.

## Concepts (kept distinct on purpose)

| Concept | Meaning |
|---|---|
| Workflow definition | The code/shape of a process, versioned |
| Workflow execution (run) | One execution of a definition, with its own durable state |
| Step | One named unit of work inside a run, whose result is journaled |
| Attempt | One try at executing a run; a run may need several |
| Business task | A Polygrow-level concept — *not* every business task becomes a durable workflow |

## Honesty contract

- Attempts execute **at-least-once**.
- A step whose result was recorded is never executed again for that run.
- An external effect is exactly-once **only** if the external system
  accepts the idempotency key UltraFlows propagates. No universal
  exactly-once is claimed, ever.

## Layout

```text
crates/
  ultraflows-core/    pure types, state machine, invariants (no Tokio, no I/O)
  ultraflows-store/   Store trait + backends (Postgres real; SQLite/memory for tests)
  ultraflows/         runtime library: workflow/activity API + Tokio scheduler
  ultraflows-server/  the service binary (API + scheduler + worker gateway)
docs/adr/             architecture decision records
```

Runnable examples arrive with Phase 1 and a benchmark harness when
there is something real to measure — neither exists yet.

## Development

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Toolchain is pinned in `rust-toolchain.toml` (Rust 1.98.1, edition 2024).
See `ROADMAP.md` for the four phases and their exit criteria,
`ARCHITECTURE.md` for boundaries and invariants, and `TESTING.md` for the
testing and benchmarking strategy.

## License

Elastic License 2.0 — see `LICENSE`. Decided 2026-10-24 in
`docs/adr/0006-license-and-name.md`: source-available, so the runtime
stays public and studyable while nobody may resell it as a hosted
service.
