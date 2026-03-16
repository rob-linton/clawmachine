#!/usr/bin/env bash
# Validate a skill directory against the Agent Skills specification.
# Usage: validate-skill.sh <skill-directory>

set -euo pipefail

SKILL_DIR="${1:-.}"
SKILL_MD="$SKILL_DIR/SKILL.md"

errors=0

echo "Validating skill: $SKILL_DIR"
echo "================================"

# Check SKILL.md exists
if [ ! -f "$SKILL_MD" ]; then
    echo "ERROR: SKILL.md not found in $SKILL_DIR"
    exit 1
fi

# Extract frontmatter (only between first two --- lines)
FRONTMATTER=$(awk '/^---$/{n++; next} n==1{print} n>=2{exit}' "$SKILL_MD")

# Check name field exists (first match only)
NAME=$(echo "$FRONTMATTER" | grep -m1 '^name:' | sed 's/name: *//' | tr -d '"' | tr -d "'")
if [ -z "$NAME" ]; then
    echo "ERROR: Missing required 'name' field in frontmatter"
    errors=$((errors + 1))
else
    echo "  name: $NAME"

    # Validate name format
    if [[ ! "$NAME" =~ ^[a-z0-9]([a-z0-9-]*[a-z0-9])?$ ]]; then
        echo "ERROR: name must be lowercase alphanumeric + hyphens, no leading/trailing hyphens"
        errors=$((errors + 1))
    fi

    if [[ "$NAME" == *"--"* ]]; then
        echo "ERROR: name must not contain consecutive hyphens"
        errors=$((errors + 1))
    fi

    if [ ${#NAME} -gt 64 ]; then
        echo "ERROR: name must be <= 64 characters (got ${#NAME})"
        errors=$((errors + 1))
    fi

    # Check name matches directory
    DIR_NAME=$(basename "$SKILL_DIR")
    if [ "$NAME" != "$DIR_NAME" ]; then
        echo "WARNING: name '$NAME' does not match directory name '$DIR_NAME'"
    fi
fi

# Check description field exists
DESC=$(echo "$FRONTMATTER" | grep -m1 '^description:' | sed 's/description: *//')
if [ -z "$DESC" ]; then
    echo "ERROR: Missing required 'description' field in frontmatter"
    errors=$((errors + 1))
else
    DESC_LEN=${#DESC}
    echo "  description: ${DESC:0:80}..."
    if [ $DESC_LEN -gt 1024 ]; then
        echo "ERROR: description must be <= 1024 characters (got $DESC_LEN)"
        errors=$((errors + 1))
    fi
fi

# Check body length
BODY_LINES=$(sed '1,/^---$/d' "$SKILL_MD" | sed '1,/^---$/d' | wc -l | tr -d ' ')
echo "  body: $BODY_LINES lines"
if [ "$BODY_LINES" -gt 500 ]; then
    echo "WARNING: SKILL.md body exceeds 500 lines ($BODY_LINES). Consider moving content to references/"
fi

# Check optional directories
for dir in scripts references assets; do
    if [ -d "$SKILL_DIR/$dir" ]; then
        FILE_COUNT=$(find "$SKILL_DIR/$dir" -type f | wc -l | tr -d ' ')
        echo "  $dir/: $FILE_COUNT files"
    fi
done

# Check scripts are executable
if [ -d "$SKILL_DIR/scripts" ]; then
    for script in "$SKILL_DIR/scripts/"*; do
        [ -f "$script" ] || continue
        if [ ! -x "$script" ]; then
            echo "WARNING: $script is not executable (chmod +x)"
        fi
    done
fi

echo "================================"
if [ $errors -eq 0 ]; then
    echo "VALID: Skill passes all checks"
    exit 0
else
    echo "INVALID: $errors error(s) found"
    exit 1
fi
