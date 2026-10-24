# ADR-0002 — Runtime shape: service-first

- Status: **Accepted** (Roger, 2026-10-23 — option B)
- Deciders: Roger

## Context

UltraFlows could start as (A) an embedded Rust library, (B) a service —
one binary with an API, a scheduler and external workers — or (C) a
hybrid sequencing library-then-service.

## Decision

**Service-first (B)**, following the Temporal shape: a server owns
durable state and scheduling; workers are separate processes that host
workflow/activity code and poll for work. Internally the service is
still built on a pure core library (`ultraflows-core`) and a runtime
library (`ultraflows`) — that layering is an implementation discipline,
not the product shape.

## Consequences

- Phase 1 must include: the server binary, a minimal worker protocol,
  and PostgreSQL as the real store (ADR-0003) — it is bigger than a
  library-only Phase 1, and ROADMAP.md phases it as 1A (core slice) →
  1B (service shell) so the system stays working at each step.
- Workers in other languages become possible later over the same
  protocol; v1 workers are Rust (ADR-0004).

## Revisit if

An embedding use case (Polygrow process wanting in-process execution)
becomes primary — the library crates remain usable for that without
changing the service.
