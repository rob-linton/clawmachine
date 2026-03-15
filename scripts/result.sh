#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"

ID="${1:?Usage: result.sh <job_id>}"

STATUS=$(rcli GET "claw:job:$ID:status" 2>/dev/null)

if [ -z "$STATUS" ] || [ "$STATUS" = "(nil)" ]; then
  echo "Job $ID not found"
  exit 1
fi

echo "Job:    $ID"
echo "Status: $STATUS"

if [ "$STATUS" = "failed" ]; then
  ERROR=$(rcli GET "claw:job:$ID:error" 2>/dev/null)
  echo "Error:  $ERROR"
  exit 1
fi

if [ "$STATUS" = "completed" ]; then
  echo ""
  RESULT=$(rcli GET "claw:job:$ID:result" 2>/dev/null)
  echo "$RESULT" | jq -r '.result // .' 2>/dev/null || echo "$RESULT"
fi
