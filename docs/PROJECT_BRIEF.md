# PROJECT BRIEF — UltraFlows (for handoff to another AI/engineer)

**What:** a native Rust durable workflow runtime (temporary name),
service-first, modeled semantically on Temporal: deterministic workflow
code, side effects in activities/steps, an append-only event history,
journaled results, durable timers/waits, retries, schedules and human
approvals. Built to become Polygrow's durable-execution layer, with zero
coupling to Polygrow's business logic or to any agent framework.

**Why native Rust when Temporal exists:** learning (the owner is using
this project to master Rust/systems engineering — mentoring protocol in
AGENTS.md), a single small binary + PostgreSQL operational footprint,
and a runtime shaped to Polygrow's future needs.

**Key decisions:** docs/adr/0001–0007. Unspecified = Temporal semantics.

**State:** Phase 0 scaffold only. No runtime behavior implemented, no
performance or durability claims proven yet. Exit criteria per phase in
ROADMAP.md.
