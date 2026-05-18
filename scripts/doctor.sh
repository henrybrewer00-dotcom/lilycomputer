#!/usr/bin/env bash
# Lily Computer doctor — diagnose & repair common problems.
# Run this AS THE WORKER USER (rhettbrewer).
set -uo pipefail

WHO="$(id -un)"
UID_NUM="$(id -u)"
GREEN='\033[0;32m'; RED='\033[0;31m'; YEL='\033[1;33m'; DIM='\033[2m'; CLR='\033[0m'

ok()   { printf "  ${GREEN}✓${CLR} %s\n" "$*"; }
bad()  { printf "  ${RED}✗${CLR} %s\n" "$*"; }
warn() { printf "  ${YEL}!${CLR} %s\n" "$*"; }
hdr()  { printf "\n${DIM}▸ %s${CLR}\n" "$*"; }

echo "Lily Computer doctor — running as $WHO (uid=$UID_NUM)"

# 1) Daemon binary present
hdr "binary"
BIN="$HOME/.local/bin/lilyd"
if [[ -x "$BIN" ]]; then
  ok "$BIN exists"
else
  bad "$BIN missing — run ./scripts/install.sh daemon"
  exit 1
fi

# 2) Code signature
hdr "code signature"
if codesign -dv "$BIN" 2>&1 | grep -q "Signature="; then
  ok "lilyd is signed (TCC grants will persist across rebuilds with the same hash)"
else
  warn "lilyd is unsigned — TCC grants may be brittle. Run ./scripts/install.sh daemon to ad-hoc sign."
fi

# 3) LaunchAgent loaded
hdr "LaunchAgent"
if launchctl print "gui/$UID_NUM/computer.lily.daemon" >/dev/null 2>&1; then
  ok "LaunchAgent loaded"
else
  bad "LaunchAgent not loaded; bootstrapping now"
  PLIST="$HOME/Library/LaunchAgents/computer.lily.daemon.plist"
  [[ -f "$PLIST" ]] && launchctl bootstrap "gui/$UID_NUM" "$PLIST" || bad "no plist at $PLIST — install first"
fi

# 4) Kickstart (picks up any new TCC permission grants)
hdr "restarting daemon (so new permission grants take effect)"
launchctl kickstart -k "gui/$UID_NUM/computer.lily.daemon" 2>&1 && ok "kickstarted"
sleep 1

# 5) Health
hdr "health"
if [[ -r /Users/Shared/lily/token ]]; then
  TOKEN="$(cat /Users/Shared/lily/token)"
  HEALTH="$(curl -sS -m 3 -H "Authorization: Bearer $TOKEN" http://127.0.0.1:7777/health 2>/dev/null || true)"
  if [[ -n "$HEALTH" && "$HEALTH" == *'"ok":true'* ]]; then
    ok "$HEALTH"
  else
    bad "daemon not responding on http://127.0.0.1:7777"
    bad "  recent stderr below; look for 'panic', 'GROQ_API_KEY', 'address already in use'"
  fi
else
  bad "/Users/Shared/lily/token missing — install hasn't run yet"
fi

# 6) Groq key
hdr "groq key"
if [[ -r "$HOME/.lily/env" ]] && grep -q '^GROQ_API_KEY=[^[:space:]]' "$HOME/.lily/env"; then
  ok "key found in ~/.lily/env"
elif [[ -r /Users/Shared/lily/env ]] && grep -q '^GROQ_API_KEY=[^[:space:]]' /Users/Shared/lily/env; then
  ok "key found in /Users/Shared/lily/env (shared)"
else
  bad "GROQ_API_KEY not set anywhere"
fi

# 7) Ask the daemon to test ITS OWN permissions (this is what actually matters —
#    perms granted to Terminal don't apply to lilyd, and vice-versa).
hdr "permissions (tested from the daemon's TCC context)"
DIAG=""
if [[ -n "${TOKEN:-}" ]]; then
  DIAG="$(curl -sS -m 5 -H "Authorization: Bearer $TOKEN" http://127.0.0.1:7777/diagnose 2>/dev/null || true)"
fi

screen_ok=false
auto_ok=false
acc_ok=false
ext_ok=false
if [[ -n "$DIAG" ]]; then
  echo "  raw: $DIAG"
  [[ "$DIAG" == *'"screen_recording"'*'"ok":true'* ]] && screen_ok=true
  [[ "$DIAG" == *'"automation_system_events"'*'"ok":true'* ]] && auto_ok=true
  [[ "$DIAG" == *'"accessibility"'*'"ok":true'* ]] && acc_ok=true
  [[ "$DIAG" == *'"browser_extension"'*'"connected":true'* ]] && ext_ok=true
else
  bad "daemon did not respond to /diagnose — is it running?"
fi

if $ext_ok; then
  ok "Chrome extension — connected over WebSocket"
else
  warn "Chrome extension — not connected (browser_* tools will fail)"
  warn "Install: chrome://extensions → Developer mode → Load unpacked → pick the extension/ folder"
  warn "Then click the puzzle-piece icon, open the Lily Computer options, confirm green dot."
fi

if $screen_ok; then
  ok "Screen Recording — daemon can screencapture"
else
  bad "Screen Recording FAILED for lilyd"
  warn "Note: this is EXPECTED on a backgrounded Fast-User-Switched session — Tahoe blocks all display capture for non-foreground users, even with the permission granted. The AX-tree tools (read_ui, get_text, click_element) work without it."
  warn "If you DO want screenshot to work, foreground this user and try again, or:"
  warn "Privacy → Screen Recording. Add ~/.local/bin/lilyd, toggle ON, kickstart daemon."
  open "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
fi

if $auto_ok; then
  ok "Automation → System Events — daemon can talk to apps"
else
  bad "Automation FAILED for osascript (causes -1743 errors)"
  warn "Run in rhettbrewer's terminal:  ~/.local/bin/lilyd warmup"
  warn "Click 'Allow' on the Automation popup. Then kickstart, re-run doctor."
fi

if $acc_ok; then
  ok "Accessibility — daemon can send keystrokes / click_element"
else
  bad "Accessibility FAILED for osascript (causes -1002 / 'not allowed assistive access')"
  warn "macOS attributes click/keystroke to osascript itself, not lilyd."
  warn "Fix #1: run  ~/.local/bin/lilyd warmup  in rhettbrewer's terminal — should pop a"
  warn "         dialog 'osascript wants to control your computer' → click Allow."
  warn "Fix #2 (if no dialog appears): System Settings → Privacy → Accessibility,"
  warn "         click +, press Cmd+Shift+G, type /usr/bin/osascript, Open, toggle ON."
  open "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
fi

# 9) Tail recent log
hdr "recent lilyd stderr (last 20 lines)"
LOG="$HOME/Library/Logs/lily/lilyd.err.log"
if [[ -r "$LOG" ]]; then
  tail -20 "$LOG"
else
  echo "  (no log at $LOG yet)"
fi

echo
echo "Done. If a permission was missing, grant it, then re-run this script."
