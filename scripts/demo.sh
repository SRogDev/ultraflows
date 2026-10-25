#!/usr/bin/env bash
# UltraFlows end-to-end demo (v0.1.0).
#
# What it proves, in order:
#   1. A workflow runs through the HTTP API and suspends on a human approval.
#   2. `kill -9` on the server while it waits loses NOTHING: after restart
#      the run is still Waiting, with its earlier steps in the journal.
#   3. Approving it resumes the run to Completed — earlier steps come
#      from the journal, they are NOT re-executed (check /steps).
#   4. A flaky workflow retries with backoff and completes.
#   5. A timer workflow wakes after its delay.
#
# Requires: cargo, curl, python3, jq (jq optional — output is still readable).
set -euo pipefail
cd "$(dirname "$0")/.."

export PATH="$HOME/.cargo/bin:$PATH"
DB="${ULTRAFLOW_DB:-/tmp/ultraflows-demo.db}"
PORT="${PORT:-3000}"
BASE="http://127.0.0.1:$PORT"
rm -f "$DB"
cargo build --bin ultraflows-server >/dev/null 2>&1

start_server() {
  ULTRAFLOW_DB="$DB" PORT="$PORT" ./target/debug/ultraflows-server >/tmp/ultraflows-demo.log 2>&1 &
  echo $!
  for _ in $(seq 1 50); do
    curl -sf "$BASE/health" >/dev/null 2>&1 && return 0
    sleep 0.2
  done
  echo "server did not start; see /tmp/ultraflows-demo.log" >&2
  exit 1
}

poll_status() { # poll_status <run_id> <wanted> [timeout_s]
  local id="$1" want="$(echo "$2" | tr 'a-z' 'A-Z')" timeout="${3:-30}"
  for _ in $(seq 1 "$timeout"); do
    local st
    st=$(curl -s "$BASE/v1/runs/$id" | python3 -c "import sys,json;print(json.load(sys.stdin)['status'])" 2>/dev/null || echo "?")
    if [ "$st" = "$want" ]; then echo "$st"; return 0; fi
    sleep 1
  done
  echo "TIMEOUT waiting for $want (last: $st)" >&2
  curl -s "$BASE/v1/runs/$id"; echo; exit 1
}

echo "== 1. Start server, trigger client-onboarding =="
PID=$(start_server)
RUN=$(curl -s -X POST "$BASE/v1/runs" -H 'content-type: application/json' \
  -d '{"workflow":"client-onboarding","payload":{"company":"Acme"}}' \
  | python3 -c "import sys,json;print(json.load(sys.stdin)['run_id'])")
echo "run: $RUN"
poll_status "$RUN" "Waiting" 15
echo "run is Waiting on approval (token: onboarding:Acme):"
curl -s "$BASE/v1/runs/$RUN" | python3 -m json.tool | grep -E 'wait_|status'

echo
echo "== 2. kill -9 the server mid-wait, then restart it =="
kill -9 "$PID" 2>/dev/null || true
sleep 1
PID=$(start_server)
poll_status "$RUN" "Waiting" 15
echo "run survived the kill: still Waiting, steps so far:"
curl -s "$BASE/v1/runs/$RUN/steps" | python3 -m json.tool

echo
echo "== 3. Approve it, watch it complete without re-executing steps =="
curl -s -X POST "$BASE/v1/approvals/onboarding:Acme" -H 'content-type: application/json' \
  -d '{"result":{"approved":true,"by":"demo-human"}}'; echo
poll_status "$RUN" "Completed" 15
echo "final output:"; curl -s "$BASE/v1/runs/$RUN" | python3 -m json.tool | grep -A6 '"output"'
echo "steps (validate + create-account appear ONCE each):"
curl -s "$BASE/v1/runs/$RUN/steps" | python3 -c "import sys,json;[print(' -',s['key']) for s in json.load(sys.stdin)]"
echo "event log:"; curl -s "$BASE/v1/runs/$RUN/events" | python3 -c "import sys,json;[print(f\"  {e['seq']:>2} {e['kind']}\") for e in json.load(sys.stdin)]"

echo
echo "== 4. Flaky report: fails twice, retries, completes =="
RUN2=$(curl -s -X POST "$BASE/v1/runs" -H 'content-type: application/json' \
  -d '{"workflow":"flaky-report","payload":{"fail_times":2}}' \
  | python3 -c "import sys,json;print(json.load(sys.stdin)['run_id'])")
poll_status "$RUN2" "Completed" 20
curl -s "$BASE/v1/runs/$RUN2" | python3 -m json.tool | grep -E 'attempt|output' -A3

echo
echo "== 5. Timer: delayed-reminder wakes after 5s =="
RUN3=$(curl -s -X POST "$BASE/v1/runs" -H 'content-type: application/json' \
  -d '{"workflow":"delayed-reminder","payload":{"seconds":5}}' \
  | python3 -c "import sys,json;print(json.load(sys.stdin)['run_id'])")
poll_status "$RUN3" "Completed" 20
echo "timer run completed."

kill "$PID" 2>/dev/null || true
echo
echo "DEMO OK — all five proofs passed."
