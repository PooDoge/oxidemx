#!/usr/bin/env bash
# agentd-smoke.sh — automated, GUI-free smoke test of the truthful-runs backend.
#
# Drives the RUNNING agentd over D-Bus (org.oxidemx.Agent) exactly as a connector
# would, exercising the RunLauncher path end-to-end: launch a flow, poll its real
# status to a terminal state, prove an unknown run_id is reported honestly, and
# list runs. No overlay/GUI required.
#
# Usage:  scripts/agentd-smoke.sh [project_dir] [flow_id]
# Requires: a live agentd owning org.oxidemx.Agent, and a valid flow on disk.
set -uo pipefail

PROJECT="${1:-$HOME}"
FLOW="${2:-self-test-hello-world}"
AG=(busctl --user call org.oxidemx.Agent /org/oxidemx/Agent org.oxidemx.Agent)
pass=0; fail=0
ok()   { echo "  PASS: $1"; pass=$((pass+1)); }
bad()  { echo "  FAIL: $1"; fail=$((fail+1)); }

echo "== agentd truthful-runs smoke test (project=$PROJECT flow=$FLOW) =="

# 1. RunFlow → expect a run-N id (the launcher's format, not <flow>-<timestamp>)
rid=$("${AG[@]}" RunFlow sss "$PROJECT" "$FLOW" "{}" 2>&1 | sed -E 's/^s "//; s/"$//')
echo "RunFlow → '$rid'"
[[ "$rid" =~ ^run-[0-9]+$ ]] && ok "launch returns a real run-N id" \
                             || bad "expected run-N, got '$rid'"

# 2. Poll RunStatus to a terminal state (running → finished/failed)
status=""
for _ in $(seq 1 20); do
  status=$("${AG[@]}" RunStatus s "$rid" 2>&1 | sed -E 's/^s "//; s/"$//')
  [[ "$status" == "finished" || "$status" == "failed" || "$status" == "cancelled" ]] && break
  sleep 1
done
echo "RunStatus($rid) → '$status'"
[[ "$status" =~ ^(running|finished|failed|cancelled)$ ]] && ok "status is a real ground-truth value" \
                                                         || bad "status not a known value: '$status'"
[[ "$status" == "finished" ]] && ok "flow completed successfully" \
                              || echo "  NOTE: flow ended '$status' (status reporting still correct)"

# 3. RunStatus on a bogus id → honest 'unknown', never a fabricated status
unknown=$("${AG[@]}" RunStatus s "run-999999" 2>&1)
echo "RunStatus(run-999999) → '$unknown'"
echo "$unknown" | grep -qiE "unknown run_id|not found" && ok "unknown run reported honestly (no confabulation)" \
                                                       || bad "unknown run not honest: '$unknown'"

# 4. ListRuns → the run is listed
listed=$("${AG[@]}" ListRuns s "$PROJECT" 2>&1)
echo "ListRuns → '$listed'"
echo "$listed" | grep -q "$rid" && ok "run appears in ListRuns" \
                                || bad "run '$rid' missing from ListRuns"

echo "== $pass passed, $fail failed =="
exit $(( fail > 0 ? 1 : 0 ))
