#!/usr/bin/env bash
# Lily Computer installer.
#
# Usage:
#   ./scripts/install.sh client   # install just the lily TUI binary
#   ./scripts/install.sh daemon   # install lilyd + LaunchAgent (run as the assistant)
#   ./scripts/install.sh both     # install both on the current user (single-user setup)

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

MODE="${1:-}"
if [[ "$MODE" != "client" && "$MODE" != "daemon" && "$MODE" != "both" ]]; then
  echo "usage: $0 client|daemon|both" >&2
  exit 1
fi

BIN_DIR="$HOME/.local/bin"
mkdir -p "$BIN_DIR"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo "  NOTE: add $BIN_DIR to your PATH (eg. in ~/.zshrc)";;
esac

LAUNCHAGENTS_DIR="$HOME/Library/LaunchAgents"
LOG_DIR="$HOME/Library/Logs/lily"
mkdir -p "$LAUNCHAGENTS_DIR" "$LOG_DIR"

export PATH="$HOME/.cargo/bin:$PATH"

# Pick a build/output directory. If a fresh binary already exists in the
# default target/release (e.g. you built once on the primary user), we'll
# reuse it. Otherwise we build, using a per-user CARGO_TARGET_DIR if the
# project root isn't writable by the current user.
default_build_dir="$ROOT/target/release"
BUILD_DIR="$default_build_dir"

need_lily=true
need_lilyd=true
case "$MODE" in
  client) need_lilyd=false ;;
  daemon) need_lily=false ;;
esac

already_built=true
$need_lily  && [[ ! -x "$default_build_dir/lily"  ]] && already_built=false
$need_lilyd && [[ ! -x "$default_build_dir/lilyd" ]] && already_built=false

if $already_built; then
  echo "▸ using prebuilt binaries in $default_build_dir (skipping cargo)"
else
  if [[ ! -x "$HOME/.cargo/bin/cargo" ]] && ! command -v cargo >/dev/null 2>&1; then
    echo "error: cargo not found. install Rust: https://www.rust-lang.org/tools/install" >&2
    exit 1
  fi

  if [[ ! -w "$ROOT" ]]; then
    export CARGO_TARGET_DIR="$HOME/.cache/lily-target"
    mkdir -p "$CARGO_TARGET_DIR"
    BUILD_DIR="$CARGO_TARGET_DIR/release"
    echo "▸ source dir not writable; building into $CARGO_TARGET_DIR"
  fi

  build_targets=()
  case "$MODE" in
    client) build_targets=(-p lily) ;;
    daemon) build_targets=(-p lilyd) ;;
    both)   build_targets=() ;;
  esac
  echo "▸ cargo build --release ${build_targets[*]:-}"
  cargo build --release "${build_targets[@]}"
fi

ensure_env_file() {
  mkdir -p "$HOME/.lily"
  chmod 700 "$HOME/.lily" 2>/dev/null || true
  if [[ ! -f "$HOME/.lily/env" ]]; then
    cat >"$HOME/.lily/env" <<EOF
# Lily Computer environment. Loaded by lily and lilyd.
# Falls back to /Users/Shared/lily/env if this is empty.
GROQ_API_KEY=
EOF
    chmod 600 "$HOME/.lily/env"
    echo "▸ created $HOME/.lily/env (empty)"
  fi
}

install_client() {
  cp -f "$BUILD_DIR/lily" "$BIN_DIR/lily"
  chmod +x "$BIN_DIR/lily"
  ln -sf "$BIN_DIR/lily" "$BIN_DIR/lc"
  echo "▸ installed $BIN_DIR/lily (alias: lc)"
  ensure_env_file
}

install_daemon() {
  cp -f "$BUILD_DIR/lilyd" "$BIN_DIR/lilyd"
  chmod +x "$BIN_DIR/lilyd"
  echo "▸ installed $BIN_DIR/lilyd"

  # Ad-hoc codesign the daemon. macOS TCC remembers permission grants by
  # the binary's signed identity / hash. An ad-hoc signature gives lilyd a
  # stable identity so the user only has to grant Screen Recording +
  # Accessibility once per build.
  if command -v codesign >/dev/null 2>&1; then
    codesign --force --sign - "$BIN_DIR/lilyd" >/dev/null 2>&1 \
      && echo "▸ ad-hoc codesigned $BIN_DIR/lilyd" \
      || echo "  (codesign skipped: command failed, non-fatal)"
  fi

  ensure_env_file

  PLIST_DST="$LAUNCHAGENTS_DIR/computer.lily.daemon.plist"
  sed \
    -e "s|__LILYD_BIN__|$BIN_DIR/lilyd|g" \
    -e "s|__HOME__|$HOME|g" \
    -e "s|__LOGDIR__|$LOG_DIR|g" \
    "$ROOT/assets/launchagent.plist.tmpl" >"$PLIST_DST"
  echo "▸ wrote $PLIST_DST"

  UID_NUM="$(id -u)"
  launchctl bootout "gui/$UID_NUM/computer.lily.daemon" 2>/dev/null || true
  launchctl bootstrap "gui/$UID_NUM" "$PLIST_DST"
  launchctl kickstart -k "gui/$UID_NUM/computer.lily.daemon"
  echo "▸ launchctl bootstrapped + kickstarted"

  echo
  echo "  lilyd is running. Logs:"
  echo "    $LOG_DIR/lilyd.err.log"
  echo "    $LOG_DIR/lilyd.out.log"
  echo
  echo "  Next: grant Screen Recording + Accessibility permissions."
  echo "  See scripts/grant-perms.md."
}

case "$MODE" in
  client) install_client ;;
  daemon) install_daemon ;;
  both)   install_client; install_daemon ;;
esac

echo "▸ done."
