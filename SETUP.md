# Lily Computer — Setup

This is the long-form setup guide. If you just want it working, run the
one-line installer in `README.md` and follow the 3 manual steps it prints.

---

## 0. Prerequisites

- macOS 14+ (tested on 26 / Tahoe).
- Google Chrome (any recent version).
- A [Groq API key](https://console.groq.com/keys). Free tier is plenty for
  exploratory use; the agent uses `meta-llama/llama-4-scout-17b-16e-instruct`
  on the `on_demand` service tier.
- Rust toolchain — installed automatically by `setup.sh` if you don't have it.

---

## 1. Choose a topology

**Single-user (simpler).** `lily` and `lilyd` both run on your normal macOS
account. Lily acts on the same screen you're using. Best for: trying it out,
desktop automation that doesn't conflict with your workflow.

**Dual-user (more interesting).** `lilyd` runs as a second macOS user via Fast
User Switching. `lily` (the TUI) runs on your normal account and talks to
`lilyd` over `127.0.0.1`. Lily clicks/types in the *other* account's apps;
you keep using yours. Best for: long-running browser automation in parallel
with your work.

You can switch topology at any time — just re-run `setup.sh` on the user you
want the daemon to live on.

---

## 2. Install

### One-step (recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/henrybrewer00-dotcom/lilycomputer/main/setup.sh | bash
```

For dual-user, switch to the assistant account (e.g. `the assistant user`), open a
Terminal, and run the same command there.

### Manual

```bash
git clone https://github.com/henrybrewer00-dotcom/lilycomputer.git ~/lilycomputer
cd ~/lilycomputer
./scripts/install.sh both        # client + daemon on this user
```

`install.sh` accepts `client`, `daemon`, or `both`. Examples:

- `./scripts/install.sh both` — typical single-user setup
- `./scripts/install.sh daemon` (as `the assistant user`) + `./scripts/install.sh client` (as `your-main-user`) — dual-user

The daemon is registered as a LaunchAgent
(`~/Library/LaunchAgents/computer.lily.daemon.plist`) so it survives reboot
and Fast User Switching.

---

## 3. Groq key

```bash
echo 'GROQ_API_KEY=gsk_yourkeyhere' > ~/.lily/env
chmod 600 ~/.lily/env
launchctl kickstart -k gui/$(id -u)/computer.lily.daemon
```

`lilyd` reads the key from `~/.lily/env` on startup, falling back to
`/Users/Shared/lily/env` (machine-scoped, shared between users on the same
machine).

---

## 4. Chrome extension

Required if you want Lily to use browser tools (which is most of what it's
good at).

1. Open `chrome://extensions` in Chrome on the *daemon* user.
2. Toggle **Developer mode** (top right).
3. Click **Load unpacked**.
4. Select the `extension/` folder inside your clone
   (default: `~/lilycomputer/extension`).
5. Click the puzzle-piece icon in Chrome's toolbar, pin "Lily Computer", and
   open its **Options**. The dot should turn green within a second or two.

The extension's service worker connects to `ws://127.0.0.1:7777/ws/chrome`
and stays connected; it auto-reconnects with backoff if the daemon restarts.

---

## 5. macOS permissions (only for native, non-browser tools)

These are only needed if you want Lily to operate non-browser apps (Mail,
Finder, Music, custom apps, etc.). If you only care about browser automation,
skip — the extension doesn't need any of them.

On the daemon user, run:

```bash
~/.local/bin/lilyd warmup
```

It tests three things and triggers the relevant macOS prompts:

- **Screen Recording** — for `screencapture`. Foreground only on Tahoe.
- **Automation → System Events** — for `applescript` / `tell application "System Events"`.
- **Accessibility (granted to `/usr/bin/osascript`)** — for clicks + keystrokes.

If a prompt doesn't appear, the script opens the relevant System Settings
pane for you and tells you exactly what to toggle.

After granting:

```bash
launchctl kickstart -k gui/$(id -u)/computer.lily.daemon
~/lilycomputer/scripts/doctor.sh
```

`doctor.sh` runs from inside the daemon's TCC context and reports
per-permission status, so it's accurate where a naive shell test isn't.

---

## 6. First run

```bash
lily                                    # interactive
lily "what's open in chrome right now"  # one-shot, stays open
lily --once "summarize my unread mail"  # exits when done
```

Inside the TUI: `/help` for commands, `⌃R` to clear, `↑` for prompt history.

---

## 7. Troubleshooting

### "VISION_UNAVAILABLE" when Lily takes a screenshot
You're trying to capture a backgrounded user's display. Macos Tahoe blocks
this at the OS level (no compositor running for that session). Use the
browser tools instead — they work because they render inside Chrome's own
process.

### `-1743` "not authorized to send Apple events to System Events"
Automation permission missing. Run `lilyd warmup` from the daemon user's
terminal and click Allow on the popup.

### `1002` "osascript is not allowed to send keystrokes"
Accessibility missing for `/usr/bin/osascript`. System Settings → Privacy &
Security → Accessibility → click `+` → `⌘⇧G` → paste `/usr/bin/osascript`
→ Enter → toggle ON.

### "no Chrome extension is connected"
The extension's service worker isn't running. Open `chrome://extensions`,
make sure "Lily Computer" is enabled, click its **service worker** link and
verify it logs "[lily] connected". `chrome://serviceworker-internals` →
**Inspect** for live logs.

### `Address already in use (os error 48)`
Another `lilyd` is already bound to port 7777. `launchctl kickstart -k
gui/$(id -u)/computer.lily.daemon` to restart the official one. Or set
`LILY_PORT=7780` in the LaunchAgent plist if you have a port conflict.

### "GROQ_API_KEY not found"
Put the key in `~/.lily/env` or `/Users/Shared/lily/env` and restart the
daemon. The daemon checks env var first, then those two files.

---

## 8. Uninstall

```bash
~/lilycomputer/scripts/uninstall.sh   # removes binaries + LaunchAgent
rm -rf ~/lilycomputer ~/.lily         # remove source + per-user state
sudo rm -rf /Users/Shared/lily         # remove machine-wide token + key (optional)
```

In Chrome, go to `chrome://extensions` and click **Remove** under Lily Computer.

---

## 9. Build from source

```bash
git clone https://github.com/henrybrewer00-dotcom/lilycomputer.git
cd lilycomputer
cargo build --release
./scripts/install.sh both
```

Workspace structure:

- `crates/lily/` — TUI client (~600 LoC, ratatui + crossterm)
- `crates/lilyd/` — daemon (~900 LoC, axum + tokio)
- `crates/lily-core/` — shared types (~150 LoC)
- `crates/lily-actions/` — macOS tools (~700 LoC)
- `extension/` — Chrome MV3 extension (~400 LoC JS)

Release profile uses `lto = "fat"` + `codegen-units = 1` so the binaries are
~2-3 MB each.

---

## 10. Architecture (one-paragraph summary)

`lilyd` is an HTTP+SSE+WebSocket axum server bound to `127.0.0.1:7777`. The
TUI POSTs `/run` with a natural-language prompt; the agent loop calls Groq
with a tool schema, dispatches tool calls either through `lily-actions` (for
macOS tools — `osascript`, `screencapture`, `System Events` AX tree, shell)
or through `BrowserBridge` (for `browser_*` tools, relayed over the WebSocket
to the Chrome extension); responses stream back to the TUI as SSE. The agent
maintains conversation memory in `~/.lily/history.json` across daemon restarts.

---

If something's wrong and `doctor.sh` doesn't tell you what, the daemon logs
are at `~/Library/Logs/lily/lilyd.err.log`.
