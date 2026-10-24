# ADR-0003 — Storage: Store trait, Postgres real, SQLite/memory for tests

- Status: **Accepted** (Roger, 2026-10-23 — option C)
- Deciders: Roger

## Context

Options: (A) PostgreSQL from day one for everything, (B) embedded
SQLite first, (C) a `Store` trait with multiple backends — in-memory and
SQLite for tests/development, PostgreSQL as the real backend.

## Decision

**C.** The runtime's correctness rules may only depend on the `Store`
trait contract (transactions, insert-once step records, append-only
event log, claim-with-lease semantics). PostgreSQL is the production
backend for the service; SQLite and in-memory backends exist so tests
run fast and anywhere.

## Consequences

- The trait contract must be written *before* its implementations, with
  a shared conformance test-suite run against every backend. A behavior
  that only one backend passes is a contract bug.
- Risk recorded: SQL dialect differences (locking, `SKIP LOCKED`,
  isolation) can leak through the trait. Mitigation: the contract states
  semantics, not SQL; the Postgres conformance suite is the authority,
  SQLite/memory are explicitly test-grade.
- Driver: `sqlx` (async, compile-time-agnostic query API — no
  compile-time DB requirement in CI).

## Revisit if

Conformance shows the trait forcing lowest-common-denominator semantics
that weaken the Postgres backend's guarantees — then Postgres becomes
the only *supported* backend and the others shrink to test doubles.
