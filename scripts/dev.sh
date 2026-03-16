#!/bin/bash
# Deprecated: use startup.sh instead
echo "Note: dev.sh is now merged into startup.sh"
exec "$(dirname "$0")/startup.sh" "$@"
