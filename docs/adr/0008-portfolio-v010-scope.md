# ADR-0008 — v0.1.0 portfolio scope: what "functional" means here

- Status: **Accepted** (Roger's call, 2026-10-25: job search starts in
  2 days; ship a working, honest demo now, depth later)

## Decision

v0.1.0 is a **working single-node demo runtime**, not the full
Phase 1–4 vision:

- Implemented: core state machine + retry policy, SQLite `Store`
  implementation, journaled steps with replay, timer waits, approval
  waitpoints, retries with backoff, cancellation, append-only event
  log, HTTP API, one in-process worker, 3 demo workflows, 11 tests,
  and a `kill -9` restart demo script.
- Explicitly NOT implemented (and not claimed): PostgreSQL backend
  (the `Store` trait exists, the implementation does not — ADR-0003's
  "Postgres is real" is deferred to the next increment), remote
  workers, multi-worker claiming/lease expiry, schedules/cron,
  authentication, multi-tenancy, and any performance claim.
- Crash-recovery boundary in v0.1.0: a run that is Waiting (timer or
  approval) or Pending (retry) survives a server kill and resumes
  correctly. A run that is **Running** at kill time stays Running
  until the worker restarts and is reclaimed manually — there is no
  lease expiry yet, so this release does **not** claim full
  crash-resume for in-flight runs. The demo kills the server during a
  wait, which is the boundary actually proven.

## Rationale

A recruiter-facing portfolio piece must be runnable and truthful in
the same sentence. Claiming Postgres/multi-worker durability that is
not implemented would fail the first code read; shipping a smaller
system whose every claim is backed by a test or a demo step survives it.

## Revisit

The next increment is, in order: (1) lease expiry + Running-run
recovery, (2) PostgreSQL backend against the same trait, (3) remote
workers, (4) schedules. Each lands with tests before the README claims it.
