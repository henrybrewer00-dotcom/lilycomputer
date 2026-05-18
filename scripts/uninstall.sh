#!/usr/bin/env bash
set -euo pipefail

UID_NUM="$(id -u)"
PLIST="$HOME/Library/LaunchAgents/computer.lily.daemon.plist"

launchctl bootout "gui/$UID_NUM/computer.lily.daemon" 2>/dev/null || true
rm -f "$PLIST"
rm -f "$HOME/.local/bin/lily" "$HOME/.local/bin/lilyd"
echo "▸ removed lily binaries and LaunchAgent"
echo "  (left ~/.lily/env and /Users/Shared/lily/token in place — delete manually if you want)"
