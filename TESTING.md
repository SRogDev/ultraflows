# TESTING — UltraFlows

Correctness first, performance second, claims last.

## Layers

1. **Unit tests** — the core state machine and policies (`ultraflows-core`
   has no async/I/O, so these are fast and synchronous by design).
2. **Property tests** (`proptest`, dev-dependency of core): invariants
   I1–I7 from `ARCHITECTURE.md` under randomized transition sequences,
   random failure points and random interleavings.
3. **Integration tests** — runtime + real store backend: journal
   insert-once under concurrency, crash/restart cycles against a real
   database file (SQLite Phase 1).
4. **Fault injection** — a harness that kills the process (`SIGKILL`) at
   randomized points (after a step's effect but before its record; after
   the record; during a wait) and audits the journal afterwards. This is
   the Phase 1 crown-jewel test; a durability claim without it is void.
5. **Differential/behavioral probes** (from Phase 2): retry timing, wait
   accuracy, concurrency invariants, schedule/DST behavior — each probe
   declares setup, actions, fault injections, assertions and tolerances.

## Benchmarks

- No benchmark numbers exist yet and none may be quoted until the harness
  produces them on a **fixed, published profile** (hardware, limits,
  workload, versions — a manifest committed with the results).
- `criterion` will be added as a dev-dependency **when the first real
  benchmark is written**, not before: an empty bench harness measures
  nothing and invites invented numbers.
- Raw results are committed as data files next to the manifest.

## Rules

- A failing property/fault test blocks merges to `main`.
- Tests must name the invariant they protect.
- Never weaken a tolerance to make a test pass without an ADR entry.
