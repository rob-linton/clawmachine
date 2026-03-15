#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"

# Phase 0 smoke test
# Prerequisites: Redis running, claw-prototype worker running

PASS=0
FAIL=0

red() { printf '\033[0;31m%s\033[0m\n' "$1"; }
green() { printf '\033[0;32m%s\033[0m\n' "$1"; }
yellow() { printf '\033[0;33m%s\033[0m\n' "$1"; }

check() {
  local desc="$1"
  local expected="$2"
  local actual="$3"

  if echo "$actual" | grep -qi "$expected"; then
    green "  PASS: $desc"
    PASS=$((PASS + 1))
  else
    red "  FAIL: $desc"
    red "    expected to contain: $expected"
    red "    actual: $actual"
    FAIL=$((FAIL + 1))
  fi
}

wait_for_job() {
  local job_id="$1"
  local timeout="${2:-120}"
  local elapsed=0

  while [ $elapsed -lt $timeout ]; do
    STATUS=$(rcli GET "claw:job:$job_id:status" 2>/dev/null)
    if [ "$STATUS" = "completed" ] || [ "$STATUS" = "failed" ]; then
      echo "$STATUS"
      return 0
    fi
    sleep 3
    elapsed=$((elapsed + 3))
    printf '.'
  done
  echo "timeout"
  return 1
}

echo "========================================="
echo "  ClaudeCodeClaw Phase 0 Smoke Test"
echo "========================================="
echo ""

# Check prerequisites
echo "Checking prerequisites..."
rcli ping > /dev/null 2>&1 || { red "Redis not running"; exit 1; }
green "  Redis: OK (DB $REDIS_DB)"

echo ""

# Test 1: Submit and complete a simple job
echo "Test 1: Simple math question"
ID=$(uuidgen | tr '[:upper:]' '[:lower:]')
rcli RPUSH claw:queue:pending "{\"id\":\"$ID\",\"prompt\":\"What is 2+2? Reply with just the number.\"}" > /dev/null

printf "  Waiting for completion"
FINAL_STATUS=$(wait_for_job "$ID" 120)
echo ""

check "Job completed" "completed" "$FINAL_STATUS"

if [ "$FINAL_STATUS" = "completed" ]; then
  RESULT=$(rcli GET "claw:job:$ID:result" 2>/dev/null)
  check "Result contains answer" "4" "$RESULT"
  check "Result is valid JSON" "job_id" "$RESULT"

  COST=$(echo "$RESULT" | jq -r '.cost_usd // 0' 2>/dev/null || echo "0")
  DURATION=$(echo "$RESULT" | jq -r '.duration_ms // 0' 2>/dev/null || echo "0")
  yellow "  Cost: \$$COST | Duration: ${DURATION}ms"
fi

# Test 2: Submit a second job to verify the loop continues
echo ""
echo "Test 2: Worker continues after first job"
ID2=$(uuidgen | tr '[:upper:]' '[:lower:]')
rcli RPUSH claw:queue:pending "{\"id\":\"$ID2\",\"prompt\":\"Say hello in one word.\"}" > /dev/null

printf "  Waiting for completion"
FINAL_STATUS2=$(wait_for_job "$ID2" 120)
echo ""

check "Second job completed" "completed" "$FINAL_STATUS2"

if [ "$FINAL_STATUS2" = "completed" ]; then
  RESULT2=$(rcli GET "claw:job:$ID2:result" 2>/dev/null)
  check "Second result is valid JSON" "job_id" "$RESULT2"
fi

# Test 3: Verify status tracking
echo ""
echo "Test 3: Status keys exist"
STATUS_VAL=$(rcli GET "claw:job:$ID:status" 2>/dev/null)
check "Status key is 'completed'" "completed" "$STATUS_VAL"

# Summary
echo ""
echo "========================================="
if [ $FAIL -eq 0 ]; then
  green "  ALL TESTS PASSED ($PASS/$PASS)"
else
  red "  $FAIL FAILED, $PASS PASSED"
fi
echo "========================================="

exit $FAIL
