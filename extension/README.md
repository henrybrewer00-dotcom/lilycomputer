# Lily Computer — Chrome extension

Companion extension that lets Lily drive Chrome from your terminal.
The extension's service worker keeps a WebSocket open to `lilyd`
(`ws://127.0.0.1:7777/ws/chrome`) and responds to commands like
`navigate`, `click`, `type`, `screenshot`, `read_page`, `query`,
`scroll`, `tabs`.

## Install (one-time, on the worker user)

1. Open Chrome on the user where `lilyd` runs (e.g. `rhettbrewer`).
2. Visit `chrome://extensions`.
3. Toggle **Developer mode** on (top right).
4. Click **Load unpacked**.
5. Pick the `extension/` directory inside this repo
   (`/Users/stevebrewer/LilyComputer/extension`).
6. You should see "Lily Computer" appear in the list. Pin it for visibility.

The service worker connects automatically. Click the extension icon →
**Options** to see live status. The dot is green when `lilyd` is
reachable and the bridge is connected.

## How it works

```
lily TUI  →  HTTP   →  lilyd  →  ws://127.0.0.1:7777/ws/chrome  →  this extension
                          ↑                                            │
                          └─── {id, ok, summary, ...} ─────────────────┘
```

Commands route via the agent loop's `browser_*` tools. The
extension calls `chrome.tabs.captureVisibleTab`,
`chrome.scripting.executeScript`, etc. Page interactions happen
inside Chrome's process and don't depend on the macOS display
compositor — so vision works even on backgrounded Fast-User-Switched
sessions.

## Debugging

- Service worker logs: `chrome://serviceworker-internals` → find
  "Lily Computer" → click **Inspect**.
- Daemon logs: `~/Library/Logs/lily/lilyd.err.log`.
- Force reconnect: extension options page → **Reconnect now**, or
  reload the extension at `chrome://extensions`.

## Security note

The bridge endpoint `/ws/chrome` is **unauthenticated** (token check
is bypassed for this route). That's safe in practice because:

- `lilyd` binds `127.0.0.1` only — not reachable off-machine.
- Only one extension connection is held at a time; the latest
  connection wins.

If you want stricter pairing, add a token check in `background.js`
(read from `chrome.storage.local`) and re-enable the check in
`crates/lilyd/src/auth.rs`.
