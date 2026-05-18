#!/usr/bin/env bash
# Lily Computer — one-step installer.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/henrybrewer00-dotcom/lilycomputer/main/setup.sh | bash
#   ── or, if you already cloned the repo ──
#   ./setup.sh
#
# What it does (idempotent — safe to re-run):
#   1. Verifies macOS + Apple Silicon / Intel.
#   2. Installs Rust via rustup if cargo is missing.
#   3. Clones lilycomputer to ~/lilycomputer if not running from inside the repo.
#   4. Builds release binaries (~1 min cold, ~25s incremental).
#   5. Installs `lily` (TUI) + `lilyd` (daemon) to ~/.local/bin and a LaunchAgent
#      so the daemon survives reboot + Fast User Switching.
#   6. Prints the remaining manual steps (Groq key + Chrome extension load).

set -euo pipefail

REPO="https://github.com/henrybrewer00-dotcom/lilycomputer.git"
INSTALL_DIR="$HOME/lilycomputer"

pink() { printf "\033[1;35m%s\033[0m\n" "$*"; }
dim()  { printf "\033[2m%s\033[0m\n" "$*"; }
ok()   { printf "  \033[32m✓\033[0m %s\n" "$*"; }
warn() { printf "  \033[33m!\033[0m %s\n" "$*"; }

pink "─── lily computer · setup ────────────────────────────────────────"

# 1) OS check
if [[ "$(uname)" != "Darwin" ]]; then
  echo "error: lily computer is macOS-only (you're on $(uname))." >&2
  exit 1
fi
ok "macOS $(sw_vers -productVersion) ($(uname -m))"

# 2) Rust
if ! command -v cargo >/dev/null 2>&1 && [[ ! -x "$HOME/.cargo/bin/cargo" ]]; then
  dim "installing Rust toolchain via rustup (no sudo, ~30s)..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal
fi
export PATH="$HOME/.cargo/bin:$PATH"
if ! command -v cargo >/dev/null 2>&1; then
  echo "error: rustup install failed; cargo not on PATH" >&2
  exit 1
fi
ok "cargo $(cargo --version | awk '{print $2}')"

# 3) Repo
if [[ -f "$(pwd)/setup.sh" && -f "$(pwd)/Cargo.toml" ]]; then
  REPO_DIR="$(pwd)"
  ok "running inside existing clone: $REPO_DIR"
else
  if [[ -d "$INSTALL_DIR/.git" ]]; then
    dim "updating existing $INSTALL_DIR..."
    git -C "$INSTALL_DIR" pull --ff-only || true
  else
    dim "cloning lilycomputer → $INSTALL_DIR..."
    git clone "$REPO" "$INSTALL_DIR"
  fi
  REPO_DIR="$INSTALL_DIR"
fi
cd "$REPO_DIR"

# 4) Build
dim "building release binaries (~1 min cold)..."
cargo build --release
ok "built target/release/lily and target/release/lilyd"

# 5) Install (client + daemon on this user)
./scripts/install.sh both

# 6) Next steps
echo
pink "─── almost done ─────────────────────────────────────────────────"
echo
echo "  ▸ 1. Put your Groq API key into  $HOME/.lily/env"
echo "       (free tier at https://console.groq.com/keys — model is meta-llama/llama-4-scout-17b-16e-instruct)"
echo
echo "       echo 'GROQ_API_KEY=gsk_...your_key...' > $HOME/.lily/env && chmod 600 $HOME/.lily/env"
echo
echo "  ▸ 2. Load the Chrome extension on this user:"
echo "       open chrome://extensions, toggle Developer mode (top right),"
echo "       click 'Load unpacked', select $REPO_DIR/extension"
echo
echo "  ▸ 3. Restart the daemon to pick up the key:"
echo "       launchctl kickstart -k gui/\$(id -u)/computer.lily.daemon"
echo
echo "  ▸ 4. Run a quick health check:"
echo "       $REPO_DIR/scripts/doctor.sh"
echo
echo "  ▸ 5. Launch the TUI:"
echo "       lily          # or 'lc' shortcut"
echo
dim "if you want dual-user mode (Lily drives a second macOS account while you stay productive on yours),"
dim "Fast User Switch to that account, re-run this same script there, and follow the same steps."
echo
