# ADR-0007 — Temporal as the default reference

- Status: **Accepted** (Roger, 2026-10-23 — "lo que quede no claro,
  hazlo similar a Temporal de momento")

## Decision

When a behavior, term or semantic is unspecified in UltraFlows' own
docs, adopt **Temporal's documented semantics** as the provisional
default — e.g. its workflow/activity split, event-history replay,
signals/updates/queries vocabulary for external interaction, retry-policy
shape (initial interval, backoff coefficient, maximum interval,
maximum attempts), task queues, worker polling, workflow-ID uniqueness
and timeout taxonomy (execution / run / task / schedule-to-close etc.) —
and record the adoption in the relevant ADR or doc when it is used.

## Limits (what "similar to Temporal" does NOT mean)

- Not Temporal's scale: no multi-node clustering, shard distribution,
  Cassandra/Elasticsearch tiers or global namespaces until measured
  need exists (ROADMAP Phase 4 at the earliest).
- Not wire-compatibility: UltraFlows does not implement Temporal's
  protobuf protocol or promise SDK interoperability.
- Not a copy of its code: Temporal is MIT-licensed, but UltraFlows is a
  clean implementation guided by its *documented behavior*.
- Where Temporal and Trigger.dev's documented models conflict, the
  conflict is surfaced to Roger, not resolved silently by this ADR.

## Factual note (verified 2026-10-23)

Temporal's server is written in **Go** (MIT license). Its **SDK Core —
the shared workflow-execution engine under the Python/TypeScript/etc.
SDKs — is written in Rust**, and pre-1.0 Rust client crates
(`temporalio-client`, `temporalio-common`, …) are published from that
same sdk-core monorepo. So "Temporal is Rust" is true of its SDK core,
not of its server.
