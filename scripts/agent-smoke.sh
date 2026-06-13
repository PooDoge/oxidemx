#!/usr/bin/env bash
# Agent ↔ AI-backend smoke tests.
#
# Drives the REAL agent runtime headlessly via `oxidemx-overlay
# --agent-selftest` against the live provider, plus the conductor's
# flow engine, and asserts each tool/flow actually works end-to-end —
# the model must RECEIVE the tool's result, not just call it.
#
# Catches the two regressions that slipped past manual testing:
#   * max_turns too low → tools silently never called
#   * plain-text tool results delivered to the model as null
#
# Usage:  scripts/agent-smoke.sh [--flows-only] [--tools-only]
# Needs:  a Gemini key (env GEMINI_API_KEY or ~/.config/oxidemx/gemini.key),
#         and the installed binaries on PATH (or set $OVERLAY / $CONDUCTOR).
# Exit:   0 = all pass, 1 = any failure (CI-friendly).

set -uo pipefail

OVERLAY="${OVERLAY:-oxidemx-overlay}"
CONDUCTOR="${CONDUCTOR:-oxidemx-conductor}"
FLOWS_DIR="${OXIDEMX_FLOWS_DIR:-$HOME/.config/oxidemx/flows}"
AGENTS_DIR="${OXIDEMX_AGENTS_DIR:-$HOME/.config/oxidemx/agents}"
TIMEOUT="${AGENT_TEST_TIMEOUT:-120}"
export OXIDEMX_FLOWS_DIR="$FLOWS_DIR" OXIDEMX_AGENTS_DIR="$AGENTS_DIR" OXIDEMX_AGENT_DEBUG=1

PASS=0; FAIL=0; FAILED_NAMES=()
green() { printf '\033[32m%s\033[0m' "$1"; }
red()   { printf '\033[31m%s\033[0m' "$1"; }
ok()    { PASS=$((PASS+1)); echo "  $(green PASS)  $1"; }
bad()   { FAIL=$((FAIL+1)); FAILED_NAMES+=("$2"); echo "  $(red FAIL)  $1"; [ -n "${3:-}" ] && echo "        ↳ $3"; }

# ── Tool tests ───────────────────────────────────────────────────────
# run_tool <name> <prompt> <must-call-tool> <reply-regex>
# Asserts: the tool was REQUESTED, did NOT fail, its result was non-null,
# and the model's reply matches <reply-regex> (proving the result
# reached the model — the null-delivery guard).
run_tool() {
  local name="$1" prompt="$2" tool="$3" rx="$4"
  local log; log="$(mktemp)"
  timeout "$TIMEOUT" "$OVERLAY" --agent-selftest "$prompt" >"$log" 2>&1
  local reply; reply="$(sed -n '/--- reply ---/,$p' "$log")"

  if ! grep -q "ToolCallRequested.*tool_name: \"$tool\"" "$log"; then
    bad "$name — tool '$tool' never called" "$name" "$(grep -oE 'tool_name: \"[a-z_]+\"' "$log" | sort -u | tr '\n' ' ')"
    rm -f "$log"; return
  fi
  if grep -q "ToolCallFailed.*tool_name: \"$tool\"" "$log"; then
    bad "$name — tool '$tool' FAILED" "$name" "$(grep "ToolCallFailed" "$log" | grep "$tool" | head -1 | cut -c1-160)"
    rm -f "$log"; return
  fi
  # Null-delivery guard: the completed result must not be empty/null.
  if grep "ToolCallCompleted.*tool_name: \"$tool\"" "$log" | grep -qE 'result: String\("?\s*"?\)|"content": *null'; then
    bad "$name — tool '$tool' returned NULL/empty to the model" "$name"
    rm -f "$log"; return
  fi
  if echo "$reply" | grep -qiE "$rx"; then
    ok "$name"
  else
    bad "$name — reply missing expected evidence /$rx/" "$name" "reply: $(echo "$reply" | tr '\n' ' ' | cut -c1-160)"
  fi
  rm -f "$log"
}

# ── Flow tests (deterministic, no API) ───────────────────────────────
flow_validate() {
  local id="$1"
  if "$CONDUCTOR" validate "$id" >/dev/null 2>&1; then ok "validate $id"
  else bad "validate $id" "validate-$id" "$("$CONDUCTOR" validate "$id" 2>&1 | head -2 | tr '\n' ' ')"; fi
}
flow_mock_run() {
  local id="$1" wd; wd="$(mktemp -d)"
  local extra=("${@:2}")
  if "$CONDUCTOR" run "$id" --mock --workdir "$wd" "${extra[@]}" >/dev/null 2>&1 \
     && [ -f "$wd/run.json" ] && grep -q '"success": true' "$wd/run.json"; then
    ok "mock-run $id"
  else
    bad "mock-run $id" "run-$id" "no success run.json"
  fi
  rm -rf "$wd"
}

echo "════ OxideMX agent smoke tests ════"

if [ "${1:-}" != "--tools-only" ]; then
  echo "── Flow engine (deterministic) ──"
  for f in $("$CONDUCTOR" list 2>/dev/null); do flow_validate "$f"; done
  flow_mock_run research-digest --input url=https://example.com
  flow_mock_run doc-digest --input path=/tmp/x.md
  flow_mock_run system-doctor
fi

if [ "${1:-}" != "--flows-only" ]; then
  echo "── Tools (live backend; needs a Gemini key) ──"
  run_tool "read_file"       "Use read_file to read $FLOWS_DIR/research-digest/flow.md and report its flow id." \
                             "read_file"       "research-digest"
  run_tool "list_dir"        "Use list_dir on $HOME/.local/share/oxidemx/runs and report how many entries." \
                             "list_dir"        "[0-9]+"
  run_tool "execute_command" "Run 'df -h /' with execute_command and quote the output." \
                             "execute_command" "Filesystem|composefs|[0-9]+%"
  run_tool "google_search"   "Use google_search for the latest stable Rust version and state it." \
                             "google_search"   "rust|1\.[0-9]+"
  run_tool "get_menu_config" "Use get_menu_config and tell me how many radial menu pages I have." \
                             "get_menu_config" "page|[0-9]+"
  run_tool "list_system_apps" "Use list_system_apps and name one installed app." \
                             "list_system_apps" "[A-Za-z]"
  run_tool "memory"          "Save the memory 'agent smoke test marker' then search memory for 'smoke' and confirm." \
                             "memory"          "smoke|saved|remember"
  run_tool "run_flow"        "Use run_flow to run system-doctor in mock mode and report it finished." \
                             "run_flow"        "system-doctor|finished|complete|steps"
fi

echo "════ ${PASS} passed, ${FAIL} failed ════"
[ "$FAIL" -gt 0 ] && { echo "failed: ${FAILED_NAMES[*]}"; exit 1; }
exit 0
