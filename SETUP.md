# Lily Computer — Setup

Two ways to run Lily:

- **Single-user.** One Mac account does it all: `lily` (the TUI you type into) and
  `lilyd` (the daemon that drives Chrome) both run on your normal account.
  Easiest path. Read the **Assistant guide** below — that's the whole thing.

- **Dual-user.** `lilyd` and Chrome run on a second macOS account you switch to
  via Fast User Switching. Your normal account just runs `lily` (the TUI) and
  talks to `lilyd` over loopback. You can keep using your computer while Lily
  works. Read **both** guides — Assistant first (set up the worker side),
  then Client.

---

## Assistant guide

This is the side that runs the daemon, Chrome, and the extension.
(For single-user, this is your only setup.)

### 1 · Run the installer

```bash
curl -L tinyurl.com/lily-get|sh
```

This builds + installs `lily`, `lilyd`, and the LaunchAgent. ~90 seconds.

### 2 · Groq API key

The installer asks for it (hidden input). If you skip it, drop it in later:

```bash
echo 'GROQ_API_KEY=gsk_yourkeyhere' > ~/.lily/env
chmod 600 ~/.lily/env
launchctl kickstart -k gui/$(id -u)/computer.lily.daemon
```

Free key at <https://console.groq.com/keys>. Model:
`meta-llama/llama-4-scout-17b-16e-instruct` on `service_tier: on_demand`.

### 3 · macOS permissions

The installer runs `lilyd warmup`, which exercises each TCC-gated API and
triggers macOS prompts. For any that don't grant cleanly, the installer
opens the System Settings pane and waits for you to confirm.

You only need these if you want Lily to drive *native* macOS apps (Mail,
Finder, Music, etc.). The Chrome extension does its own thing and doesn't
need them.

The three perms:

- **Screen Recording** — for `screencapture`. Foreground only on Tahoe;
  expected to fail on backgrounded users (use the AX-tree tools / browser
  tools instead).
- **Automation → System Events** — for `applescript` / `osascript` calls
  into System Events. (Causes `-1743` errors when missing.)
- **Accessibility** — granted to `/usr/bin/osascript`. This is what lets
  System Events synthesize keystrokes and clicks. (Causes `1002` "not
  allowed assistive access" when missing.)

### 4 · Chrome extension

Final installer step. It prints the extension folder, copies the path
to your clipboard, and opens `chrome://extensions`. There:

1. Toggle **Developer mode** (top right).
2. Click **Load unpacked**.
3. Paste the path (⌘V) into the Open dialog → Enter.

You'll see "Lily Computer" appear in the extensions list. Pin it from
the puzzle-piece menu. The installer polls `/diagnose` for up to 2
minutes and prints "Chrome extension connected to lilyd" when the
service worker hooks up.

### 5 · Verify

```bash
~/lilycomputer/scripts/doctor.sh
```

All four lines should be green:

```
✓ Screen Recording — daemon can screencapture
✓ Automation → System Events — daemon can talk to apps
✓ Accessibility — daemon can send keystrokes / click_element
✓ Chrome extension — connected over WebSocket
```

### 6 · First run

```bash
lily                       # interactive TUI
lily "do something"        # runs the prompt at startup, then stays open
lily --once "do X"         # runs and exits
```

Inside the TUI: `/help` for commands, `⌃R` clears memory, `↑` recalls
history, `v` views the latest screenshot, `/autoview` opens every
screenshot in Preview automatically.

---

## Client guide

Only needed if you're running dual-user (the Mac/account where you'll
type prompts, separate from the assistant). For single-user, skip this
section — `lily` is already installed.

### 1 · Run the client-only installer

```bash
curl -L tinyurl.com/lily-get|sh -s client
```

Only ~15 seconds. Builds + installs the `lily` TUI binary (plus the `lc`
alias). No daemon, no LaunchAgent, no Chrome, no permissions.

### 2 · Verify the assistant is up

```bash
lily
```

If the assistant is reachable, you see the banner inside the TUI and a
green status dot in the header. If not:

```
  lily: cannot reach lilyd at http://127.0.0.1:7777 — connect
  Is lilyd running on this machine?
  Try:  launchctl print gui/$(id -u)/computer.lily.daemon
```

That means lilyd isn't running on the assistant. Switch to the assistant
account and run `~/lilycomputer/scripts/doctor.sh` there.

### Note on loopback

Lily uses `127.0.0.1:7777` for all client↔assistant traffic. On macOS,
loopback is machine-scoped (not user-scoped), so two users on the same
Mac via Fast User Switching can talk to each other over it. The token at
`/Users/Shared/lily/token` is world-readable inside that one Mac, which
is how both users authenticate.

If you want client and assistant on *different* physical Macs, you'd
need to expose lilyd on a non-loopback interface and add real auth.
Not supported out of the box.

---

## Topology in one paragraph

`lilyd` listens on `127.0.0.1:7777`. The Chrome extension and the TUI
both connect to that. In single-user, all three are in the same session
and process group. In dual-user, the TUI is in your foreground session
and lilyd + Chrome are in the assistant's backgrounded session — but
since loopback is machine-scoped, the TUI still reaches lilyd, and the
extension still answers commands inside Chrome's process (independent
of macOS's WindowServer compositor state).

---

## Troubleshooting

### "VISION_UNAVAILABLE" when Lily tries to take a screenshot
You're trying to capture a backgrounded user's display. macOS Tahoe
blocks this — no compositor running for that session. Use the browser
tools (or the AX-tree tools for native apps). Native screenshots only
work when the assistant is foregrounded.

### `-1743` "not authorized to send Apple events to System Events"
Automation perm missing for `lilyd`. Run `~/.local/bin/lilyd warmup`
from the assistant's terminal and Allow the popup. Or System Settings
→ Privacy & Security → Automation → osascript → toggle "System Events".

### `1002` "osascript is not allowed to send keystrokes"
Accessibility missing for `/usr/bin/osascript`. System Settings → Privacy
& Security → Accessibility → click `+` → `⌘⇧G` → paste
`/usr/bin/osascript` → Enter → toggle ON.

### "no Chrome extension is connected"
The extension's service worker isn't running. Open `chrome://extensions`,
make sure "Lily Computer" is enabled, click its **service worker** link
and verify it logs `[lily] connected`. `chrome://serviceworker-internals`
→ **Inspect** for live logs.

### `Address already in use (os error 48)`
Another `lilyd` is bound to 7777. `launchctl kickstart -k gui/$(id
-u)/computer.lily.daemon` to restart the official one. Or set
`LILY_PORT=7780` in the LaunchAgent plist.

### "GROQ_API_KEY not found"
Put the key in `~/.lily/env` (preferred) or `/Users/Shared/lily/env`
(shared between users). The daemon checks env var first, then those
two files.

---

## Uninstall

```bash
~/lilycomputer/scripts/uninstall.sh    # binaries + LaunchAgent
rm -rf ~/lilycomputer ~/.lily          # source + per-user state
sudo rm -rf /Users/Shared/lily         # machine-wide token + key (optional)
```

In Chrome: `chrome://extensions` → Remove under Lily Computer.

---

## Build from source

```bash
git clone https://github.com/henrybrewer00-dotcom/lilycomputer.git
cd lilycomputer
cargo build --release
./scripts/install.sh both        # or 'client' / 'assistant'
```

Workspace:

- `crates/lily/` — TUI client (~600 LoC, ratatui + crossterm)
- `crates/lilyd/` — daemon (~900 LoC, axum + tokio)
- `crates/lily-core/` — shared protocol + config (~150 LoC)
- `crates/lily-actions/` — macOS tools (~700 LoC)
- `extension/` — Chrome MV3 extension (~400 LoC JS)

Release uses `lto = "fat"` and `codegen-units = 1`. Both binaries are
~2-3 MB.

Daemon logs: `~/Library/Logs/lily/lilyd.err.log`.
