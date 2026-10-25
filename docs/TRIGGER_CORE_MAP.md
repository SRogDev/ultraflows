# Trigger.dev core runtime — module map for the UltraFlows reimplementation (DRAFT)

> **Status:** reviewed by the implementation coordinator 2026-10-25 and
> committed as the analysis record for the Trigger-core increment.
> No Trigger.dev source code is quoted in this file (identifiers,
> file paths, and short paraphrases only).
> This file contains **no Trigger.dev source code** — only identifiers, file
> paths, and short paraphrases of observable behavior. It is an analysis
> artifact, not a copy of upstream code.

## 0. Provenance and honesty rule

- **Source:** public repository `triggerdotdev/trigger.dev`, branch `main`,
  downloaded anonymously via codeload tarball (no token, no api.github.com)
  on 2026-10-25. Extracted tree (analysis scratch, NOT part of UltraFlows):
  `~/workspace/trigger-recon-source/trigger.dev-main/` — hereafter
  `trigger.dev-main/`.
- **License of the studied code:** Apache License 2.0 (see
  `trigger.dev-main/LICENSE`). UltraFlows itself is Elastic License 2.0 and
  contains no code copied from this tree; it is reimplemented in Rust from
  behavioral understanding only.
- **Tarball:** `trigger-dev.tar.gz`, 34 MB, 8947 files,
  sha256 `d5321e046a0ce086e402f85ec029e62778f9d889dd751e5a5def467df3e23507`.
  (Full note: `~/workspace/trigger-recon-source/UPSTREAM.md`.)
- **Date of this map:** 2026-10-25.
- **Tool:** REA (`rea-agents`, CLI one-shot, zero-engine JS/TS static
  analysis): `rea analyze-javascript-application <abs-src-dir>
  --max-semantic-nodes 30000 --json`. REA binary:
  `~/workspace/reverse-lab/node_modules/.bin/rea`, Node v24.22.1.
  Evidence JSONs live under `~/workspace/trigger-recon-source/rea-evidence/`
  (NOT in UltraFlows).
- **Prior docs-only recon** (cross-check only, not the source of claims here):
  `~/workspace/trigger-rust-recon/` (`RECON.md`, `SPEC_RUST_RUNTIME.md`,
  `FEATURE_MATRIX.csv`).

### Honesty rule (binding for this document)

Static analysis does **not** prove runtime execution. Every REA claim below
is a claim about *inert syntax*: which files exist, which names are imported
and exported, and which static relations REA could resolve. REA's own
limitations, repeated verbatim from every evidence file:

- "CommonJS and ESM binding relationships were recovered from inert syntax
  and resolved only within the inventoried artifact container."
- "JavaScript and HTML were parsed as inert text; bundle bootstrap code was
  never executed."
- "Static imports, entrypoints, workers, endpoints, and storage relationships
  do not prove runtime execution."
- "Static paths and relationships may remain unresolved when expressions are
  dynamic or obfuscated."
- "Webpack/Rspack factories were recovered from AST literals; REA did not
  invoke push handlers or bundle bootstrap code."

Additional limits found in this run:

- The `--max-semantic-nodes 30000` cap **truncated** the semantic graph for
  every package except schedule-engine (`coverage.status = "partial"`,
  `truncated = true`). Application-graph node/edge counts below are
  complete for the inventoried files; semantic-node counts are capped, so
  per-function relation claims are partial, not exhaustive.
- REA inventoried each `src/` directory **including its test/bench files**
  (there is no test exclusion flag in the verified invocation), so
  `parsed_javascript_files` counts include tests. Test files were excluded
  by hand when reading source directly.
- REA resolves imports only inside the inventoried container: cross-package
  imports (e.g. run-engine importing `@trigger.dev/database` or
  `@trigger.dev/core`) appear as unresolved/dynamic edges, not as verified
  links. Cross-package flow claims in §8 therefore rest on direct source
  reading (named imports/exports), not on REA edges.
- No code was executed, no database or Redis was run, and no live Trigger.dev
  service was contacted. Anything not found in code is listed in §9 UNKNOWN,
  never invented.

### REA evidence table

| Package (src dir run) | Evidence file | evidence_id | Parsed JS files | Parse failures | App-graph nodes / edges | Semantic nodes / relations / unknowns |
|---|---|---|---|---|---|---|
| `internal-packages/run-engine/src` | `rea-evidence/run-engine.json` (67,712,307 B) | `ev_358154304dd547573b2ec2a1173be2a8ca086f549d513815af81f53c9cb0d94b` | 221 | 0 | 2295 / 3863 | 30000 (capped) / 33368 / 10050 |
| `internal-packages/run-store/src` | `rea-evidence/run-store.json` (62,658,974 B) | `ev_ea476554ecec9102208731f2d74cd01bb35801f3c8a8c261790b9685db431311` | 157 | 0 | 1119 / 2237 | 30000 (capped) / 34254 / 10440 |
| `packages/core/src` | `rea-evidence/core.json` (64,445,269 B) | `ev_adfb91d952e0a45656fca2e6732ced022a76477b6275561238a2bff537237262` | 329 | 0 | 2299 / 4168 | 30000 (capped) / 27548 / 9923 |
| `packages/trigger-sdk/src` | `rea-evidence/trigger-sdk.json` (57,048,459 B) | `ev_bdd653107684e7e450d007820391abc9f021a9968908d4b2ff37d23d6570d44c` | 102 | 0 | 796 / 1630 | 30000 (capped) / 29952 / 10812 |
| `packages/redis-worker/src` | `rea-evidence/redis-worker.json` (57,486,994 B) | `ev_d0a597fa2d54cbdcbcfc209c89ba3e53194069e920e3a60a05b6453440decc85` | 38 | 0 | 314 / 543 | 30000 (capped) / 33362 / 11050 |
| `internal-packages/webhook-engine/src` | `rea-evidence/webhook-engine.json` (54,331,524 B) | `ev_36368e0d0d6778fb9d71c74ecea303aa12b78a131703eb13dd8b06b1bdf4496e` | 40 | 0 | 316 / 591 | 30000 (capped) / 30217 / 10811 |
| `internal-packages/schedule-engine` (prior run, package dir incl. tests) | `rea-evidence/schedule-engine.json` (14,067,825 B) | `ev_5377e64d6c119e3a52106085386fe224e11651d8be15d9f88847a9cbc1011383` | 15 | 0 | 115 / 189 | 7341 / 8101 / 2755 (not truncated) |

Notes on the table:

- All evidence_ids above were verified with
  `jq -r .evidence_id rea-evidence/<name>.json` after the runs completed
  (an early draft transcription of the core id was wrong and has been
  corrected to `ev_adfb91d952e0a45656fca2e6732ced022a76477b6275561238a2bff537237262`).
- The schedule-engine evidence is from a **prior** run over the whole
  package directory (20 relevant files incl. `package.json`, tests, tsconfig),
  not over `src/` only. Its semantic graph was not truncated. It was not
  re-run for this map; its `src/engine/*` files were skim-read directly
  instead.
- Application-graph edge relations are dominated by `imports` edges
  (e.g. run-engine 3237, core 2208, trigger-sdk 1105, redis-worker 375),
  plus `contains` and `exposes` edges; `calls` edges at application-graph
  level are few (3–14 per package) — REA's call resolution at that level is
  weak, and the semantic graph carries the richer `calls`/`awaits`/`reads`
  relations instead. Do not read "few calls edges" as "few calls in code".
- No REA run failed or timed out. The 600 s per-package timeout in the run
  script was never hit; all six packages completed. (An early draft of
  this file was written while webhook-engine was still running; its row
  has since been completed and the draft notes removed.)

## 1. run-engine — the control plane (`internal-packages/run-engine/src`)

**What it does:** the server-side run lifecycle engine. It owns creating
runs, enqueueing them, leasing them to workers (dequeue), starting and
completing attempts, cancelling, expiring, retrying, and resuming runs that
are blocked on waitpoints. It does not execute user task code itself; it
hands a dequeued run to a worker and tracks the result.

**Entry point:** `internal-packages/run-engine/src/index.ts` exports
`RunEngine` (from `engine/index.ts`), the batch queue
(`BatchQueue`, `BatchCompletionTracker` from `batch-queue/index.ts`), and
waitpoint fan-out support (`WaitpointFanoutWorker`).

**Key modules (non-test files only):**

| File | Role |
|---|---|
| `engine/index.ts` — `class RunEngine` | Public facade: `trigger`, `createCancelledRun`, `createFailedTaskRun`, `dequeueFromWorkerQueue(s)`, `dequeueFromEnvironmentWorkerQueue`, `startRunAttempt`, `completeRunAttempt`, `cancelRun`, `rescheduleDelayedRun`, queue/concurrency introspection (`lengthOfQueue`, `currentConcurrencyOfQueue`, `getQueueConcurrencyLimit`, …), waitpoint ops (`createDateTimeWaitpoint`, `createManualWaitpoint`, `blockRunWithWaitpoint`, `completeWaitpoint`, `getOrCreateRunWaitpoint`), batch ops (`initializeBatch`, `enqueueBatchItem`, `tryCompleteBatch`, `expireBatch`), checkpoint ops (`createCheckpoint`, `continueRunExecution`, `getRunExecutionData`, `getSnapshotsSince`, `heartbeatRun`, `reportRestoreOutcome`), `registerRacepointForRun`. Its internal worker catalog (below) is wired as job handlers: `finishWaitpoint`, `heartbeatSnapshot`, `repairSnapshot`, `expireRun`, `cancelRun`, `queueRunsPendingVersion`, `expireParkedExternalDeploymentRun`, `tryCompleteBatch`, `expireBatch`, `continueRunIfUnblocked`, `ensureRunFinalized`, `ensureWaitpointCompleted`, `enqueueDelayedRun`, `expireTtlRun`. |
| `engine/types.ts` — `RunEngineOptions`, `TriggerParams` | `TriggerParams` is the engine-side trigger contract: `friendlyId`, `environment`, `taskIdentifier`, `payload` + `payloadType`, `queue`, `idempotencyKey` + `idempotencyKeyExpiresAt` + `idempotencyKeyOptions { key, scope: "run" \| "attempt" \| "global" }`, `concurrencyKey`, `gates`, `delayUntil`, `queuedAt`, `maxAttempts`, `ttl`, `tags`, `parentTaskRunId` / `rootTaskRunId`, `batch { id, index, resumeParentOnCompletion }`, `machine` (preset name), `maxDurationInSeconds`, `priorityMs`, `workerQueue`, `region`, `lockedToVersionId`, trace fields (`traceId`, `spanId`, `parentSpanId`). |
| `engine/statuses.ts` | Status predicates. Dequeueable execution statuses: `QUEUED`, `QUEUED_EXECUTING`. Executing: `EXECUTING`, `EXECUTING_WITH_WAITPOINTS`. Checkpointable: `RUN_CREATED`, `QUEUED`, `EXECUTING`, `EXECUTING_WITH_WAITPOINTS`, `QUEUED_EXECUTING`. Final run statuses: `CANCELED`, `INTERRUPTED`, `COMPLETED_SUCCESSFULLY`, `COMPLETED_WITH_ERRORS`, `SYSTEM_FAILURE`, `CRASHED`, `EXPIRED`, `TIMED_OUT`. |
| `engine/systems/enqueueSystem.ts` — `EnqueueSystem` | `enqueueRun`, `publishRun`. Note found in source: TTL is included only on the first enqueue from trigger; re-enqueues (waitpoint, checkpoint, delayed, pending-version) must not add TTL. |
| `engine/systems/dequeueSystem.ts` — `DequeueSystem` | `dequeueFromWorkerQueues`. Observed behavior in source comments: locks the run before mutating it; a stale message for a finished run is removed instead of started; a `PENDING_EXECUTING` snapshot is left for the stalled-run path; a worker mismatch puts the message back in the queue; max attempts falls back to the task's retry config when not set on the trigger. |
| `engine/systems/runAttemptSystem.ts` — `RunAttemptSystem` | `startRunAttempt`, `completeRunAttempt`, `attemptSucceeded`, `attemptFailed`, `systemFailure`, `tryNackAndRequeue`, `reportRestoreOutcome`, `cancelRun`, `ensureRunFinalized`, `resolveTaskRunContext`. All mutating paths take the per-run lock (`runLock.lock(...)`). |
| `engine/systems/waitpointSystem.ts` — `WaitpointSystem` | `createDateTimeWaitpoint`, `createManualWaitpoint`, `blockRunWithWaitpoint` (+ lockless variant), `blockRunAndCompleteWaitpoint`, `completeWaitpoint`, `continueRunIfUnblocked`, `ensureWaitpointCompleted`, `getOrCreateRunWaitpoint`, `clearBlockingWaitpoints`. Completion is guarded (`#scheduleCompletionGuard` / `#ackCompletionGuard`). |
| `engine/systems/checkpointSystem.ts` — `CheckpointSystem` | `createCheckpoint`, `continueRunExecution` (both under the run lock). Description constant: run continued after being suspended. |
| `engine/systems/executionSnapshotSystem.ts` — `ExecutionSnapshotSystem` | `createExecutionSnapshot`, `heartbeatRun`, `restartHeartbeatForRun`, `enqueueHeartbeatIfNeeded`, plus pure helpers `enhanceExecutionSnapshotWithWaitpoints`, `getLatestExecutionSnapshot(OrFinishedRun)`, `executionResultFromSnapshot`, `executionDataFromSnapshot`, `getExecutionSnapshotsSince`. |
| `engine/systems/batchSystem.ts` — `BatchSystem` | `scheduleCompleteBatch`, `performCompleteBatch`, `scheduleExpireBatch`, `expireBatch`, `#tryCompleteBatch`, `#resumeBatchParent`. |
| `engine/systems/debounceSystem.ts` — `DebounceSystem` | `DebounceOptions`, `DebounceResult`; `RunEngine.trigger` handles debounce before creating a new run. |
| `engine/systems/delayedRunSystem.ts` — `DelayedRunSystem` | Delayed-run enqueueing (`scheduleDelayedRunEnqueuing` on the engine). |
| `engine/systems/ttlSystem.ts` — `TtlSystem` | TTL expiry (`expireTtlRun` job, `scheduleExpireRun`). |
| `engine/systems/pendingVersionSystem.ts` — pending-version runs | Runs that cannot execute yet because their task/queue is not deployed; `queueRunsPendingVersion` job; `PARKED_ON_EXTERNAL_DEPLOYMENT_STATUS_REASON`; parked external-deployment expiry. |
| `engine/systems/completedWaitpointResolver.ts`, `completedWaitpointRecordAdapter.ts`, `waitpointResultRunStore.ts` | Resolving completed-waitpoint records into resume data for a run (the "what did the wait return" path). |
| `engine/systems/raceSimulationSystem.ts` | Racepoint support (`registerRacepointForRun`). Semantics beyond the name were not established — see §9. |
| `engine/waitpointCoordinator/` | Two coordinator implementations behind a `WaitpointCoordinator` interface: `storeCoordinator.ts` (`WaitpointStatus = "PENDING" \| "COMPLETED"`, `WaitpointCompletion` inline-vs-ref with `MAX_INLINE_COMPLETION_OUTPUT_BYTES = 524_288`, `EncodedWaitpointCompletion`) and `legacyPostgresCoordinator.ts`. Plus `fanoutWorker.ts` (`WaitpointFanoutWorker`, fan-out of one completion to many blocked runs, with `fanoutPolicy.ts`: `completionFingerprint`, `DEFAULT_FANOUT_RETRY_POLICY`, `fanoutRetryDelayMs`), `keys.ts` (Redis key layout: per-waitpoint keys, per-run block keys `pend`/`done`/`edge`/`state`, fan-out index partitioned into 16 partitions, `idempotencyKey(environmentId, key)`), `scripts.ts` (Redis Lua commands via `registerWaitpointCommands`). |
| `engine/locking.ts` — `RunLocker`, `LockAcquisitionTimeoutError` | Per-run distributed lock used by essentially every mutating system above. This is the engine's core concurrency primitive: one run, one writer at a time. |
| `engine/retrying.ts` — `retryOutcomeFromCompletion` | Maps an attempt completion to `RetryOutcome`: `cancel_run`, `fail_run` (with sanitized error), or `retry` (`method: "queue" \| "immediate"`, settings, optional machine). Cancelled-error code short-circuits to `cancel_run`; OOM errors retry on a larger machine if one is available, else fail. Delegates to core helpers `calculateNextRetryDelay`, `shouldRetryError`, `isOOMRunError`, `sanitizeError`. |
| `engine/workerCatalog.ts`, `engine/workerQueueRouting.ts`, `engine/ttlWorkerCatalog.ts`, `engine/controlPlaneResolver.ts`, `engine/eventBus.ts`, `engine/billingCache.ts`, `engine/machinePresets.ts` | Worker-job catalog (job schemas + visibility timeouts, mostly 30 s, 60 s for pending-version/parked jobs; `ensureRunFinalized` retries up to 10,000 attempts, 1 s–300 s backoff); worker-queue routing; TTL worker catalog; control-plane environment resolution; event bus; billing cache (with fallback to the run's stored plan type); machine presets (default `small-1x`). |
| `run-queue/` — `RunQueue` (`run-queue/index.ts`) | The Redis-backed queue layer the engine enqueues/dequeues through: `enqueueMessage`, `dequeueMessageFromWorkerQueue`, concurrency-limit management (per-queue, per-concurrency-key, per-environment total, burst factor), queue lengths, dead-letter queue (`lengthOfDeadLetterQueue`, `messageInDeadLetterQueue`, `redriveMessage`), plus internal jobs `processQueueForWorkerQueue`, `scanConcurrencySets`, `processMarkedRuns`. `run-queue/types.ts`: `QueueGate` (max 4 gates per run: 3 requested + the task's anonymous inline-limit gate — see `gateParsing.ts` `ParsedGate { queue, concurrencyKey? }`), `QueueDescriptor`, `WorkerQueuePopOptions`, `RunQueueSelectionStrategy`. Selection strategies: `fairQueueSelectionStrategy.ts`, `weightedWorkerQueueOrder.ts`. |
| `batch-queue/` — `BatchQueue`, `BatchCompletionTracker` | Batch enqueue/completion tracking used by `BatchSystem`. |
| `shared/index.ts` | `AuthenticatedEnvironment` / `MinimalAuthenticatedEnvironment` types. |

**REA support:** `run-engine.json` — 221 parsed files, 0 parse failures,
2295 application-graph nodes, 3863 edges (3237 `imports`, 442 `contains`,
167 `exposes`, 14 `calls`, 3 `loads`). Semantic graph capped at 30000 nodes
(partial). The export list above was confirmed by direct reading, not by REA
alone.

## 2. run-store — persistence (`internal-packages/run-store/src`)

**What it does:** the storage interface and implementations for runs,
attempts, waitpoints, snapshots, batches, and queues. The engine talks to a
`RunStore`; Postgres is the durable store, Redis holds execution snapshots
and pending units, and decorator stores route between them.

**Entry point:** `internal-packages/run-store/src/index.ts` re-exports
`types.ts`, `PostgresRunStore.ts`, `runOpsStore.ts`, `readReplicaClient.ts`,
`redisSnapshotStore.ts`, `snapshotKeys.ts`, `pendingIndex.ts`,
`pendingRecoveryWorker.ts`, `routingStoreMetrics.ts`, `delegatingRunStore.ts`,
`snapshotEntry.ts`, `snapshotResidency.ts`, `snapshotResidencyResolver.ts`,
`taskRunExecutionSnapshotStore.ts`, `completedWaitpointHydratingRunStore.ts`.

**Key modules:**

| File | Role |
|---|---|
| `types.ts` — `interface RunStore` | The contract. Creation: `createRun(CreateRunInput, tx?)` → `TaskRunWithWaitpoint`, `createCancelledRun`, `createFailedRun`. Lifecycle: `finalizeRun`, `expireRunsBatch`, `lockRunToWorker`, `promotePendingVersionRuns`, `expireParkedRun`, `rescheduleRun`, `enqueueDelayedRun`, `rewriteDebouncedRun`, `updateMetadata`, `clearIdempotencyKey`, `pushTags`, `pushRealtimeStream`. Reads: `findRun`, `findRunOrThrow`, `findRunOnPrimary`, `findRuns`, `findRunsByIds`, `findRunsByIdempotencyKeys`. Batches: `createBatchTaskRun`, `updateManyBatchTaskRun`, `createBatchTaskRunItem`, `countBatchTaskRunItems`, `recordBulkActionMembership`. Snapshots: `createExecutionSnapshot`, `findLatestExecutionSnapshot`, `readSnapshotRoute`, `settlePendingUnit`. Waitpoints: `blockRunWithWaitpointEdges`, `countPendingWaitpoints`, `updateManyWaitpoints`, `markWaitpointCompleted`, `forWaitpointCompletion`, `deleteManyTaskRunWaitpoints`, `findWaitpointConnectedRunIds`, `findSnapshotCompletedWaitpointIds` (+ `WithPresence` variants). Waitpoint tags: `upsertWaitpointTag`, `findManyWaitpointTags`. Checkpoints: `findTaskRunCheckpointById`. `CreateRunInput = { data, snapshot, associatedWaitpoint?, onBirthResidency? }` (plus optional organization flags). `runInTransaction(runId, fn)` runs a co-resident multi-write unit atomically **on the store that owns that run** — explicitly not a cross-DB transaction. |
| `PostgresRunStore.ts` — `class PostgresRunStore implements RunStore` | Postgres implementation via Prisma. `RunStoreSchemaVariant = "legacy" \| "dedicated"` (dual-schema migration in progress; many `*.test.ts` files probe legacy-vs-dedicated behavior). |
| `runOpsStore.ts` | Run-ops (separate-database) variant of the store; the engine's read views are replica-lag aware (many tests named `*.replicaLag.test.ts`). |
| `taskRunExecutionSnapshotStore.ts` — `TaskRunExecutionSnapshotStore extends DelegatingRunStore` | Decorator that dual-writes snapshots: modes `dual-write` / `redis-read` / `redis-only` (`TaskRunExecutionSnapshotStoreMode`, dial `off` + modes). Overrides `createRun`, `createExecutionSnapshot`, `lockRunToWorker`, `completeAttemptSuccess`, `expireRun`, `rescheduleRun`, `findLatestExecutionSnapshot`, `findSnapshotCompletedWaitpointIds*`. |
| `redisSnapshotStore.ts` | Redis snapshot store: prepared/finalized snapshot entries, completed-waitpoint records, quarantine moves, residency markers, recovery leases, read-retry/liveness handling. A redis-primary run has no Postgres snapshot row; its payload is reproduced from Redis (see `types.ts` comment on `LatestExecutionSnapshotRead`). |
| `delegatingRunStore.ts` — `DelegatingRunStore implements RunStore` | Base decorator forwarding every method; subclassed by the snapshot store and the hydrating store. |
| `completedWaitpointHydratingRunStore.ts` — `CompletedWaitpointHydratingRunStore` | Decorator that hydrates completed-waitpoint results into snapshot reads. |
| `pendingIndex.ts` — `PendingIndex` | Index of pending snapshot units. |
| `pendingRecoveryWorker.ts` — `PendingRecoveryWorker`, `RecoverySweeper` | Recovers pending units whose Postgres commit status is `committed` / `aborted` / `in progress` / null; quarantine reasons: `structurally-invalid`, `redis-primary-null`, `base-expired`. |
| `snapshotKeys.ts`, `snapshotEntry.ts`, `snapshotResidency.ts`, `snapshotResidencyResolver.ts`, `placementCatalog.ts`, `readReplicaClient.ts`, `routingStoreMetrics.ts` | Snapshot key layout, snapshot entry encoding, which store "owns" a run's snapshot (residency routing, incl. env-scoped residency), placement catalog, read-replica client, routing metrics. |

**What is stored per run/task (from the Prisma schema, §7):** one `TaskRun`
row (payload as a string + `payloadType`, trace ids, queue, worker queue,
concurrency key, gates, TTL, debounce JSON, idempotency key + expiry +
options JSON, tags, usage/cost, lock fields, parent/child links via
`TaskRunDependency`, batch links), one `TaskRunAttempt` row per attempt
(status, error JSON, output string, usage), `TaskRunExecutionSnapshot` rows
(execution status + run status + completed-waitpoint ids in order +
checkpoint link + heartbeat), `TaskRunCheckpoint` rows (type, location,
image ref), `Waitpoint` rows + `TaskRunWaitpoint` edges, `TaskQueue` rows
(concurrency limits, rate limit JSON, paused flag), `BatchTaskRun` +
`BatchTaskRunItem` rows, `TaskSchedule` + `TaskScheduleInstance` rows.

**REA support:** `run-store.json` — 157 parsed files (mostly tests), 0 parse
failures, 1119 application-graph nodes, 2237 edges (1826 `imports`, 314
`contains`, 88 `exposes`, 9 `calls`). Semantic graph capped at 30000 nodes
(partial). The interface method list above was confirmed by direct reading.

## 3. core — shared types, schemas, retry math (`packages/core/src`)

**What it does:** the shared library every other package builds on: API
client, request/response schemas (zod), task/queue/retry/waitpoint types,
idempotency-key handling, the worker-side queue naming, and the runtime
manager interfaces. It contains no run-lifecycle engine and no persistence.

**Entry point:** `packages/core/src/v3/index.ts` re-exports the apiClient,
clock/logger/task-context/trace-context/runtime/sdkScope/usage/run-metadata/
wait-until/timeout/lifecycle-hooks/locals/heartbeats/streams/waitpoints
APIs, all of `schemas/index.ts`, `types/index.ts`, `idempotencyKeys.ts`,
`jwt.ts`, `apiKeys.ts`, and more. Top-level `packages/core/src/index.ts`
adds `eventFilterMatches.ts`, `logger.ts`, `retry.ts`, `types.ts`.

**Key modules:**

| File / area | Role |
|---|---|
| `v3/schemas/schemas.ts` — `RetryOptions` | The retry contract: `maxAttempts?`, `factor?` ("each subsequent retry is `previousTimeout * factor`"), `minTimeoutInMs?`, `maxTimeoutInMs?`, `randomize?`. Same shape is reused for fetch retries (`v3/schemas/fetch.ts`: `FetchRetryBackoffStrategy`, `FetchRetryOptions` with `byStatus`, `connectionError`, `timeout`). |
| `v3/utils/retries.ts` — `defaultRetryOptions`, `calculateNextRetryDelay` | Defaults: `maxAttempts: 3`, `factor: 2`, `minTimeoutInMs: 1000`, `maxTimeoutInMs: 60000`, `randomize: true`. Delay formula (identifiers only, paraphrased): no delay once `attempt >= maxAttempts`; otherwise `min(maxTimeoutInMs, random * minTimeoutInMs * factor^(attempt-1))`, rounded, where `random = Math.random() + 1` when `randomize` is true, else 1. Note: `maxAttempts` counts attempts including the first (so default 3 = 1 initial + 2 retries) — this matches the docs recon. |
| `v3/schemas/api.ts` | Trigger/batch/waitpoint API bodies. Trigger body fields include `queue { name }`, `delay` (string or date), `idempotencyKey` (max 2048 chars) + `idempotencyKeyTTL` + `idempotencyKeyOptions`, `ttl` (string or non-negative int). Waitpoint bodies: `CreateWaitpointTokenRequestBody/ResponseBody`, `CreateInputStreamWaitpointRequestBody`, `CreateSessionStreamWaitpointRequestBody`, `WaitpointTokenStatus`, `CompleteWaitpointTokenRequestBody/ResponseBody`. |
| `v3/schemas/runEngine.ts` | Engine wire types: `StartRunAttemptResult`, `DequeuedMessage` (the message a worker receives on dequeue). |
| `v3/schemas/queues.ts`, `v3/types/queues.ts` | Queue definitions incl. concurrency configuration. |
| `v3/types/tasks.ts` — `TaskOptions`, `CommonTaskOptions` | Task-definition options: `retry`, `queue` (string or queue object), `machine` (preset, default `small-1x` per `machinePresets`), `maxDuration`, `ttl` ("if the run is not executed within this time, it will be removed from the queue and never execute"; per-trigger override allowed), `handleError` / `catchError` (modify the error or return new retry options), lifecycle hooks (`onStart` first execution only, not per retry; `onStartAttempt`, `onFailure`, `onSuccess`, `onComplete`, `onWait`, `onResume`, `onCancel`, `middleware`), `triggerSource` (internal; `"agent"` for agent tasks). |
| `v3/idempotencyKeys.ts`, `v3/idempotency-key-catalog/` | `idempotencyKeys.create(key, { scope })` with `IdempotencyKeyScope = "run" \| "attempt" \| "global"` (default `"run"`); the in-process catalog maps a key hash back to its original key + scope so metadata is never silently dropped. `create()` on a plain string also hashes it into a branded key. |
| `v3/runEngineWorker/` | Worker-side queue naming and supervision: `workerQueue.ts` (`WORKER_QUEUE_VERSION = "v2"`, `WorkerQueueDispatchClass = "ondemand" \| "scheduled"`, `WorkerQueuePhase = "fresh" \| "restore"`, `WorkerQueueCompatibility = "any" \| "container" \| "compute"`, `WorkerQueueChannel = "stable" \| "canary"`, `SCHEDULED_WORKER_QUEUE_SUFFIX = ":scheduled"`, `parseWorkerQueue` / `formatWorkerQueue` / `restoreWorkerQueue` / `scheduledWorkerQueue`), `supervisor/` (HTTP, session, consumer pool), `workload/` (HTTP, schemas). The `restore` phase is the checkpoint-restore path: a run resumed from a checkpoint re-enters through a restore worker queue. |
| `v3/runtime/` | `manager.ts`, `sharedRuntimeManager.ts`, `noopRuntimeManager.ts`, `preventMultipleWaits.ts` — the runtime-manager interface the worker uses to run task code (exact resume mechanics were not established from these files alone — see §9). |
| `v3/waitpoints/` | `WaitpointTimeoutError`, `ManualWaitpointPromise`. |
| `retry.ts` (top level) | `calculateResetAt` (rate-limit reset math) — distinct from run retry math. |
| `v3/apiClient/` | `triggerTask` and friends (the SDK's HTTP client), `defaultRetryOptions` re-exported here as well, pagination, run-trace pages. |

**REA support:** `core.json` — 329 parsed files, 0 parse failures, 2299
application-graph nodes, 4168 edges (2208 `imports`, 658 `contains`, 1288
`exposes`, 14 `calls`). Semantic graph capped at 30000 nodes (partial).
The retry defaults and formula above were confirmed by direct reading.

## 4. trigger-sdk — the developer-facing SDK (`packages/trigger-sdk/src`)

**What it does:** what task authors import. It defines tasks, triggers them
(via the API client), waits durably, batches, schedules, and exposes
run/queue/concurrency management helpers. It contains no engine and no
queue implementation; its `trigger()` ultimately issues an API call that
the control plane turns into a `RunEngine.trigger`.

**Entry point:** `packages/trigger-sdk/src/v3/index.ts` re-exports
`cache`, `config`, `retry`, `shared` (`queue`, `concurrencyLimit`,
`BatchTriggerError`), `tasks`, `batch`, `wait`, `waitUntil`, `usage`,
`idempotencyKeys`, `tags`, `metadata`, `timeout`, `webhooks`,
`webhookWaiters`, `locals`, `otel`, `schemas`, `heartbeats`, `streams`,
`sessions`, `query`, `schedules` (namespace), `deployments` (namespace),
`queues` (namespace), `concurrencyLimits` (namespace), `envvars`
(namespace), `auth` (`configure`, `auth`), `TriggerClient`,
`prompts`/`skills` (namespaces, AI-agent surface — excluded from the
UltraFlows core map's scope).

**Key modules:**

| File | Role |
|---|---|
| `v3/shared.ts` — `createTask`, `trigger`, `triggerAndWait`, `triggerAndSubscribe`, `batchTriggerById`, `batchTriggerByIdAndWait`, `batchTriggerTasks`, `batchTriggerAndWaitTasks`, `queue`, `concurrencyLimit` | `createTask` normalizes the task definition: queue (a bare string queue name is a *reference* to a queue defined elsewhere and is not registered as a definition; an object queue is registered), `retry` merged over `defaultRetryOptions`, `machine` normalized to `{ preset }`, `maxDuration`, `ttl` carried through. `trigger(task, payload, options?)` → `trigger_internal("tasks.trigger()", …)`. `triggerAndWait` / `triggerAndSubscribe` build a waitpoint on the child run and block the parent on it (the fan-out/fan-in primitive). Batch variants trigger many items and optionally wait for all of them. |
| `v3/tasks.ts` | Thin re-export layer over `shared.ts`: `task = createTask`, `schemaTask = createSchemaTask`, `toolTask = createToolTask`, `tasks = { … }`, plus hooks re-exported from `hooks.ts` (`onStart`, `onStartAttempt`, `onFailure`, `onSuccess`, `onComplete`, `onWait`, `onResume`, `onHandleError`, `onCatchError`, `middleware`, `onCancel`). |
| `v3/wait.ts` — `wait = { for, until, forToken }` | `wait.for(options: WaitForOptions)` — durable duration wait; `wait.until({ date, throwIfInThePast? })` — durable date wait; `wait.forToken` — manual waitpoint (returns a token that an external caller completes). Common options include `idempotencyKey` + `idempotencyKeyTTL`. Implementation note found in source: waits of ≤ 5000 ms (`DURATION_WAIT_CHARGE_THRESHOLD_MS`) are executed inline and count toward compute usage instead of creating a durable waitpoint. |
| `v3/waitUntil.ts` | `waitUntil(promise)` — attach a promise to the run's lifetime (does not itself create a waitpoint). |
| `v3/batch.ts` — `batch = { trigger, triggerAndWait, triggerByTask, triggerByTaskAndWait, retrieve }` | Batch API surface over `shared.ts` batch functions. |
| `v3/idempotencyKeys.ts` — `idempotencyKeys = { create, … }` | SDK-side creation of scoped keys (delegates to core). |
| `v3/retry.ts` — `retry = { fetch: retryFetch, … }` | Durable fetch with retry (`createAttributesFromHeaders`, `createFetchAttributes`, `createFetchResponseAttributes` for tracing). |
| `v3/queues.ts`, `v3/concurrencyLimits.ts` | Queue and named-concurrency-limit management API surface. |
| `v3/schedules/` — `schedules.task`, `schedules/api.ts` | `CreateScheduleOptions`, `UpdateScheduleOptions`, `ListScheduleOptions`, `ScheduledTaskPayload` — imperative schedule management. Declarative schedules are the task's `cron` property (see §6 and §7 `ScheduleType`: `DECLARATIVE` vs `IMPERATIVE`). |
| `v3/triggerClient.ts` — `TriggerClient` | Client configuration holder (`TriggerClientConfig`); the actual trigger calls live in `shared.ts`, not here. |
| `v3/runs.ts`, `v3/query.ts`, `v3/tags.ts`, `v3/metadata.ts`, `v3/timeout.ts`, `v3/usage.ts`, `v3/heartbeats.ts`, `v3/streams.ts`, `v3/sessions.ts`, `v3/webhooks.ts`, `v3/webhookWaiters.ts`, `v3/cache.ts`, `v3/locals.ts` | Runs API, run querying, tags, mutable run metadata, timeouts, usage, heartbeats, streams/sessions, webhook trigger helpers, webhook waiters (client side of the webhook-engine waiter mechanism). |
| `v3/ai.ts`, `v3/chat*.ts`, `v3/agentSkillsRuntime.ts`, `v3/prompts.ts`, `v3/skills.ts`, `v3/skill.ts` | AI-agent surface — **excluded** from UltraFlows core scope (see §10). Listed here only so a reader does not mistake their absence for an oversight. |

**REA support:** `trigger-sdk.json` — 102 parsed files, 0 parse failures,
796 application-graph nodes, 1630 edges (1105 `imports`, 204 `contains`,
316 `exposes`, 5 `calls`). Semantic graph capped at 30000 nodes (partial).

## 5. redis-worker — the Redis queue worker (`packages/redis-worker/src`)

**What it does:** a generic Redis-backed job/queue worker library used by
Trigger.dev's internal workers (including the run-engine's `RunQueue` jobs
and the schedule/webhook engines' jobs). It is **not** the task-execution
worker that runs user code; it is the durable job dispatcher underneath the
control plane. For UltraFlows this package is reference only: UltraFlows
uses a DB queue by design (see §10).

**Entry point:** `packages/redis-worker/src/index.ts` re-exports the
fair-queue, mollifier, queue, and worker modules.

**Key modules:**

| File | Role |
|---|---|
| `worker.ts` — `class Worker` | Generic worker loop (`runWorkerLoop`), job catalog (`WorkerCatalog`, `JobHandler`, `JobHandlerParams`), batching (`BatchConfig`, `flushTimedOutBatches`, `flushAllBatches`, `flushBatch`, `processBatch`, `processItem`), cron jobs inside the worker (`CronSchema`, `enqueueCronJob`, `rescheduleCronJob` with jitter), dead-letter redrive (`handleRedriveMessage`), graceful `shutdown`/`stop`, concurrency options (`WorkerConcurrencyOptions`). |
| `queue.ts` — `SimpleQueue` | The queue primitive: `enqueue`, `enqueueOnce` (deduplication), `requeue`, `dequeue(count)`, `ack(id, deduplicationKey?)`, `reschedule(id, availableAt)`, `cancel(cancellationKey)`, `size({ includeFuture })`, `oldestMessageAge`, `getJob`, `moveToDeadLetterQueue`, `sizeOfDeadLetterQueue`, `redriveFromDeadLetterQueue`. Visibility-timeout semantics (a dequeued-but-unacked message becomes visible again) are the lease mechanism. |
| `fair-queue/` | Fair dispatch across many queues: `workerQueue.ts` (`WorkerQueueManager`: `push`, `pushBatch`, `blockingPop`, `pop`, `getLength`, `peek`, `remove`, `clear`), `masterQueue.ts` (`MasterQueue`: per-shard queue sets scored by oldest-message timestamp — `addQueue`, `updateQueueScore`, `removeQueue`, `getQueuesFromShard`, `addQueueIfNotEmpty`, `removeQueueIfEmpty`), `concurrency.ts` (`ConcurrencyManager`), `visibility.ts` (`VisibilityManager`), `tenantDispatch.ts`, `telemetry.ts`, `keyProducer.ts`, `connection.ts`, `retry.ts`, `scheduler.ts`. Schedulers: `schedulers/drr.ts` (`DRRScheduler`, deficit round robin), `schedulers/roundRobin.ts` (`RoundRobinScheduler`), `schedulers/weighted.ts` (`WeightedScheduler`). |
| `mollifier/` | `buffer.ts`, `drainer.ts`, `schemas.ts` — batching/buffering layer that coalesces items before draining them to handlers (exact flush policy not established — see §9). |

**REA support:** `redis-worker.json` — 38 parsed files, 0 parse failures,
314 application-graph nodes, 543 edges (375 `imports`, 76 `contains`, 76
`exposes`, 3 `calls`, 13 `loads`). Semantic graph capped at 30000 nodes
(partial).

## 6. schedule-engine — cron and scheduled runs (`internal-packages/schedule-engine/src`)

**What it does:** turns schedule definitions into run triggers at the right
time, spread out so thousands of schedules do not fire in the same second.

**Entry point:** `internal-packages/schedule-engine/src/index.ts` exports
`ScheduleEngine` and `calculateNextNominalTimestamp`, plus schedule types.

**Key modules:**

| File | Role |
|---|---|
| `engine/index.ts` — `class ScheduleEngine` | `registerNextTaskScheduleInstance`, `triggerScheduledTask`, `enqueueScheduledTask` (private), `recoverSchedulesInEnvironment`, `#recoverTaskScheduleInstance`, `#handleTriggerScheduledTaskJob`, `getJob`, `quit`. Its worker catalog (`engine/workerCatalog.ts`, `scheduleWorkerCatalog`) carries the scheduled-trigger jobs. |
| `engine/scheduleCalculation.ts` | `calculateNextNominalTimestamp`, `calculateNextSchedulableOccurrence`, `previousScheduledTimestamp`, `nextScheduledTimestamps` — pure cron-occurrence math. |
| `engine/scheduleTiming.ts` | `resolveScheduleWindow`, `validateScheduleWindow`, `resolvePolicyMinimumMs`, `resolveScheduleWindowMs`, `calculateEffectiveScheduleTime`, `calculateSchedulePhase`. Constants: `SCHEDULE_PHASE_DENOMINATOR = 2_147_483_648`, `MAX_SCHEDULE_PHASE = denominator - 1`, `MINIMUM_SCHEDULE_RANGE_MS = 60_000`. |
| `engine/distributedScheduling.ts` | `calculateDistributedExecutionTime` — spreads a schedule's executions across its window using the schedule's phase (a stable per-schedule fraction), so cron stampedes are flattened. The engine options include `schedulePhaseSecret` and `cronSpreadFraction` (fraction of schedules with cron spread active). |
| `engine/types.ts` | Schedule-engine options and params (`RegisterScheduleInstanceParams`, `TriggerScheduleParams`). |

**Schedule data model (Prisma, §7):** `TaskSchedule` (`type`:
`DECLARATIVE` | `IMPERATIVE`, `taskIdentifier`, `deduplicationKey` +
`userProvidedDeduplicationKey`, `generatorExpression`, `generatorType`:
`CRON` (…), `timezone` default `UTC`, window fields
`windowDurationSeconds` / `windowPercentage` /
`defaultWindowDurationSeconds` / `minimumWindowDurationSeconds`,
`externalId`, `lastRunTriggeredAt`, `active`) and per-environment
`TaskScheduleInstance` (`schedulePhase`, `lastScheduledTimestamp`,
`nextScheduledTimestamp`, `active`).

**REA support:** `schedule-engine.json` (prior package-dir run) — 15 parsed
JS files, 0 parse failures, 115 application-graph nodes, 189 edges, 7341
semantic nodes, 8101 relations, 2755 unknowns, **not truncated**. The
module list above was confirmed by direct skim of `src/engine/*`.

## 7. Database model — the durable state (`internal-packages/database/prisma/schema.prisma`)

Read for model names and key fields only. The models that matter for a Rust
reimplementation:

| Model | Key fields (abridged) |
|---|---|
| `TaskRun` | `id` (cuid), `friendlyId` (unique), `engine` (`RunEngineVersion`, default `V1`), `status` (`TaskRunStatus`, default `PENDING`), `statusReason`, `idempotencyKey` + `idempotencyKeyExpiresAt` + `idempotencyKeyOptions` (Json), `debounce` (Json), `taskIdentifier`, `isTest`, `payload` (String) + `payloadType` (default `application/json`), `context`/`traceContext` (Json), `traceId`, `spanId`, `runtimeEnvironmentId`, `projectId`, `organizationId`, `queue`, `lockedQueueId`, `workerQueue` (default `"main"`, column `masterQueue`), `region`, `attemptNumber`, `runTags` (String[]), `taskVersion`/`sdkVersion`/`cliVersion`, `machinePreset`, `usageDurationMs`, `costInCents`, `baseCostInCents`, `lockedAt`/`lockedById`/`lockedToVersionId`, `priorityMs`, `concurrencyKey`, `gates` (Json), `delayUntil`, `queuedAt`, `ttl`, `expiredAt`, `maxAttempts`, `lockedRetryConfig` (Json), `oneTimeUseToken`, timestamps (`startedAt`, `executedAt`, `completedAt`). Relations: `attempts`, `checkpoints`, `snapshots` (via `TaskRunExecutionSnapshot.runId`). |
| `TaskRunStatus` (enum) | Non-final: `DELAYED`, `PENDING`, `PENDING_VERSION`, `WAITING_FOR_DEPLOY` (deprecated in favor of `PENDING_VERSION`), `DEQUEUED`, `EXECUTING`, `WAITING_TO_RESUME`, `RETRYING_AFTER_FAILURE`, `PAUSED`. Final: `CANCELED`, `INTERRUPTED`, `COMPLETED_SUCCESSFULLY`, `COMPLETED_WITH_ERRORS`, `SYSTEM_FAILURE`, `CRASHED`, `EXPIRED`, `TIMED_OUT`. |
| `TaskRunAttempt` | `number`, `friendlyId`, `taskRunId`, `backgroundWorkerId`, `backgroundWorkerTaskId`, `runtimeEnvironmentId`, `queueId`, `status` (`TaskRunAttemptStatus`: non-final `PENDING`/`EXECUTING`/`PAUSED`; final `FAILED`/`CANCELED`/`COMPLETED`), `startedAt`, `completedAt`, `usageDurationMs`, `error` (Json), `output` (String) + `outputType`. Relations to dependencies, batch items, checkpoint-restore events, child runs. |
| `TaskRunExecutionSnapshot` | `engine` (default `V2`), `executionStatus` (`TaskRunExecutionStatus`), `description`, `isValid`, `error`, `previousSnapshotId`, `runId`, `runStatus`, `batchId`, `attemptNumber`, `environmentId`/`environmentType`, `projectId`, `organizationId`, `completedWaitpoints` (relation) + `completedWaitpointOrder` (String[]), `checkpointId`, `workerId`, `runnerId`, `lastHeartbeatAt`, `metadata` (Json). |
| `TaskRunExecutionStatus` (enum) | `RUN_CREATED`, `DELAYED`, `QUEUED`, `QUEUED_EXECUTING` ("in the RunQueue, and also executing — happens when a run is continued [and] cannot reacquire concurrency"), `PENDING_EXECUTING`, `EXECUTING`, `EXECUTING_WITH_WAITPOINTS`, `SUSPENDED`, `PENDING_CANCEL`, `FINISHED`. |
| `TaskRunCheckpoint` / `TaskRunCheckpointType` | `friendlyId`, `type`, `location`, `imageRef`, `reason`, `metadata`, project + environment. |
| `Checkpoint` / `CheckpointType`, `CheckpointRestoreEvent` / `CheckpointRestoreEventType` | The checkpoint store proper (distinct from `TaskRunCheckpoint`); restore events track restore outcomes. |
| `Waitpoint` | `friendlyId`, `type` (`WaitpointType`: `RUN`, `DATETIME`, `MANUAL`, `BATCH`), `status` (`WaitpointStatus`: `PENDING`, `COMPLETED`), `completedAt`, `idempotencyKey` + `userProvidedIdempotencyKey` + `idempotencyKeyExpiresAt` + `inactiveIdempotencyKey`, `completedByTaskRunId`, `completedAfter`, `completedByBatchId`, `output` + `outputType` + `outputIsError`, project + environment, `tags`. |
| `TaskRunWaitpoint` | Edge row: `taskRunId`, `waitpointId`, `spanIdToComplete`, `batchId`, `batchIndex`. |
| `WaitpointTag` | Tags on waitpoints (used by the waiter/indexing paths). |
| `TaskQueue` | `friendlyId`, `name`, `type` (default `VIRTUAL`), `version`, `orderableName`, `concurrencyVersion`, `role` (`QUEUE`), project + environment, `concurrencyLimit` (+ override/base/percent fields), `totalConcurrencyLimit` (+ override fields), `rateLimit` (Json), `paused`, `archivedAt`. |
| `BatchTaskRun` / `BatchTaskRunStatus` | `friendlyId`, `idempotencyKey` + expiry, `status` (`PENDING`, `PROCESSING`, `COMPLETED`, `PARTIAL_FAILED`, `ABORTED`), `runIds` (String[]), `runCount`, `payload`, `options` (Json), `batchVersion` (default `v1`), `sealed` + `sealedAt`, `expectedCount`, `completedCount`, `completedAt`, `resumedAt`, processing counters, `successfulRunCount`/`failedRunCount`, `taskIdentifier`, `oneTimeUseToken`. |
| `BatchTaskRunItem` / `BatchTaskRunItemStatus` | Per-item: `batchTaskRunId`, `taskRunId`, `taskRunAttemptId` (nullable), `status` (default `PENDING`), `completedAt`. |
| `TaskSchedule` / `TaskScheduleInstance` | See §6. `ScheduleType`: `DECLARATIVE` (defined on the task via `cron`) vs `IMPERATIVE` (explicit SDK/dashboard calls). `ScheduleGeneratorType`: `CRON` (… — other values not recorded here; see schema lines 2613+). |
| `BackgroundWorker`, `BackgroundWorkerTask`, `BackgroundWorkerFile` | The deployed worker (deployment) and its tasks; `BackgroundWorkerTask.gates` (Json) is where task-level gates live. |
| `TaskRunDependency`, `TaskRunCounter`, `TaskRunNumberCounter`, `TaskEvent` | Parent/child run dependencies; per-run counters; the run event/timeline log (`TaskEventKind`, `TaskEventLevel`, `TaskEventStatus`). |
| `WebhookEndpoint`, `WebhookDelivery`, `RuntimeEnvironment`, `WorkerInstance`, `WorkerInstanceGroup`, `WorkerDeployment` | Webhook endpoints/deliveries, environments, worker instances/deployments — supporting cast, not core runtime state. |

Two-status-model note (important for reimplementation): Trigger.dev keeps
**two** status axes per run — the user-facing `TaskRunStatus` and the
engine-internal `TaskRunExecutionStatus` on snapshots. They are not the
same thing: a run can be `EXECUTING` on one axis while `QUEUED_EXECUTING`
or `EXECUTING_WITH_WAITPOINTS` on the other. Any Rust model must keep both
or justify merging them.

## 8. Cross-package flow (each claim cited)

### 8.1 trigger() → queued run

1. **SDK definition.** `task({...})` in `packages/trigger-sdk/src/v3/shared.ts`
   (`createTask`) normalizes queue/retry/machine/maxDuration/ttl and
   registers the task (object queues are registered; bare string queue names
   are references only). Source: `trigger-sdk/src/v3/shared.ts`,
   `trigger-sdk/src/v3/tasks.ts`.
2. **SDK trigger call.** `trigger(task, payload, options?)` →
   `trigger_internal("tasks.trigger()", …)` in `shared.ts`; the API client
   method is `triggerTask` in `packages/core/src/v3/apiClient/index.ts`.
   Trigger options carried on the wire (core schemas,
   `packages/core/src/v3/schemas/api.ts`): `queue { name }`, `delay`,
   `idempotencyKey` (≤ 2048 chars) + `idempotencyKeyTTL` +
   `idempotencyKeyOptions`, `ttl`, plus per-trigger queue/concurrency
   overrides. Source: `core/src/v3/schemas/api.ts` lines ~377–520.
3. **Engine trigger.** The control plane calls `RunEngine.trigger`
   (`internal-packages/run-engine/src/engine/index.ts`, line ~895). In
   order, it: handles debounce *before* creating a new run; asserts the
   environment exists; creates the run via the run store
   (`RunStore.createRun`, `internal-packages/run-store/src/types.ts`);
   builds/gets the run's associated waitpoint
   (`getOrCreateRunWaitpoint`, `buildRunAssociatedWaitpoint`); blocks the
   run on that waitpoint when applicable (`blockRunWithWaitpoint`,
   `blockRunWithWaitpointLockless`); schedules TTL expiry
   (`scheduleExpireRun`) and delayed-run enqueueing
   (`scheduleDelayedRunEnqueuing`) when `delayUntil`/`ttl` are set; and
   publishes the run to the queue (`publishRun` → `EnqueueSystem.publishRun`,
   `engine/systems/enqueueSystem.ts`). Re-enqueues never re-add TTL.
   Source: `run-engine/src/engine/index.ts` trigger body; `enqueueSystem.ts`.
4. **Queueing.** `RunQueue.enqueueMessage`
   (`run-engine/src/run-queue/index.ts`) puts the message on the
   environment/worker queue in Redis, subject to concurrency accounting
   (per-queue limit, per-concurrency-key limit, per-environment total limit
   + burst factor, gates). The run row's `workerQueue` defaults to `"main"`
   (column `masterQueue`); v2 worker queues are named by region/class/phase
   (core `runEngineWorker/workerQueue.ts`). Source: `run-queue/index.ts`,
   `core/src/v3/runEngineWorker/workerQueue.ts`.

### 8.2 Worker lease (dequeue → attempt)

1. A worker (or the engine on its behalf) calls
   `RunEngine.dequeueFromWorkerQueue(s)` /
   `dequeueFromEnvironmentWorkerQueue`
   (`run-engine/src/engine/index.ts` lines ~1593–1674) →
   `DequeueSystem.dequeueFromWorkerQueues`
   (`engine/systems/dequeueSystem.ts`).
2. The dequeue path locks the run, drops stale messages for finished runs,
   puts worker-mismatched messages back, falls back to the task's retry
   config when the trigger set no `maxAttempts`, and pre-generates a
   snapshot id. It then hands the worker a `DequeuedMessage`
   (core `schemas/runEngine.ts`) and moves the run to `PENDING_EXECUTING` /
   `EXECUTING` via `RunAttemptSystem.startRunAttempt`
   (`engine/systems/runAttemptSystem.ts`), which also records the attempt
   row (`TaskRunAttempt`) and locks the run to the worker
   (`RunStore.lockRunToWorker`).
3. The underlying lease is the Redis queue's visibility timeout:
   `SimpleQueue.dequeue` + `ack` in `packages/redis-worker/src/queue.ts`,
   with `VisibilityManager` (`fair-queue/visibility.ts`) and dead-letter
   handling (`moveToDeadLetterQueue`, `redriveFromDeadLetterQueue`). A
   dequeued-but-unacked message reappears — that is the crash-recovery
   backstop at the queue layer. Source: `redis-worker/src/queue.ts`,
   `redis-worker/src/fair-queue/visibility.ts`.
4. Heartbeats keep the lease honest while the run executes:
   `ExecutionSnapshotSystem.heartbeatRun` / `restartHeartbeatForRun` /
   `enqueueHeartbeatIfNeeded`, driven by the `heartbeatSnapshot` worker job
   (30 s visibility timeout). A missed heartbeat leaves a
   `PENDING_EXECUTING` snapshot for the stalled-run path (named in
   `dequeueSystem.ts` comments; internals not established — see §9).

### 8.3 Steps, subtasks, and waitpoints

- **Subtasks are runs.** Triggering a task from inside a task creates a
  child `TaskRun` with `parentTaskRunId` / `rootTaskRunId` set
  (`TriggerParams`, `TaskRun` relations via `TaskRunDependency`). The
  parent blocks on the child by creating a `RUN`-type waitpoint and a
  `TaskRunWaitpoint` edge: `triggerAndWait` / `batchTriggerAndWait` in
  `trigger-sdk/src/v3/shared.ts`; engine side `blockRunWithWaitpoint` +
  `getOrCreateRunWaitpoint` in `run-engine/src/engine/index.ts`.
- **Wait types.** `WaitpointType`: `RUN` (subtask/batch), `DATETIME`
  (`wait.until`, `createDateTimeWaitpoint`), `MANUAL` (`wait.forToken`,
  `createManualWaitpoint`), `BATCH` (batch completion). `WaitpointStatus`:
  only `PENDING` / `COMPLETED` — there is no separate "timed out" waitpoint
  status in the schema; expiry is handled by the run's TTL / waitpoint
  completion guards instead (exact timeout semantics: §9 UNKNOWN).
- **Blocking and resuming.** A blocked run sits in
  `EXECUTING_WITH_WAITPOINTS` (execution status) while its worker is free.
  Completion flows through `WaitpointSystem.completeWaitpoint` →
  `continueRunIfUnblocked` (under the run lock) →
  `continueRunExecution` (checkpoint system) → re-enqueue via
  `EnqueueSystem` (without re-adding TTL). Fan-out to many blocked runs
  goes through `WaitpointFanoutWorker` with a completion fingerprint and
  bounded retry policy (`fanoutPolicy.ts`).
- **Waitpoint completion payloads.** `Waitpoint.output` (+ `outputType`,
  `outputIsError`) carries the result; the store-coordinator path caps
  inline completions at 524,288 bytes and otherwise stores a reference
  (`WaitpointCompletionOutput = { inline } | { ref } | null`,
  `storeCoordinator.ts`). Completed-waitpoint ids are recorded on snapshots
  in order (`completedWaitpointOrder`), which is what lets a resumed run
  replay/skip already-completed waits — the durability mechanism behind
  "completed steps never re-execute" at the waitpoint layer. (Step-level
  memoization *inside* a single task execution, e.g. subtask result caching
  by idempotency key, is a separate mechanism: see §8.5.)
- **Short waits are not waitpoints.** `wait.for` / `wait.until` with a
  duration ≤ 5000 ms execute inline in the worker and count toward compute
  usage (`trigger-sdk/src/v3/wait.ts`, `DURATION_WAIT_CHARGE_THRESHOLD_MS`).
  Only longer waits become durable waitpoints.

### 8.4 Retries

- **Options and defaults.** `RetryOptions` (core `schemas/schemas.ts`):
  `maxAttempts`, `factor`, `minTimeoutInMs`, `maxTimeoutInMs`, `randomize`.
  Defaults (core `utils/retries.ts`): 3 / 2 / 1000 ms / 60000 ms / true.
  Delay: `min(maxTimeoutInMs, random * minTimeoutInMs * factor^(attempt-1))`,
  `random = Math.random() + 1` if randomized, and no further retry once
  `attempt >= maxAttempts` (attempts include the first execution).
- **Decision point.** On attempt completion,
  `retryOutcomeFromCompletion` (`run-engine/src/engine/retrying.ts`)
  returns `cancel_run` (cancelled-error code), `fail_run` (sanitized error;
  also when OOM has no larger machine left), or `retry` with
  `method: "queue" | "immediate"` and the computed settings. OOM retries
  escalate the machine preset when possible. `handleError` / `catchError`
  hooks on the task can modify the error or return new retry options
  (core `types/tasks.ts`).
- **Retry execution.** `RunAttemptSystem.attemptFailed` (under the run lock)
  applies the outcome; queue-method retries re-enter through the queue
  with the computed delay (the run lands in `RETRYING_AFTER_FAILURE` on the
  user-facing axis). The engine's own internal jobs have their own retry
  policy (e.g. `ensureRunFinalized`: up to 10,000 attempts, 1 s–300 s).
- **Fetch retries are separate.** `retry.fetch` (SDK) / `FetchRetryOptions`
  (core) retry HTTP calls by status (`429,408,409,5xx` → backoff with the
  same defaults), connection error, and timeout. Do not conflate with run
  retries.

### 8.5 Idempotency keys — scoping

- **Scopes.** `IdempotencyKeyScope = "run" | "attempt" | "global"`
  (core `idempotency-key-catalog/catalog.ts`; same union in
  `TriggerParams.idempotencyKeyOptions` and the API schemas). Default scope
  is `"run"` (core `idempotencyKeys.ts`).
- **What each scope means (from the names + the fields that carry them;
  exact dedup window semantics are §9 UNKNOWN):** `run` — the key dedups
  the whole run (re-triggering with the same key returns the existing run
  instead of creating a second one); `attempt` — the key dedups a single
  attempt (used for subtask result caching inside a retrying parent, the
  mechanism behind "completed steps never re-execute" per the docs recon);
  `global` — the key dedups across runs (the strongest scope; the SDK docs
  example in `core/src/v3/idempotencyKeys.ts` uses `{ scope: "global" }`).
- **Storage.** The key is stored on the run row (`TaskRun.idempotencyKey`,
  `idempotencyKeyExpiresAt`, `idempotencyKeyOptions` Json) and on waitpoints
  (`Waitpoint.idempotencyKey`, `userProvidedIdempotencyKey`,
  `idempotencyKeyExpiresAt`, `inactiveIdempotencyKey`). The run store
  exposes `findRunsByIdempotencyKeys` and `clearIdempotencyKey`. The
  waitpoint coordinator's Redis key is namespaced per environment:
  `idempotencyKey(environmentId, key)` (`waitpointCoordinator/keys.ts`).
  The SDK/API cap a user key at 2048 chars; a key TTL (`idempotencyKeyTTL`)
  can be set per trigger/wait. The often-quoted "30 days" default expiry
  from the docs recon was **not** found in the code paths read here — see
  §9.
- **In-process catalog.** Core keeps an in-memory catalog mapping a key
  hash back to the original key + scope so the scope is never silently
  dropped when a key crosses a process boundary
  (`idempotency-key-catalog/inMemoryIdempotencyKeyCatalog.ts`).

### 8.6 Schedules

1. A schedule is either **declarative** (the task's `cron` property,
   registered at deploy) or **imperative** (created via the SDK
   `schedules` API / dashboard), stored as `TaskSchedule` +
   per-environment `TaskScheduleInstance` (§6–§7).
2. `ScheduleEngine.registerNextTaskScheduleInstance` computes the next
   occurrence (`calculateNextNominalTimestamp` /
   `calculateNextSchedulableOccurrence`), applies the window policy
   (`resolveScheduleWindow`, minimum range 60 s) and the distribution phase
   (`calculateSchedulePhase`, `calculateDistributedExecutionTime` with
   `schedulePhaseSecret` + `cronSpreadFraction`), and stores
   `nextScheduledTimestamp` on the instance.
3. At the effective time, `triggerScheduledTask` /
   `enqueueScheduledTask` triggers the task through the normal trigger path
   (§8.1), stamping `lastRunTriggeredAt` / `lastScheduledTimestamp`, and
   registers the following instance. Recovery after an outage replays from
   `recoverSchedulesInEnvironment` / `#recoverTaskScheduleInstance`.
4. Timezone is stored per schedule (`timezone`, default `UTC`); the exact
   DST edge behavior was not established from code — see §9.

### 8.7 Batches, debounce, delay, TTL, gates, concurrency

- **Batches.** `batchTrigger*` creates a `BatchTaskRun` (sealed at
  `expectedCount` items) plus one `TaskRun` + `BatchTaskRunItem` per item.
  The parent (if any) blocks on a `BATCH` waitpoint. Completion is tracked
  by `BatchCompletionTracker` / `BatchSystem` (`expectedCount` vs
  `completedCount`, `PARTIAL_FAILED` when some items fail); the parent is
  resumed by `#resumeBatchParent`. Batch status values:
  `PENDING`, `PROCESSING`, `COMPLETED`, `PARTIAL_FAILED`, `ABORTED`.
- **Debounce.** Handled in `RunEngine.trigger` before run creation
  (`DebounceSystem`, `TaskRun.debounce` Json, `RunStore.rewriteDebouncedRun`):
  a re-trigger inside the debounce window rewrites the pending run instead
  of creating a new one. Exact window/key semantics: §9.
- **Delay.** `delayUntil` / `delay` (string duration or date) parks the run
  in `DELAYED` (both status axes) until `DelayedRunSystem` /
  `scheduleDelayedRunEnqueuing` enqueues it. Reschedule is a first-class
  op (`RunEngine.rescheduleDelayedRun`, `RunStore.rescheduleRun`).
- **TTL.** `ttl` (string duration or non-negative int) is enforced by
  `TtlSystem` / `scheduleExpireRun` / `expireTtlRun`; an unexecuted run past
  TTL becomes `EXPIRED` (final). TTL counts from first enqueue only — it
  is deliberately not re-added on re-enqueue (§8.1 step 3). The docs
  recon's "TTL counts from enqueue when delay is set" matches this code
  shape but the delay+TTL interaction arithmetic was not verified line by
  line — see §9.
- **Gates.** A run may carry up to 4 gates (`QueueGate`, max 4: 3 requested
  + the task's anonymous inline-limit gate; `ParsedGate { queue,
  concurrencyKey? }`): the run must hold a concurrency slot in each gated
  queue while executing. Gates live on `TaskRun.gates` (Json) and
  `BackgroundWorkerTask.gates` (Json); malformed gates are dropped, never
  fatal (`gateParsing.ts`).
- **Concurrency.** Three levels, all visible on `RunQueue` and
  `RunEngine`: per-queue limit, per-concurrency-key limit
  (`concurrencyKey` on the run; `concurrencyKeyBreakdown` /
  `concurrencyKeyLiveStats`), per-environment total limit with burst
  factor (`getEnvConcurrencyLimitWithBurstFactor`). Named shared limits
  (one limit across tasks) exist as `concurrencyLimits` in the SDK and as
  `TaskQueue` rows; the fair-queue schedulers (DRR / round-robin /
  weighted) decide *which* queue's message is dequeued next when many
  queues are backlogged. Exact fairness math: §9.

### 8.8 Webhooks (control-plane ingress)

`WebhookEngine` (`internal-packages/webhook-engine/src/engine/index.ts`):
`ingest` (endpoint path: verify → filter → route → queue delivery),
`ingestWaiter` (waiter path: a run waiting for a matching webhook),
`createWaiter` / `cancelWaiter` / `listWaiters`, `verifyGetHandshake`
(provider handshake), `replayDelivery`, `simulateInject`.
Supporting machinery: `filter/` (parse + evaluate event filters),
`verification/` (HMAC, asymmetric, shared-secret, URL-secret verifiers),
`signing/`, `waiters/store.ts` (`WebhookWaiterStore`: front-gate claim,
session-part claim/commit/release, reserve, limits per environment /
endpoint / shape), `waiters/match.ts` (shape + values hashing of the
match object), `waiters/scripts.ts` (Redis Lua: `FRONT_GATE`, `RESERVE`,
`ENV_RESERVE`, `ENV_CONFIRM`, `REGISTER`, `CANCEL`, `CLAIM`, `ACK`, …),
`partitions.ts` (`bootstrapPartitions`), `sessionKey.ts`, `jobQueue.ts`,
`memoryGuard.ts`, `targets.ts`. The SDK side is `webhooks.ts` /
`webhookWaiters.ts` (trigger-sdk) and `WebhookEndpoint` / `WebhookDelivery`
rows (Prisma). REA support: `webhook-engine.json` — 40 parsed files, 0 parse
failures, 316 application-graph nodes, 591 edges (408 `imports`, 80
`contains`, 102 `exposes`, 1 `calls`); semantic graph capped at 30000 nodes
(partial).

## 9. UNKNOWN — explicitly not established

Do not implement these from this document without further evidence:

1. **Crash-mid-step recovery internals.** What exactly happens when a
   worker dies mid-attempt: the queue visibility timeout re-exposes the
   message and a `PENDING_EXECUTING` snapshot is left for a "stalled"
   system (both named in code comments), but the stalled-run detector's
   timing, ownership transfer, and exactly-once/at-least-once guarantees
   were not read. The checkpoint-restore path (`restore` worker-queue
   phase, `CheckpointRestoreEvent`, `reportRestoreOutcome`) is named but
   its step-level resume semantics (which steps replay, which are skipped)
   were not established. The docs recon's "self-hosted Trigger lacks
   checkpoints" claim vs. the extensive checkpoint code here is unresolved:
   this tree evidently *has* checkpoint machinery; whether it is cloud-only
   or gated could not be determined from these files.
2. **Exact dispatch ordering under contention.** The fair schedulers (DRR /
   round-robin / weighted) and `fairQueueSelectionStrategy` /
   `weightedWorkerQueueOrder` exist, but their scoring formulas, quantum
   sizes, and starvation behavior were not read. `priorityMs` exists on
   runs but its ordering effect was not traced.
3. **Idempotency dedup windows.** Default expiry ("30 days" in the docs
   recon) was not found in the code read; neither was the exact dedup
   lookup (which store/table is consulted on trigger, and what is returned
   on a hit — the existing run, its output, or an error). `attempt`-scope
   subtask result caching is inferred from scope names + the docs recon,
   not from a traced code path.
4. **Debounce key/window semantics.** Which fields form the debounce key,
   and whether the window is sliding or fixed, were not read.
5. **Delay + TTL interaction arithmetic.** TTL is not re-added on
   re-enqueue (verified), but whether the TTL clock starts at trigger time
   or at first-enqueue time when a delay is set was not verified line by
   line.
6. **Waitpoint timeout semantics.** `WaitpointStatus` has no timeout state;
   how a `DATETIME`/`MANUAL` waitpoint that never completes is expired, and
   what error (if any) the blocked run receives, was not established.
   (`WaitpointTimeoutError` exists in core `waitpoints/`, but its raising
   path was not traced.)
7. **Racepoints / race simulation.** `registerRacepointForRun` and
   `raceSimulationSystem.ts` are named but their semantics were not read.
8. **Mollifier flush policy.** When buffered items are drained (size vs.
   time thresholds) was not read.
9. **Schedule DST behavior.** Timezone is stored per schedule, but DST
   edge handling in `scheduleCalculation.ts` was not verified.
10. **Worker-side execution protocol.** How the dequeued run's code is
    actually invoked (supervisor HTTP protocol, `runEngineWorker/supervisor/`
    and `workload/` schemas, `runtime/manager.ts`) and how checkpoint images
    are captured/restored was not traced. The `restore` phase naming
    suggests CRIU-style restore, but that is an inference, not a finding.
11. **Cross-package REA edges.** REA resolves imports only inside one
    inventoried container, so no REA edge in this map proves a link between
    two packages; §8's cross-package claims rest on matching names across
    directly-read files.
12. **API route handlers.** The HTTP layer that maps API calls to
    `RunEngine.trigger` (webapp/API app, excluded from this map's package
    list) was not read; §8.1 step 2→3 is a name-level join (SDK
    `triggerTask` → engine `trigger`), not a traced request path.
13. **Webhook-engine REA depth.** The webhook evidence file exists and is
    summarized in §0, but its semantic graph is capped at 30000 nodes
    (partial), like the other large packages; §8.8's behavioral claims rest
    on direct reading, not on REA relations.

## 10. EXCLUDED — deliberately out of the UltraFlows core map

- **Webapp / dashboard** (`apps/`): UI, run-timeline views, realtime
  subscriptions UI. Not runtime behavior.
- **CLI / deploy / build pipeline** (`packages/cli-v3`, `packages/build`,
  `apps/` deploy flows): task registration at deploy time is relevant only
  insofar as it produces `BackgroundWorker` / `BackgroundWorkerTask` rows
  (§7); the build itself is out of scope.
- **Billing** (`internal-packages/billing`, `billingCache.ts` except as
  noted, cost fields on runs): cost accounting is not runtime semantics.
- **AI agents / chat** (`trigger-sdk/src/v3/ai*.ts`, `chat*.ts`,
  `prompts.ts`, `skills.ts`, `agentSkillsRuntime.ts`,
  `internal-packages/dashboard-agent*`): agent orchestration on top of the
  runtime, not the runtime.
- **ClickHouse** (`internal-packages/clickhouse`, `metrics-pipeline`):
  analytics/event export, not the execution path.
- **Redis-as-queue** (`packages/redis-worker` as an implementation choice,
  and the Redis parts of `run-engine/src/run-queue`,
  `waitpointCoordinator`, `run-store` snapshot storage): documented here
  because Trigger.dev uses them, but **UltraFlows uses a DB queue by
  design** — the Rust reimplementation must not copy the Redis topology.
  What transfers is the *semantics*: visibility-timeout leases,
  dead-letter + redrive, fair per-queue dispatch, per-run locking,
  snapshot + waitpoint-order resume. What does not transfer: Redis key
  layouts, Lua scripts, shard/score structures, and the master-queue
  fan-out index.
- **Observability stack** (`internal-packages/observability-map`,
  `tracing`, OpenTelemetry wiring): tracing is instrumentation, not
  semantics, except where span/trace ids are stored on runs (§7).
- **Everything not listed in §1–§8** (emails, SSO/RBAC, replication,
  LLM model catalog, Python SDK, React hooks, Slack/Linear plugins):
  adjacent product surface, not core runtime.
