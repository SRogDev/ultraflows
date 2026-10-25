# ADR-0009 — Trigger-core scope: runtime only, Trigger-shaped surface

- **Status:** Accepted (2026-10-25)
- **Context:** Roger ordered UltraFlows converted into a functional
  clone of Trigger.dev's core runtime in Rust, so his own apps can
  send it jobs — while he is away, with autonomous work authorized
  until the task is done and tested.
- **Decision:**
  1. **Runtime only.** The server schedules work and serves the
     worker protocol; it never runs task code. External apps define
     tasks (Rust SDK), register them, trigger runs, and work leases.
     No UI/dashboard/webapp, no build/deploy pipeline, no billing,
     no AI agents/chat, no CRIU checkpoints, no ClickHouse/Redis.
  2. **Trigger-shaped surface, own protocol.** The API and
     programming model (tasks, trigger/batch-trigger, waits,
     triggerAndWait, schedules, idempotency scopes, concurrency
     caps) follow Trigger.dev's behavior as established by REA
     static analysis + the prior docs recon. It is deliberately NOT
     wire-compatible with Trigger.dev's hosted API.
  3. **REA as the analysis tool.** The core-runtime packages
     (run-engine, run-store, core, trigger-sdk, redis-worker,
     schedule-engine, webhook-engine) were mapped with REA
     (`rea-agents`, static JS/TS analysis) plus direct reading; the
     map is `docs/TRIGGER_CORE_MAP.md`. REA evidence lives outside
     the repo (`~/workspace/trigger-recon-source/rea-evidence/`).
     Six of seven evidence files are partial (30,000 semantic-node
     cap); claims rest on file:line citations + direct reading, and
     anything not established is listed in the map's UNKNOWN section
     — never invented.
  4. **Legal/honesty.** No Trigger.dev source code was copied into
     UltraFlows (the map contains identifiers and short paraphrases
     only). Trigger.dev is Apache 2.0; UltraFlows is Elastic
     License 2.0. No Trigger trademark is used in UltraFlows naming
     or marketing; docs describe it as "Trigger-shaped /
     Trigger-compatible programming model" only. No claims of
     wire-compatibility, production-readiness, multi-node safety, or
     performance numbers without evidence.
  5. **ADR-0007 narrowed.** "Unspecified behavior defaults to
     Temporal" now applies only where Trigger semantics are silent.
     Where Trigger behavior is established (lease, waitpoints,
     idempotency scopes, retry shape), Trigger wins.
  6. **Durability engine unchanged.** Journaled step memoization +
     deterministic replay (ADR-0001) remains the engine; only the
     surface changed (see `docs/TRIGGER_CORE_SPEC_DELTA.md`).
- **Excluded (deliberately, this increment):** webapp/dashboard,
  CLI/deploy/build, billing, AI agents/chat, ClickHouse, Redis-as-queue,
  checkpoints/CRIU, webhooks + webhook waiters, debounce, gates,
  batch waitpoints (`batchTriggerAndWait`), declarative code
  schedules, Postgres backend, auth, multi-node operation, `L` cron
  syntax (rejected with a clear error).
- **Known limitations (honest, carried from v0.1.0 and new):**
  single node; SQLite only; no auth (localhost-only bind); a run
  `Running` at kill time is reclaimed only via lease expiry (30 s)
  on the remote path — the embedded executor still has no lease
  expiry; waitpoint timeout semantics are unestablished (a
  never-completed token wait blocks forever unless cancelled);
  schedule DST behavior is tested for the Havana spring/fall cases
  only; concurrent duplicate triggers rely on the single-connection
  pool, not on a distributed lock.
- **Consequences:** The v0.1.0 in-process demo workflows and
  `POST /v1/runs` are removed; `scripts/demo.sh` forwards to
  `scripts/trigger-demo.sh`. The next increment should pick from:
  Postgres backend, declarative schedules, batch waitpoints, or
  webhook waiters — in that order of runtime value.
