#!/bin/bash
# Test whether Claude Code re-reads CLAUDE.md on --continue
# This determines whether we can use dynamic CLAUDE.md injection
# or need to fall back to a CONTEXT.md file approach.
set -e

WORKDIR=$(mktemp -d)
echo "Working directory: $WORKDIR"
cd "$WORKDIR"

# Phase 1: Establish baseline with secret word ALPHA
echo "# Test Workspace" > CLAUDE.md
echo "" >> CLAUDE.md
echo "The secret word is ALPHA. If asked, respond with just the secret word." >> CLAUDE.md

echo "--- Phase 1: Asking for secret word (should be ALPHA) ---"
RESP1=$(claude -p "What is the secret word defined in CLAUDE.md? Reply with ONLY the word, nothing else." --output-format text 2>/dev/null)
echo "Response 1: $RESP1"

# Phase 2: Change CLAUDE.md and ask again with --continue
echo "# Test Workspace" > CLAUDE.md
echo "" >> CLAUDE.md
echo "The secret word is BRAVO. If asked, respond with just the secret word." >> CLAUDE.md

echo "--- Phase 2: Changed CLAUDE.md, asking with --continue (testing re-read) ---"
RESP2=$(claude -p "The CLAUDE.md file has been updated. What is the secret word in CLAUDE.md NOW? Reply with ONLY the current word, nothing else." --continue --output-format text 2>/dev/null)
echo "Response 2: $RESP2"

# Phase 3: Change again to rule out caching
echo "# Test Workspace" > CLAUDE.md
echo "" >> CLAUDE.md
echo "The secret word is CHARLIE. If asked, respond with just the secret word." >> CLAUDE.md

echo "--- Phase 3: Changed again, asking with --continue ---"
RESP3=$(claude -p "CLAUDE.md was updated again. What secret word does it contain RIGHT NOW? Reply with ONLY the word." --continue --output-format text 2>/dev/null)
echo "Response 3: $RESP3"

echo ""
echo "=== ANALYSIS ==="
REREAD=true
if echo "$RESP2" | grep -qi "BRAVO"; then
    echo "Phase 2: PASS - saw BRAVO (CLAUDE.md was re-read)"
else
    echo "Phase 2: FAIL - did not see BRAVO"
    REREAD=false
fi

if echo "$RESP3" | grep -qi "CHARLIE"; then
    echo "Phase 3: PASS - saw CHARLIE (CLAUDE.md was re-read)"
else
    echo "Phase 3: FAIL - did not see CHARLIE"
    REREAD=false
fi

echo ""
if [ "$REREAD" = true ]; then
    echo "RESULT: CLAUDE.md IS re-read on --continue. Dynamic CLAUDE.md injection will work."
else
    echo "RESULT: CLAUDE.md is NOT reliably re-read on --continue."
    echo "FALLBACK: Use .notebook/CONTEXT.md with a permanent CLAUDE.md instruction to read it."
fi

# Cleanup
rm -rf "$WORKDIR"
