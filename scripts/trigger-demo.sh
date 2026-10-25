#!/usr/bin/env bash
# Trigger-core end-to-end demo: an EXTERNAL app (ultraflows-demo-app)
# registers tasks with the server, triggers runs, and works them as a
# remote worker. Proofs:
#   1. external registration
#   2. trigger (4 tasks)
#   3. idempotent re-trigger (same key => same run id)
#   4. retry (flaky task completes on attempt 3)
#   5. timer wait (timer task completes after its sleep)
#   6. triggerAndWait (parent completes with the child's receipt)
#   7. approval wait + kill -9 + restart + resume WITHOUT re-executing
#      recorded steps (prepare recorded exactly once)
#   8. schedule firing (every-minute cron produces a scheduled run)
#   9. batch trigger (3 runs, all complete)
#
# Usage: scripts/trigger-demo.sh  (from the repo root)
# Requires: cargo, curl, python3. Binds 127.0.0.1 only.
set -u
cd "$(dirname "$0")/.."

export PATH="$HOME/.cargo/bin:$PATH"
PORT="${PORT:-3101}"
BASE="http://127.0.0.1:$PORT"
DB="/tmp/ultraflows-trigger-demo.db"
SERVER_LOG="/tmp/ultraflows-trigger-demo-server.log"
WORKER_LOG="/tmp/ultraflows-trigger-demo-worker.log"
PASS=0
FAIL=0

ok()   { PASS=$((PASS+1)); echo "PASS: $1"; }
bad()  { FAIL=$((FAIL+1)); echo "FAIL: $1"; }
check(){ if [ "$2" = "$3" ]; then ok "$1"; else bad "$1 (got $2, want $3)"; fi; }

jget() { # jget <url> <python-expr on parsed json `v`>
  python3 -c "import json,sys,urllib.request; v=json.load(urllib.request.urlopen('$1')); print($2)"
}
wait_status() { # wait_status <run-id> <STATUS> <timeout-secs>
  local id="$1" want="$2" timeout="$3" i=0 got=""
  while [ "$i" -lt "$timeout" ]; do
    got=$(jget "$BASE/v1/runs/$id" "v['status']" 2>/dev/null || echo "?")
    [ "$got" = "$want" ] && { echo "$got"; return 0; }
    sleep 1; i=$((i+1))
  done
  echo "$got"; return 1
}

cleanup() {
  kill "$WORKER_PID" 2>/dev/null
  kill "$SERVER_PID" 2>/dev/null
  wait "$WORKER_PID" 2>/dev/null; wait "$SERVER_PID" 2>/dev/null
}
trap cleanup EXIT

rm -f "$DB"
cargo build --bins 2>&1 | tail -1

ULTRAFLOW_DB="$DB" PORT="$PORT" ./target/debug/ultraflows-server >"$SERVER_LOG" 2>&1 &
SERVER_PID=$!
for i in $(seq 1 30); do
  curl -sf "$BASE/health" >/dev/null 2>&1 && break
  sleep 1
done
curl -sf "$BASE/health" >/dev/null 2>&1 || { echo "FAIL: server did not start"; exit 1; }

ULTRAFLOWS_URL="$BASE" ./target/debug/ultraflows-demo-app worker >"$WORKER_LOG" 2>&1 &
WORKER_PID=$!
sleep 2
grep -q "registered" "$WORKER_LOG" && ok "1. external registration (worker registered tasks)" \
  || bad "1. external registration"

TRIG=$(ULTRAFLOWS_URL="$BASE" ./target/debug/ultraflows-demo-app trigger-all)
APPROVAL=$(echo "$TRIG" | python3 -c "import json,sys; print(json.load(sys.stdin)['approval_run_id'])")
FLAKY=$(echo "$TRIG" | python3 -c "import json,sys; print(json.load(sys.stdin)['flaky_run_id'])")
TIMER=$(echo "$TRIG" | python3 -c "import json,sys; print(json.load(sys.stdin)['timer_run_id'])")
PARENT=$(echo "$TRIG" | python3 -c "import json,sys; print(json.load(sys.stdin)['parent_run_id'])")
IDEM1=$(echo "$TRIG" | python3 -c "import json,sys; print(json.load(sys.stdin)['idem_run_id_1'])")
IDEM2=$(echo "$TRIG" | python3 -c "import json,sys; print(json.load(sys.stdin)['idem_run_id_2'])")
[ -n "$APPROVAL" ] && [ -n "$FLAKY" ] && [ -n "$TIMER" ] && [ -n "$PARENT" ] \
  && ok "2. trigger (approval, flaky, timer, parent runs created)" \
  || bad "2. trigger"
check "3. idempotent re-trigger (same key => same run id)" "$IDEM1" "$IDEM2"

# Approval run must reach Waiting before the kill test.
wait_status "$APPROVAL" "WAITING" 30 >/dev/null \
  && ok "7a. approval run reached Waiting" || bad "7a. approval run reached Waiting"
PRE_BEFORE=$(jget "$BASE/v1/runs/$APPROVAL/steps" "sum(1 for s in v if s['key']=='prepare')")
check "7b. prepare recorded once before kill" "$PRE_BEFORE" "1"

kill -9 "$SERVER_PID" 2>/dev/null
wait "$SERVER_PID" 2>/dev/null
ULTRAFLOW_DB="$DB" PORT="$PORT" ./target/debug/ultraflows-server >"$SERVER_LOG" 2>&1 &
SERVER_PID=$!
for i in $(seq 1 30); do curl -sf "$BASE/health" >/dev/null 2>&1 && break; sleep 1; done
STILL_WAITING=$(jget "$BASE/v1/runs/$APPROVAL" "v['status']")
check "7c. approval run still Waiting after kill -9 + restart" "$STILL_WAITING" "WAITING"
PRE_AFTER=$(jget "$BASE/v1/runs/$APPROVAL/steps" "sum(1 for s in v if s['key']=='prepare')")
check "7d. prepare still recorded exactly once after restart" "$PRE_AFTER" "1"

curl -sf -X POST "$BASE/v1/waitpoints/demo-approval-token/complete" \
  -H 'Content-Type: application/json' -d '{"output":{"approved":true}}' >/dev/null
wait_status "$APPROVAL" "COMPLETED" 30 >/dev/null \
  && ok "7e. approval run completed after waitpoint completion" \
  || bad "7e. approval run completed after waitpoint completion"
PRE_FINAL=$(jget "$BASE/v1/runs/$APPROVAL/steps" "sum(1 for s in v if s['key']=='prepare')")
AFTER_FINAL=$(jget "$BASE/v1/runs/$APPROVAL/steps" "sum(1 for s in v if s['key']=='after-approval')")
check "7f. prepare NOT re-executed (still exactly once)" "$PRE_FINAL" "1"
check "7g. after-approval step recorded once" "$AFTER_FINAL" "1"

FLAKY_STATUS=$(wait_status "$FLAKY" "COMPLETED" 60) \
  && ok "4. retry (flaky run completed)" || bad "4. retry (flaky run completed, status $FLAKY_STATUS)"
FLAKY_ATTEMPT=$(jget "$BASE/v1/runs/$FLAKY" "v['attempt']")
check "4b. flaky run completed on attempt 3" "$FLAKY_ATTEMPT" "3"

TIMER_STATUS=$(wait_status "$TIMER" "COMPLETED" 30) \
  && ok "5. timer wait (timer run completed after sleep)" \
  || bad "5. timer wait (status $TIMER_STATUS)"

PARENT_STATUS=$(wait_status "$PARENT" "COMPLETED" 60) \
  && ok "6. triggerAndWait (parent completed)" || bad "6. triggerAndWait (status $PARENT_STATUS)"
PARENT_OUT=$(jget "$BASE/v1/runs/$PARENT" "v['output']['child']['receipt']")
check "6b. parent output carries child receipt" "$PARENT_OUT" "receipt-42"

# Schedule: every-minute cron; wait up to 100 s for a scheduled run.
SCHED_FIRED=""
for i in $(seq 1 100); do
  SCHED_FIRED=$(jget "$BASE/v1/runs?task=tick-task" \
    "sum(1 for r in v if r.get('schedule_id') and r['status']=='COMPLETED')" 2>/dev/null || echo 0)
  [ "${SCHED_FIRED:-0}" -ge 1 ] && break
  sleep 1
done
[ "${SCHED_FIRED:-0}" -ge 1 ] && ok "8. schedule firing (scheduled tick-task run completed)" \
  || bad "8. schedule firing (no completed scheduled run in 100 s)"

BATCH=$(curl -sf -X POST "$BASE/v1/tasks/tick-task/batch-trigger" \
  -H 'Content-Type: application/json' \
  -d '{"items":[{"payload":{}},{"payload":{}},{"payload":{}}]}')
BATCH_N=$(echo "$BATCH" | python3 -c "import json,sys; print(len(json.load(sys.stdin)))")
check "9a. batch trigger returned 3 runs" "$BATCH_N" "3"
BATCH_OK=1
for rid in $(echo "$BATCH" | python3 -c "import json,sys; [print(r['run_id']) for r in json.load(sys.stdin)]"); do
  wait_status "$rid" "COMPLETED" 30 >/dev/null || BATCH_OK=0
done
[ "$BATCH_OK" = 1 ] && ok "9b. batch runs all completed" || bad "9b. batch runs all completed"

echo "----"
echo "PASS=$PASS FAIL=$FAIL"
[ "$FAIL" = 0 ]
