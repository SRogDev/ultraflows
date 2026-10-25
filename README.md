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

**v0.1.0 — working single-node demo runtime.** Trigger workflows over
HTTP, journaled steps with replay, timer waits, human-approval
waitpoints, retries with backoff, cancellation and an append-only event
log — all on SQLite, with 11 tests and a `kill -9` restart demo. See
"Limitations" below and `docs/adr/0008-portfolio-v010-scope.md` for
exactly what is *not* implemented yet. Nothing here is claimed durable,
reliable or fast beyond what the tests and the demo prove.

## For recruiters (30 seconds)

UltraFlows is a Temporal-shaped durable workflow runtime in Rust:
workflow code is deterministic, side effects live in steps, and a run's
event history lets it survive crashes and long waits. The demo proves
the core claim end-to-end: start a run, kill the server with `kill -9`
while it waits on a human approval, restart it, approve, and watch it
complete — with earlier steps replayed from the journal, never
re-executed.

## Quickstart

```bash
# Run the full demo (server, approval wait, kill -9, restart, retries, timer)
bash scripts/demo.sh

# Or run the server yourself
ULTRAFLOW_DB=ultraflows.db PORT=3000 cargo run --bin ultraflows-server
curl -s localhost:3000/health
curl -s -X POST localhost:3000/v1/runs -H 'content-type: application/json' \
  -d '{"workflow":"client-onboarding","payload":{"company":"Acme"}}'
curl -s -X POST localhost:3000/v1/approvals/onboarding:Acme \
  -H 'content-type: application/json' -d '{"result":{"approved":true}}'
```

Workflows available in v0.1.0: `client-onboarding` (steps + approval
wait), `flaky-report` (retries), `delayed-reminder` (timer wait).
API: `POST /v1/runs`, `GET /v1/runs`, `GET /v1/runs/{id}`,
`GET /v1/runs/{id}/events`, `GET /v1/runs/{id}/steps`,
`POST /v1/runs/{id}/cancel`, `POST /v1/approvals/{token}`.

## Limitations (v0.1.0, stated plainly)

- Single node, single in-process worker; SQLite only (PostgreSQL is a
  trait away, not implemented yet).
- No authentication — bind is localhost-only by default; do not expose
  it to an untrusted network.
- No schedules/cron, no remote workers, no multi-tenancy yet.
- Crash recovery proven for Waiting/Pending runs; a run that is
  Running at kill time needs lease expiry (next increment) to be
  reclaimed automatically.
- No performance claims: no benchmark has been run yet.

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

Runnable demos live in `scripts/demo.sh` and in the server's
`workflows.rs`. A benchmark harness arrives when there is something
real to measure — no numbers are quoted before then.

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
