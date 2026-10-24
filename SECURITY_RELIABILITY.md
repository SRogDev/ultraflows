# SECURITY & RELIABILITY — UltraFlows

## Reliability threats the design must answer

- Process crash at any point (see fault injection in `TESTING.md`).
- Duplicate delivery: triggers, step records, waitpoint completions and
  external callbacks may all arrive twice — every ingress is idempotent.
- Ambiguous external outcomes: an effect may have happened while its
  result is unknown. UltraFlows propagates deterministic idempotency keys;
  it cannot resolve ambiguity inside systems that ignore them.
- Clock problems: waits/schedules depend on wall time; wake-never-early
  is an invariant, and DST behavior is specified per phase (Phase 2).
- Poison steps: a step that always fails must exhaust its policy and
  surface as `Failed`, never loop forever consuming resources.

## Security considerations

- Payloads are data, never code: the runtime does not `eval` workflow
  payloads, and step results are stored as serialized values.
- Secrets do not belong in workflow payloads or step results (they would
  be persisted in the journal). Docs and API design must steer credentials
  to references (secret IDs), not values. (Design rule from Phase 1.)
- Tenant isolation and authorization become correctness requirements
  when multi-tenancy is introduced (Phase 3). Phase 1 is single-tenant
  and says so; no isolation is claimed before it exists.
- API/service surface (Phase 3) will use per-environment, hashed-at-rest
  keys with separate worker credentials — specified in its own ADR then.

## Operational rules

- Every retry/timeout/concurrency/resource limit is explicit and logged.
- The event log is append-only; repairing state means appending a
  compensating event, never rewriting history.
- Upgrades must state what happens to in-flight long-lived runs.
