#!/usr/bin/env bash
# Lily Computer — short installer.
#   curl -L tinyurl.com/lily-get | sh
#
# Walks you through: build → install daemon → Groq key → macOS permissions
# (Screen Recording, Automation, Accessibility) → Chrome extension via
# Load Unpacked at chrome://extensions. Ends with a working `lily`.

set -euo pipefail

# Mode — passed via:  curl ... | sh -s client    (or "assistant" / "both")
# Defaults to a full install (assistant + client on the same user).
MODE="${1:-both}"
case "$MODE" in
  client|assistant|both) ;;
  *) printf 'usage: curl ... | sh -s [client|assistant|both]\n' >&2; exit 2 ;;
esac

# ───── styling ───────────────────────────────────────────────────────────────
if [[ -t 1 ]]; then
  B=$'\033[1m'; R=$'\033[0m'
  PINK=$'\033[38;5;213m'; GREEN=$'\033[38;5;77m'; RED=$'\033[38;5;167m'
  GRAY=$'\033[38;5;245m'; YEL=$'\033[38;5;221m'; CYAN=$'\033[38;5;117m'
else
  B=""; R=""; PINK=""; GREEN=""; RED=""; GRAY=""; YEL=""; CYAN=""
fi

STEP=0
case "$MODE" in
  client)    TOTAL=5 ;;
  assistant) TOTAL=8 ;;
  both)      TOTAL=8 ;;
esac
step()  { STEP=$((STEP+1)); printf "\n${B}${PINK}[%d/%d]${R} ${B}%s${R}\n" "$STEP" "$TOTAL" "$1"; }
ok()    { printf "  ${GREEN}✓${R} %s\n" "$*"; }
warn()  { printf "  ${YEL}!${R} %s\n" "$*"; }
err()   { printf "  ${RED}✗${R} %s\n" "$*" >&2; }
note()  { printf "  ${GRAY}%s${R}\n" "$*"; }
hr()    { printf "${PINK}─────────────────────────────────────────────────────────────${R}\n"; }
prompt(){ printf "  ${CYAN}%s${R}" "$*"; }

trap 'echo; err "setup aborted"; exit 130' INT

# DON'T do `exec </dev/tty` at script level here — when bash is reading the
# script from a pipe (curl | sh), that command swaps stdin mid-parse and bash
# starts trying to read the REST OF THE SCRIPT from your terminal. The script
# appears to hang. Instead, the few `read` calls below explicitly redirect
# their own stdin from /dev/tty.
TTY="/dev/tty"
[[ -r "$TTY" && -w "$TTY" ]] || TTY=""

printf "\n${PINK}"
cat <<'BANNER'
   __    _ __         ______                            __
  / /   (_) /_  __   / ____/___  ____ ___  ____  __  __/ /____  _____
 / /   / / / / / /  / /   / __ \/ __ `__ \/ __ \/ / / / __/ _ \/ ___/
/ /___/ / / /_/ /  / /___/ /_/ / / / / / / /_/ / /_/ / /_/  __/ /
\____/_/_/\__, /   \____/\____/_/ /_/ /_/ .___/\__,_/\__/\___/_/
         /____/                        /_/
BANNER
printf "${R}\n${GRAY}  a terminal AI agent that drives your Chrome from a CLI prompt.${R}\n\n"

# ───── 1. system ─────────────────────────────────────────────────────────────
step "checking system"
[[ "$(uname)" == "Darwin" ]] || { err "macOS only"; exit 1; }
ok "macOS $(sw_vers -productVersion) ($(uname -m))"

# ───── 2. rust ────────────────────────────────────────────────────────────────
step "rust toolchain"
if command -v cargo >/dev/null 2>&1 || [[ -x "$HOME/.cargo/bin/cargo" ]]; then
  export PATH="$HOME/.cargo/bin:$PATH"
  ok "cargo $(cargo --version | awk '{print $2}')"
else
  note "rustup not found — installing (no sudo, ~30s)"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal >/dev/null 2>&1
  export PATH="$HOME/.cargo/bin:$PATH"
  command -v cargo >/dev/null 2>&1 || { err "rustup install failed"; exit 1; }
  ok "cargo $(cargo --version | awk '{print $2}')"
fi

# ───── 3. source ──────────────────────────────────────────────────────────────
step "fetching lilycomputer"
if [[ -f "$(pwd)/i" && -f "$(pwd)/Cargo.toml" && -d "$(pwd)/crates" ]]; then
  REPO_DIR="$(pwd)"
  ok "using current clone at $REPO_DIR"
else
  REPO_DIR="$HOME/lilycomputer"
  if [[ -d "$REPO_DIR/.git" ]]; then
    note "updating $REPO_DIR..."
    git -C "$REPO_DIR" pull --ff-only --quiet 2>/dev/null || true
    ok "updated $REPO_DIR"
  else
    note "cloning to $REPO_DIR..."
    git clone --quiet https://github.com/henrybrewer00-dotcom/lilycomputer.git "$REPO_DIR"
    ok "cloned to $REPO_DIR"
  fi
fi
cd "$REPO_DIR"

# ───── 4. build ───────────────────────────────────────────────────────────────
step "building (~1 min cold, ~25s incremental)"
LOG="$(mktemp -t lily-build).log"
case "$MODE" in
  client) BUILD_ARGS="-p lily" ;;
  *)      BUILD_ARGS="" ;;
esac
if cargo build --release $BUILD_ARGS 2>"$LOG" 1>&2; then
  rm -f "$LOG"
  case "$MODE" in
    client) ok "built lily" ;;
    *)      ok "built lily + lilyd" ;;
  esac
else
  err "build failed — last 30 lines:"
  tail -30 "$LOG"; rm -f "$LOG"
  exit 1
fi

# ───── 5. binaries + LaunchAgent ─────────────────────────────────────────────
case "$MODE" in
  client) step "installing client binary (lily)" ;;
  *)      step "installing binaries + LaunchAgent" ;;
esac
./scripts/install.sh "$MODE" 2>&1 | sed "s/^/  ${GRAY}|${R} /"

# Client-only mode is done after step 5. The remaining steps are
# permissions + Chrome which only matter on the assistant side.
if [[ "$MODE" == "client" ]]; then
  echo
  hr
  printf "  ${B}lily client is ready.${R}\n\n"
  printf "  Make sure your assistant Mac has lilyd running, then:\n\n"
  printf "    ${B}lily${R}      ${GRAY}# launch the TUI${R}\n"
  printf "    ${B}lc${R}        ${GRAY}# shorter alias${R}\n\n"
  printf "  ${GRAY}On the assistant, run:  curl -L tinyurl.com/lily-get|sh -s assistant${R}\n"
  hr
  echo
  exit 0
fi
TOKEN_FILE=/Users/Shared/lily/token
for i in 1 2 3 4 5 6 7 8 9 10; do
  [[ -r "$TOKEN_FILE" ]] && break
  sleep 0.3
done

# Helper: hit /diagnose and return the JSON
fetch_diag() {
  local token
  token="$(cat "$TOKEN_FILE" 2>/dev/null || true)"
  [[ -z "$token" ]] && return 1
  curl -sS -m 3 -H "Authorization: Bearer $token" http://127.0.0.1:7777/diagnose 2>/dev/null
}
diag_has() {
  local key="$1" json
  json="$(fetch_diag)" || return 1
  [[ -z "$json" ]] && return 1
  printf '%s' "$json" | python3 -c '
import json, sys
try:
    d = json.load(sys.stdin)
except Exception:
    sys.exit(1)
parts = sys.argv[1].split(".")
v = d
for p in parts:
    v = v.get(p) if isinstance(v, dict) else None
    if v is None: sys.exit(1)
sys.exit(0 if v is True else 1)
' "$key"
}

# ───── 6. groq key ────────────────────────────────────────────────────────────
step "groq api key"
ENV_FILE="$HOME/.lily/env"
SHARED_ENV="/Users/Shared/lily/env"
has_key() { local f="$1"; [[ -f "$f" ]] && grep -q '^GROQ_API_KEY=gsk_' "$f"; }

if has_key "$ENV_FILE"; then
  ok "key already in $ENV_FILE"
elif has_key "$SHARED_ENV"; then
  ok "key found in shared $SHARED_ENV"
else
  note "free key at https://console.groq.com/keys"
  printf "  ${CYAN}paste key (hidden):${R} "
  if [[ -n "$TTY" ]]; then
    IFS= read -rs GROQ_KEY <"$TTY" || GROQ_KEY=""
  else
    GROQ_KEY=""
    warn "no terminal attached — skipping key prompt"
  fi
  echo
  if [[ -n "$GROQ_KEY" && "$GROQ_KEY" == gsk_* ]]; then
    mkdir -p "$HOME/.lily"; chmod 700 "$HOME/.lily" || true
    printf 'GROQ_API_KEY=%s\n' "$GROQ_KEY" > "$ENV_FILE"
    chmod 600 "$ENV_FILE"
    ok "saved to $ENV_FILE"
    launchctl kickstart -k "gui/$(id -u)/computer.lily.daemon" 2>/dev/null || true
    sleep 1
  else
    warn "no key entered — drop one into $ENV_FILE later and kickstart the daemon"
  fi
fi

# ───── 7. macOS permissions ───────────────────────────────────────────────────
step "macOS permissions"
echo
note "Lily needs three macOS permissions for non-browser tools (Mail,"
note "Finder, Music, etc). The Chrome extension itself doesn't need any."
note "I'll trigger the prompts now — click 'Allow' on each as they appear."
echo

# Run warmup once — this triggers Screen Recording + Automation + Accessibility
# prompts by exercising each API. macOS attributes the prompts to lilyd / osascript.
note "▸ running:  ~/.local/bin/lilyd warmup"
"$HOME/.local/bin/lilyd" warmup 2>&1 | sed "s/^/    ${GRAY}|${R} /"
echo

# Each permission: check live, if missing, open System Settings + tell user
# exactly what to do, then poll for grant. User can press Enter to skip.
check_perm() {
  local label="$1" diag_key="$2" pane="$3" fix_msg="$4"
  printf "  checking ${B}%s${R}... " "$label"
  if diag_has "$diag_key"; then
    printf "${GREEN}✓${R}\n"
    return 0
  fi
  printf "${YEL}needs grant${R}\n"
  printf "  ${GRAY}%s${R}\n" "$fix_msg"
  note "opening System Settings → $label..."
  open "$pane" 2>/dev/null || true
  printf "  ${CYAN}press Enter when granted (or 's' to skip):${R} "
  ans=""
  if [[ -n "$TTY" ]]; then
    IFS= read -r ans <"$TTY" || ans=""
  else
    warn "no terminal — auto-skipping $label"; return 1
  fi
  if [[ "$ans" == "s" ]]; then warn "skipped $label"; return 1; fi
  # Kickstart daemon so it picks up the new grant
  launchctl kickstart -k "gui/$(id -u)/computer.lily.daemon" 2>/dev/null || true
  sleep 1
  if diag_has "$diag_key"; then
    printf "  ${GREEN}✓${R} $label\n"
    return 0
  else
    warn "$label still missing — run scripts/doctor.sh later"
    return 1
  fi
}

check_perm "Screen Recording" \
  "screen_recording.ok" \
  "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture" \
  "add ~/.local/bin/lilyd to the list, toggle ON"

check_perm "Automation (System Events)" \
  "automation_system_events.ok" \
  "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation" \
  "expand osascript / lilyd, check 'System Events'"

check_perm "Accessibility (for osascript)" \
  "accessibility.ok" \
  "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility" \
  "click +, ⌘⇧G, paste /usr/bin/osascript, toggle ON"

# ───── 8. Chrome extension via Load Unpacked ─────────────────────────────────
step "Chrome extension"
EXT_DIR="$REPO_DIR/extension"
echo
note "now Chrome. The extension lives at:"
echo
printf "    ${B}${CYAN}%s${R}\n" "$EXT_DIR"
echo
note "I'm going to open chrome://extensions for you. There:"
note "  1) toggle ${B}Developer mode${R} (top-right corner)"
note "  2) click ${B}Load unpacked${R}"
note "  3) pick the folder above (copy/paste it into the Open dialog if easier)"
echo
# Try to put the path on the clipboard so the user can ⌘V it into the Open dialog
if command -v pbcopy >/dev/null 2>&1; then
  printf '%s' "$EXT_DIR" | pbcopy 2>/dev/null && note "(folder path copied to clipboard — paste into the Open dialog)"
fi
echo

# Open chrome://extensions
if [[ -d "/Applications/Google Chrome.app" ]]; then
  open -a "Google Chrome" "chrome://extensions" 2>/dev/null \
    || open "chrome://extensions" 2>/dev/null \
    || true
  ok "chrome://extensions opened"
else
  warn "Google Chrome not in /Applications — install Chrome, then visit chrome://extensions and Load unpacked from the path above"
fi

# Poll for the extension to actually connect over the WebSocket bridge.
echo
note "waiting for the extension to connect..."
EXT_OK=""
for i in $(seq 1 120); do  # up to ~120s
  if diag_has "browser_extension.connected"; then
    EXT_OK=1
    break
  fi
  if (( i % 10 == 0 )); then
    note "  still waiting (${i}s)... if you haven't yet, click Load unpacked + pick the folder above"
  fi
  sleep 1
done

if [[ -n "$EXT_OK" ]]; then
  ok "Chrome extension connected to lilyd"
else
  warn "extension still not connected — that's OK, you can finish loading later."
  note "after Load unpacked, re-run:  $REPO_DIR/scripts/doctor.sh"
fi

# ───── done ──────────────────────────────────────────────────────────────────
echo
hr
printf "  ${B}lily computer is ready.${R}\n\n"
printf "  ${B}lily${R}      ${GRAY}# the TUI${R}\n"
printf "  ${B}lc${R}        ${GRAY}# shorter alias${R}\n\n"
printf "  ${GRAY}troubleshoot:  $REPO_DIR/scripts/doctor.sh${R}\n"
hr
echo
