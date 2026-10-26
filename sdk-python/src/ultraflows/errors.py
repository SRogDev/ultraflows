"""Exception hierarchy for the UltraFlows Python SDK.

Mirrors the Rust SDK's ``SdkError`` variants, in Python idiom:

- ``Suspended`` — internal control flow, NOT an error: a task function
  raised it (via ``RemoteCtx``) because it reported a wait. ``Worker``
  catches it and does nothing more; the run is ``WAITING`` on the
  server. Never catch it as a failure in your own task code, and never
  let it escape a worker loop unhandled — ``Worker.run_once`` handles it.
- ``RunFailed`` — the task itself failed (or a child run ended
  FAILED/CANCELLED/EXPIRED). ``Worker`` reports it to the server, which
  retries per the task's retry policy.
- ``ApiError`` — the server answered with a non-2xx status.
- ``TransportError`` — no HTTP answer at all (connection, timeout,
  or ``httpx`` missing when no fallback transport is available).
"""


class UltraflowsError(Exception):
    """Base class for all UltraFlows SDK errors."""


class ApiError(UltraflowsError):
    """The server answered, but with a non-2xx HTTP status."""

    def __init__(self, status: int, body: str = "") -> None:
        self.status = status
        self.body = body
        super().__init__(f"server error {status}: {body}")


class TransportError(UltraflowsError):
    """The HTTP request could not be completed at all."""


class RunFailed(UltraflowsError):
    """A task (or a child run it waited on) failed."""


class Suspended(UltraflowsError):
    """Internal: the current attempt suspended on a wait.

    Raised by ``RemoteCtx.sleep`` / ``wait_until`` / ``wait_for_token``
    / ``trigger_and_wait`` after the wait has been reported to the
    server. It is not a failure and must not be reported as one.
    """

    def __init__(self, key: str) -> None:
        self.key = key
        super().__init__(f"suspended on wait {key}")
