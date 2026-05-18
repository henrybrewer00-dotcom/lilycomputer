#!/usr/bin/env bash
# Lily Computer — short installer.
# Type:  curl -L https://raw.githubusercontent.com/henrybrewer00-dotcom/lilycomputer/main/i | sh
#
# Picks up your existing Chrome profile, auto-loads the extension into it, and
# leaves you with `lily` ready to run.

set -euo pipefail

# ───── styling ───────────────────────────────────────────────────────────────
if [[ -t 1 ]]; then
  B=$'\033[1m'; R=$'\033[0m'
  PINK=$'\033[38;5;213m'; GREEN=$'\033[38;5;77m'; RED=$'\033[38;5;167m'
  GRAY=$'\033[38;5;245m'; YEL=$'\033[38;5;221m'; CYAN=$'\033[38;5;117m'
else
  B=""; R=""; PINK=""; GREEN=""; RED=""; GRAY=""; YEL=""; CYAN=""
fi

STEP=0; TOTAL=7
step() { STEP=$((STEP+1)); printf "\n${B}${PINK}[%d/%d]${R} ${B}%s${R}\n" "$STEP" "$TOTAL" "$1"; }
ok()   { printf "  ${GREEN}✓${R} %s\n" "$*"; }
warn() { printf "  ${YEL}!${R} %s\n" "$*"; }
err()  { printf "  ${RED}✗${R} %s\n" "$*" >&2; }
note() { printf "  ${GRAY}%s${R}\n" "$*"; }
hr()   { printf "${PINK}─────────────────────────────────────────────────────────────${R}\n"; }

trap 'echo; err "setup aborted"; exit 130' INT

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

# ───── 1. system check ───────────────────────────────────────────────────────
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
if cargo build --release 2>"$LOG" 1>&2; then
  rm -f "$LOG"
  ok "built lily + lilyd"
else
  err "build failed — last 30 lines:"
  tail -30 "$LOG"; rm -f "$LOG"
  exit 1
fi

# ───── 5. install binaries + LaunchAgent ─────────────────────────────────────
step "installing binaries + LaunchAgent"
./scripts/install.sh both 2>&1 | sed "s/^/  ${GRAY}|${R} /"

# lily-chrome wrapper — relaunch Chrome with extension + saved profile.
LILY_CHROME="$HOME/.local/bin/lily-chrome"
cat >"$LILY_CHROME" <<EOF
#!/usr/bin/env bash
# Relaunch your Chrome with the Lily extension loaded into your chosen profile.
PROFILE_CONF="\$HOME/.lily/chrome-profile.conf"
PROFILE_ARG=""
if [[ -r "\$PROFILE_CONF" ]]; then
  PROFILE_DIR="\$(cat "\$PROFILE_CONF")"
  PROFILE_ARG="--profile-directory=\$PROFILE_DIR"
fi
exec open -a "/Applications/Google Chrome.app" --args \\
  \$PROFILE_ARG \\
  --load-extension="$REPO_DIR/extension" \\
  --no-first-run \\
  --no-default-browser-check \\
  "\$@"
EOF
chmod +x "$LILY_CHROME"
ok "$LILY_CHROME ready"

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
  note "get a free key at https://console.groq.com/keys"
  printf "  ${CYAN}paste key (hidden):${R} "
  if [[ -t 0 ]]; then
    read -rs GROQ_KEY < /dev/tty || GROQ_KEY=""
  else
    read -rs GROQ_KEY || GROQ_KEY=""
  fi
  echo
  if [[ -n "$GROQ_KEY" && "$GROQ_KEY" == gsk_* ]]; then
    mkdir -p "$HOME/.lily"; chmod 700 "$HOME/.lily" || true
    printf 'GROQ_API_KEY=%s\n' "$GROQ_KEY" > "$ENV_FILE"
    chmod 600 "$ENV_FILE"
    ok "saved to $ENV_FILE"
    launchctl kickstart -k "gui/$(id -u)/computer.lily.daemon" 2>/dev/null || true
  else
    warn "skipped — drop key into $ENV_FILE later"
  fi
fi

# ───── 7. Chrome profile picker + extension auto-load ────────────────────────
step "Chrome — picking profile + loading extension"
CHROME_APP="/Applications/Google Chrome.app"
LOCAL_STATE="$HOME/Library/Application Support/Google/Chrome/Local State"

if [[ ! -d "$CHROME_APP" ]]; then
  warn "Google Chrome not installed at $CHROME_APP — install it, then run: lily-chrome"
else
  # Make sure Chrome has been launched at least once so a profile exists.
  if [[ ! -f "$LOCAL_STATE" ]]; then
    note "Chrome has no profile yet — launching it briefly to create one..."
    open -a "$CHROME_APP" >/dev/null 2>&1 || true
    for i in 1 2 3 4 5 6 7 8; do
      [[ -f "$LOCAL_STATE" ]] && break
      sleep 0.5
    done
    sleep 1
    osascript -e 'tell application "Google Chrome" to quit' 2>/dev/null || true
    sleep 0.5
  fi

  # Enumerate profiles via Python (jq isn't guaranteed on macOS).
  mapfile -t PROFILES < <(python3 - "$LOCAL_STATE" <<'PY' 2>/dev/null
import json, sys
try:
    with open(sys.argv[1]) as f: s = json.load(f)
    for k, v in (s.get("profile", {}) or {}).get("info_cache", {}).items():
        name = (v or {}).get("name") or k
        print(f"{k}\t{name}")
except Exception:
    pass
PY
)

  if [[ ${#PROFILES[@]} -eq 0 ]]; then
    warn "couldn't read Chrome profile list — defaulting to Default"
    CHOSEN_DIR="Default"
    CHOSEN_NAME="Default"
  elif [[ ${#PROFILES[@]} -eq 1 ]]; then
    IFS=$'\t' read -r CHOSEN_DIR CHOSEN_NAME <<<"${PROFILES[0]}"
    ok "one profile found: $CHOSEN_NAME ($CHOSEN_DIR)"
  else
    note "found ${#PROFILES[@]} Chrome profiles. ↑↓ to select, Enter to confirm:"
    echo

    # Arrow-key picker. Draw, listen, redraw on change.
    SEL=0
    N=${#PROFILES[@]}
    printf '\033[?25l'   # hide cursor

    draw() {
      for i in "${!PROFILES[@]}"; do
        IFS=$'\t' read -r dir name <<<"${PROFILES[$i]}"
        if [[ $i -eq $SEL ]]; then
          printf "  ${PINK}▸${R} ${B}%s${R}  ${GRAY}(%s)${R}\n" "$name" "$dir"
        else
          printf "    %s  ${GRAY}(%s)${R}\n" "$name" "$dir"
        fi
      done
    }

    clear_lines() {
      for ((i=0; i<N; i++)); do printf '\033[1A\033[2K'; done
    }

    draw
    while true; do
      IFS= read -rsn1 ch
      if [[ $ch == $'\x1b' ]]; then
        read -rsn2 -t 0.1 ch || true
        case $ch in
          '[A') ((SEL > 0)) && ((SEL--)) ;;
          '[B') ((SEL < N - 1)) && ((SEL++)) ;;
        esac
        clear_lines; draw
      elif [[ -z $ch || $ch == $'\n' ]]; then
        break
      fi
    done
    printf '\033[?25h'

    IFS=$'\t' read -r CHOSEN_DIR CHOSEN_NAME <<<"${PROFILES[$SEL]}"
    ok "using profile: $CHOSEN_NAME ($CHOSEN_DIR)"
  fi

  # Save choice so lily-chrome reuses it later.
  mkdir -p "$HOME/.lily"
  printf '%s\n' "$CHOSEN_DIR" > "$HOME/.lily/chrome-profile.conf"

  # Quit Chrome if running, then relaunch with extension preloaded.
  if pgrep -x "Google Chrome" >/dev/null; then
    note "quitting Chrome to reload with extension..."
    osascript -e 'tell application "Google Chrome" to quit' 2>/dev/null || true
    for i in 1 2 3 4 5 6 7 8; do
      pgrep -x "Google Chrome" >/dev/null || break
      sleep 0.5
    done
  fi

  EXT_DIR="$REPO_DIR/extension"
  open -a "$CHROME_APP" --args \
    --profile-directory="$CHOSEN_DIR" \
    --load-extension="$EXT_DIR" \
    --no-first-run \
    --no-default-browser-check 2>/dev/null || true
  ok "Chrome relaunched with extension loaded into '$CHOSEN_NAME'"

  # Verify connection.
  TOKEN="$(cat /Users/Shared/lily/token 2>/dev/null || true)"
  for i in 1 2 3 4 5 6 7 8 9 10; do
    if [[ -n "$TOKEN" ]] && curl -sS -m 1 -H "Authorization: Bearer $TOKEN" \
       http://127.0.0.1:7777/diagnose 2>/dev/null \
       | grep -q '"browser_extension"[^}]*"connected":true'; then
      ok "extension connected to lilyd"
      EXT_OK=1
      break
    fi
    sleep 0.5
  done
  if [[ -z "${EXT_OK:-}" ]]; then
    warn "extension didn't connect within 5s — Chrome should have it but it's slow to register"
    note "give it 10s, then: $REPO_DIR/scripts/doctor.sh"
  fi
fi

# ───── done ──────────────────────────────────────────────────────────────────
echo
hr
printf "  ${B}lily computer is ready.${R}\n\n"
printf "  ${B}lily${R}            ${GRAY}# launch the TUI${R}\n"
printf "  ${B}lc${R}              ${GRAY}# shorter alias${R}\n"
printf "  ${B}lily-chrome${R}     ${GRAY}# relaunch Chrome with extension if you closed it${R}\n\n"
printf "  ${GRAY}for the extension to load every time you open Chrome, use ${B}lily-chrome${R}${GRAY},${R}\n"
printf "  ${GRAY}or just launch Chrome normally — the extension persists for THIS session.${R}\n"
printf "  ${GRAY}troubleshoot:  $REPO_DIR/scripts/doctor.sh${R}\n"
hr
echo
