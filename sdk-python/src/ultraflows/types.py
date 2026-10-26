"""Wire types for the UltraFlows server protocol.

IMPORTANT — the wire format is the server's own, not Trigger.dev's
hosted API, and not camelCase:

- JSON field names are snake_case (``delay_ms``, ``idempotency_key``,
  ``concurrency_key``), exactly as the Rust SDK sends them.
- Run statuses are ``SCREAMING_SNAKE_CASE`` strings (``"COMPLETED"``,
  ``"WAITING"``, ``"FAILED"``, ``"PENDING_VERSION"``, ...). The Rust
  SDK once broke by comparing ``"Completed"`` — this SDK only ever
  compares the screaming form; see ``STATUS_*`` below.
- Rust ``std::time::Duration`` serializes as ``{"secs": N, "nanos": N}``,
  so ``RetryPolicy.to_dict()`` emits that shape.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

# Run statuses, exactly as the server serializes them (SCREAMING_SNAKE_CASE).
STATUS_PENDING = "PENDING"
STATUS_PENDING_VERSION = "PENDING_VERSION"
STATUS_RUNNING = "RUNNING"
STATUS_WAITING = "WAITING"
STATUS_COMPLETED = "COMPLETED"
STATUS_FAILED = "FAILED"
STATUS_CANCELLED = "CANCELLED"
STATUS_EXPIRED = "EXPIRED"

TERMINAL_STATUSES = frozenset(
    {STATUS_COMPLETED, STATUS_FAILED, STATUS_CANCELLED, STATUS_EXPIRED}
)

# Idempotency scopes accepted by the server.
SCOPE_RUN = "run"
SCOPE_ATTEMPT = "attempt"
SCOPE_GLOBAL = "global"


def _omit_none(mapping: dict[str, Any]) -> dict[str, Any]:
    return {k: v for k, v in mapping.items() if v is not None}


@dataclass
class RetryPolicy:
    """Retry policy for a task, mirroring ``ultraflows_core::RetryPolicy``.

    Defaults match the server's: 5 attempts, 500 ms initial delay,
    factor 2.0, 8 s cap, no jitter.
    """

    max_attempts: int = 5
    initial_delay_ms: int = 500
    backoff_factor: float = 2.0
    max_delay_ms: int = 8000
    randomize: bool = False

    def to_dict(self) -> dict[str, Any]:
        def duration(ms: int) -> dict[str, int]:
            return {"secs": ms // 1000, "nanos": (ms % 1000) * 1_000_000}

        return {
            "max_attempts": self.max_attempts,
            "initial_delay": duration(self.initial_delay_ms),
            "backoff_factor": self.backoff_factor,
            "max_delay": duration(self.max_delay_ms),
            "randomize": self.randomize,
        }


@dataclass
class TaskDef:
    """A task definition, as sent to ``POST /v1/workers/register``."""

    id: str
    version_hash: str
    retry: RetryPolicy | None = None
    concurrency_total: int | None = None
    concurrency_per_key: int | None = None
    queue: str | None = None

    def to_dict(self) -> dict[str, Any]:
        return _omit_none(
            {
                "id": self.id,
                "version_hash": self.version_hash,
                "retry": self.retry.to_dict() if self.retry else None,
                "concurrency_total": self.concurrency_total,
                "concurrency_per_key": self.concurrency_per_key,
                "queue": self.queue,
            }
        )


@dataclass
class TriggerOptions:
    """Options for ``POST /v1/tasks/{id}/trigger`` (snake_case wire)."""

    payload: Any = None
    delay_ms: int | None = None
    ttl_ms: int | None = None
    idempotency_key: str | None = None
    idempotency_scope: str | None = None  # "run" | "attempt" | "global"
    idempotency_ttl_ms: int | None = None
    concurrency_key: str | None = None
    max_attempts: int | None = None
    tags: list[str] | None = None
    metadata: Any = None

    def to_dict(self) -> dict[str, Any]:
        return _omit_none(
            {
                "payload": self.payload,
                "delay_ms": self.delay_ms,
                "ttl_ms": self.ttl_ms,
                "idempotency_key": self.idempotency_key,
                "idempotency_scope": self.idempotency_scope,
                "idempotency_ttl_ms": self.idempotency_ttl_ms,
                "concurrency_key": self.concurrency_key,
                "max_attempts": self.max_attempts,
                "tags": self.tags,
                "metadata": self.metadata,
            }
        )


@dataclass
class RunView:
    """A run as the server returns it. ``status`` is SCREAMING_SNAKE_CASE."""

    id: str
    workflow: str
    payload: Any
    output: Any | None
    error: str | None
    status: str
    attempt: int
    max_attempts: int
    wait_key: str | None = None
    wait_kind: str | None = None
    wait_token: str | None = None

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> RunView:
        return cls(
            id=str(data["id"]),
            workflow=data["workflow"],
            payload=data.get("payload"),
            output=data.get("output"),
            error=data.get("error"),
            status=data["status"],
            attempt=int(data.get("attempt", 0)),
            max_attempts=int(data.get("max_attempts", 0)),
            wait_key=data.get("wait_key"),
            wait_kind=data.get("wait_kind"),
            wait_token=data.get("wait_token"),
        )

    @property
    def is_terminal(self) -> bool:
        return self.status in TERMINAL_STATUSES


@dataclass
class StepView:
    """A journaled step result (``run_id`` + ``key`` is unique forever)."""

    run_id: str
    key: str
    result: Any

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> StepView:
        return cls(
            run_id=str(data["run_id"]), key=data["key"], result=data.get("result")
        )


@dataclass
class LeasedRun:
    """What ``POST /v1/workers/{id}/lease`` returns: the run plus its
    journaled steps so far — the replay input for ``RemoteCtx``."""

    run: RunView
    steps: list[StepView] = field(default_factory=list)

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> LeasedRun:
        return cls(
            run=RunView.from_dict(data["run"]),
            steps=[StepView.from_dict(s) for s in data.get("steps", [])],
        )
