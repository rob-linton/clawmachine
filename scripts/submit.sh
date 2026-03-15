#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"

PROMPT="${1:?Usage: submit.sh \"your prompt\" [model]}"
MODEL="${2:-}"
ID=$(uuidgen | tr '[:upper:]' '[:lower:]')

JOB_JSON=$(jq -cn \
  --arg id "$ID" \
  --arg prompt "$PROMPT" \
  --arg model "$MODEL" \
  '{id: $id, prompt: $prompt} + (if $model != "" then {model: $model} else {} end)')

rcli RPUSH claw:queue:pending "$JOB_JSON" > /dev/null
echo "Submitted job: $ID"
echo "  prompt: ${PROMPT:0:80}"
[ -n "$MODEL" ] && echo "  model: $MODEL"
echo ""
echo "Check result: ./scripts/result.sh $ID"
