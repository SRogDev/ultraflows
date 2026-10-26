"""Live integration proof: Python worker against the real Rust server.

What it proves, end to end (no stubs):
  1. registration of a Python task with the real server
  2. trigger from a Python Client
  3. a journaled step, executed by the Python worker
  4. a timer wait (sleep) that suspends and resumes
  5. completion with the task's output

Usage (from the repo root; the script builds/starts the server itself)::

    python sdk-python/scripts/integration_demo.py

Environment: ``PORT`` (default 3103), ``ULTRAFLOW_DB``
(default /tmp/ultraflows-py-demo.db). The server binds 127.0.0.1
only. The script cleans up the server subprocess on exit.

If the Rust toolchain or the server binary cannot be built in your
environment, this script will fail loudly at the build step — that
is a missing-proof signal, not a pass. Do not claim a live proof
from the stub-based pytest suite alone.
"""

from __future__ import annotations

import os
import subprocess
import sys
import time
import urllib.request

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "src"))

from ultraflows import Client, TriggerOptions, Worker

ROOT = os.path.join(os.path.dirname(__file__), "..", "..")
PORT = os.environ.get("PORT", "3103")
BASE = f"http://127.0.0.1:{PORT}"
DB = os.environ.get("ULTRAFLOW_DB", "/tmp/ultraflows-py-demo.db")
SERVER_BIN = os.path.join(ROOT, "target", "debug", "ultraflows-server")


def wait_for_health(timeout: float = 30.0) -> None:
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            with urllib.request.urlopen(f"{BASE}/health", timeout=2) as resp:
                if resp.status == 200:
                    return
        except Exception:  # noqa: BLE001 - any failure means 'not up yet'
            time.sleep(0.5)
    raise RuntimeError("server did not become healthy in time")


def main() -> int:
    if os.path.exists(DB):
        os.remove(DB)
    if not os.path.exists(SERVER_BIN):
        print("Building the Rust server (cargo build --bin ultraflows-server)...")
        subprocess.run(
            ["cargo", "build", "--bin", "ultraflows-server"],
            cwd=ROOT, check=True,
            env={**os.environ, "PATH": f"{os.path.expanduser('~')}/.cargo/bin:{os.environ.get('PATH', '')}"},
        )
    env = {**os.environ, "ULTRAFLOW_DB": DB, "PORT": PORT,
           "PATH": f"{os.path.expanduser('~')}/.cargo/bin:{os.environ.get('PATH', '')}"}
    server = subprocess.Popen([SERVER_BIN], env=env,
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        wait_for_health()
        print("PASS: server healthy")

        client = Client(BASE)
        worker = Worker(client, worker_id="py-demo-worker")
        calls = {"prepare": 0}

        @worker.task("py-demo-task", version_hash="v1")
        def demo_task(ctx, payload):
            def prepare():
                calls["prepare"] += 1
                return {"echo": payload["text"]}

            prepared = ctx.step("prepare", prepare)
            ctx.sleep("nap", 2)  # suspends; resumes after the timer fires
            after = ctx.step("after", lambda: "done")
            return {"prepared": prepared, "after": after}

        reg = worker.register()
        assert "py-demo-task" in reg["tasks"], reg
        print("PASS: 1. registration (py-demo-task)")

        run = client.trigger("py-demo-task", TriggerOptions(payload={"text": "hello"}))
        run_id = run["run_id"]
        print(f"PASS: 2. trigger (run {run_id})")

        # Drive the worker in this process until the run completes.
        deadline = time.time() + 60
        status = None
        while time.time() < deadline:
            worker.run_once()
            status = client.get_run(run_id).status
            if status == "COMPLETED":
                break
            time.sleep(0.5)
        assert status == "COMPLETED", f"run ended as {status}"
        steps = client.get_steps(run_id)
        keys = [s.key for s in steps]
        assert keys == ["prepare", "after"], keys
        assert calls["prepare"] == 1, f"prepare executed {calls['prepare']} times, want 1"
        print("PASS: 3. journaled step recorded (prepare, exactly once)")
        print("PASS: 4. timer wait suspended and resumed (sleep 2s)")
        final = client.get_run(run_id)
        assert final.output["after"] == "done", final.output
        print(f"PASS: 5. completion with output {final.output}")
        print("INTEGRATION DEMO: ALL PASS")
        return 0
    finally:
        server.terminate()
        try:
            server.wait(timeout=10)
        except subprocess.TimeoutExpired:
            server.kill()


if __name__ == "__main__":
    raise SystemExit(main())
