# Lily Computer — Setup

The default and easiest path is **single-user**: run one command on the
macOS account you're already using and Lily is ready. That's all of
section 1 below.

The optional advanced path is **dual-user**: run `lilyd` and Chrome on
a separate macOS account so Lily doesn't share your screen with you.
This **requires** one Fast-User-Switch during setup because Apple
gates Privacy permissions and Chrome extension installs per-user —
neither can be done remotely with a password or any other workaround.
Once set up, you never have to switch again. That's section 2.

---

## 1 · Single-user setup (recommended)

This is the side that runs the daemon, Chrome, and the extension.
For single-user this is the only setup you need.

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

## 2 · Dual-user setup (optional, advanced)

Only do this if you specifically want Lily to drive a *separate* macOS
account on the same Mac while you keep using your normal one. There's
no way around the per-user setup wall — Apple's security model
guarantees this:

- **Privacy permissions** (Screen Recording / Automation /
  Accessibility) live in `/Library/Application
  Support/com.apple.TCC/TCC.db`, which is SIP-protected. Only the user
  clicking through System Settings in their own session can grant
  these. Not even `sudo` works.
- **Chrome unpacked extensions** can only be loaded from a foreground
  Chrome window in the destination profile. Programmatic install paths
  all require Web Store signing.

So the dual-user flow is:

1. **On the assistant account** (FUS over once): run the full installer
   `curl -L tinyurl.com/lily-get|sh`, grant the three perms, load the
   Chrome extension. Same flow as section 1.
2. **Back on your normal account**: run the client-only installer
   `curl -L tinyurl.com/lily-get|sh -s client`. ~15 seconds. Just the
   TUI binary.

After step 1, the daemon survives reboot and Fast User Switching via
LaunchAgent, so you never have to switch back unless you want to
re-configure something. Day-to-day you just `lily` on your normal
account and Lily acts in the assistant's session over loopback.

### Client install (the TUI only)

```bash
curl -L tinyurl.com/lily-get|sh -s client
```

Only ~15 seconds. Builds + installs the `lily` TUI binary (plus the `lc`
alias). No daemon, no LaunchAgent, no Chrome, no permissions.

### Verify cross-user connection

After both installers have run, on your normal account:

```bash
lily
```

You should see the banner and a green status dot in the header. If
instead you get `lily: cannot reach lilyd …`, lilyd isn't running on
the assistant — FUS over and run `~/lilycomputer/scripts/doctor.sh`
there.

### Why loopback works between users

Lily uses `127.0.0.1:7777` for all traffic. On macOS, loopback is
*machine*-scoped, not user-scoped — two users on the same Mac via FUS
can talk over it. The shared token at `/Users/Shared/lily/token` is
readable by both. (Different physical Macs would need a non-loopback
interface and real auth, which isn't supported out of the box.)

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
