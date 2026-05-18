# lily computer

A polished terminal app that drives Chrome (and macOS apps) for you from a
single CLI prompt. Lily takes natural-language requests, plans a sequence
of browser/OS actions, and executes them — optionally on a *second* macOS
user account via Fast User Switching, so it can work while you keep using
your computer.

```
┌─ lily computer ─────────────────── stevebrewer → rhettbrewer ● live ┐
│   __    _ __         ______                            __           │
│  / /   (_) /_  __   / ____/___  ____ ___  ____  __  __/ /____  _____│
│ / /   / / / / / /  / /   / __ \/ __ `__ \/ __ \/ / / / __/ _ \/ ___/│
│/ /___/ / / /_/ /  / /___/ /_/ / / / / / / /_/ / /_/ / /_/  __/ /    │
│\____/_/_/\__, /   \____/\____/_/ /_/ /_/ .___/\__,_/\__/\___/_/     │
│         /____/                        /_/                           │
│                                                                     │
│  ▸ open gmail and tell me my 3 most recent senders                  │
│    ● browser_navigate("https://gmail.com")              412ms       │
│    ● browser_click("hint:1")  ·  Inbox                  186ms       │
│    ● browser_read_page                                  204ms       │
│  ✓ Alice (work update) · Bob (lunch?) · Carol (invoice)             │
└─ ⏱ 3.1s · 4 tools · 2,840 tok · ⌃C cancel · ⌃R clear · /help ──────┘
```

## Quick install (one command)

```bash
curl -fsSL https://raw.githubusercontent.com/henrybrewer00-dotcom/lilycomputer/main/setup.sh | bash
```

That installs Rust if missing, clones the repo, builds the binaries, installs the
daemon as a LaunchAgent, and prints the remaining 3 manual steps (Groq key +
Chrome extension load + daemon restart). Total time: ~90 seconds.

## Have an AI agent set it up for you

If you're using Claude Code, Cursor, or any other coding agent in your
terminal, paste this prompt and let it run:

> **Set up Lily Computer on this Mac.**
>
> Lily Computer is a terminal-based AI agent at <https://github.com/henrybrewer00-dotcom/lilycomputer>. It uses Groq (`meta-llama/llama-4-scout-17b-16e-instruct`, `service_tier: on_demand`) and a Chrome extension to control my browser from a CLI.
>
> Do the following:
>
> 1. Run the one-line installer in my home directory:
>    ```bash
>    curl -fsSL https://raw.githubusercontent.com/henrybrewer00-dotcom/lilycomputer/main/setup.sh | bash
>    ```
>    It installs Rust if needed, clones to `~/lilycomputer`, builds, and installs the daemon as a LaunchAgent.
>
> 2. Ask me for my Groq API key (I can get one at <https://console.groq.com/keys>). Write it to `~/.lily/env` in the format `GROQ_API_KEY=gsk_...` with mode `600`. Do NOT echo the key into shell history or commit it.
>
> 3. Tell me to open `chrome://extensions` on this Mac, toggle Developer mode, click *Load unpacked*, and select `~/lilycomputer/extension`. Wait for me to confirm.
>
> 4. Restart the daemon so it picks up the new key:
>    ```bash
>    launchctl kickstart -k gui/$(id -u)/computer.lily.daemon
>    ```
>
> 5. Run `~/lilycomputer/scripts/doctor.sh`. All four checks should report green: Screen Recording, Automation, Accessibility, and Chrome extension. If any are red, follow the script's on-screen instructions — most permission failures need a one-time grant in System Settings.
>
> 6. Confirm by running `lily --once "what's on my screen right now"`. If it returns a description, we're done. Tell me to just run `lily` (or the `lc` shortcut) any time.

## How it works

```
   ┌─ your terminal ──────────────┐         ┌─ Chrome (any user) ──────────┐
   │                              │         │                              │
   │  lily (TUI, ratatui)         │  HTTP   │   Lily extension (MV3)       │
   │   │  POST /run "do X"     ───┼─────►   │    │ WebSocket service worker│
   │   │  GET  /stream (SSE)   ◄──┼────     │    │                         │
   │   ▼                          │  SSE    │    ▼                         │
   │  events streamed live        │         │   chrome.tabs.*              │
   └──────────────────────────────┘         │   chrome.scripting.*         │
                  │                         │   chrome.tabs.captureVisibleTab │
                  ▼                         └──────────────────────────────┘
   ┌─ lilyd (LaunchAgent daemon) ────────────────┐         ▲
   │                                              │         │
   │  axum + tokio + reqwest                      │         │ ws://127.0.0.1:7777/ws/chrome
   │  • agent loop calls Groq with tool schema    │  ──────► auto-attaches {url, title, hints}
   │  • dispatches tools → bridge or macOS APIs   │           after every action
   │  • streams events back over SSE              │
   │                                              │
   │   tool dispatch:                             │
   │     OS tools  → screencapture, osascript,    │
   │                 System Events AX tree, shell │
   │     Browser   → ws://127.0.0.1:7777/ws/chrome│
   └──────────────────────────────────────────────┘
```

Two paths to act on the world:

- **Browser tools** (`browser_navigate`, `browser_click("hint:N")`, `browser_type`,
  `browser_read_page`, `browser_query`, `browser_screenshot`, `browser_wait_for`,
  `browser_back`, `browser_forward`, `browser_reload`, `browser_scroll`, `browser_tabs`,
  `browser_switch_tab`) talk to the Chrome extension over a WebSocket.
  Every state-changing call returns `{url, title, hints}` — a numbered list of
  every visible interactive element on the page — so the LLM never has to guess
  CSS selectors.
- **macOS tools** (`screenshot`, `click`, `type_text`, `key_press`, `open_app`,
  `applescript`, `read_ui` / `get_text` / `click_element` for the accessibility
  tree, `shell`, `read_file`, `list_dir`) drive native apps directly.

## Why a second user account is interesting

Fast User Switching lets a daemon stay alive in a backgrounded session.
With Lily's Chrome extension, the *browser* in that session is fully driveable
(screenshots, clicks, DOM reads) because they happen inside Chrome's process,
not through the macOS display compositor. So you can ask Lily to "go scroll
through X and find me ML hackathons" on the other user account, and stay
productive on yours.

(Native macOS app screenshots of a backgrounded session don't work on Tahoe —
that's a system limitation, not a Lily one. The AX-tree tools work fine.)

## Slash commands

Inside the TUI:

| Command | Effect |
|---|---|
| `/help` | List commands |
| `/clear` | Wipe screen + reset Lily's memory |
| `/view` | Open latest screenshot in Preview (or press `v` with empty input) |
| `/autoview` | Toggle: open every new screenshot automatically |
| `/diagnose` | Ask daemon to self-test perms + extension connection |
| `/exit` | Quit |
| `⌃C` | Cancel current run (or quit if idle) |
| `⌃R` | Same as `/clear` |
| `↑` / `↓` | Prompt history |

## CLI flags

```bash
lily                      # interactive TUI
lily "do something"       # runs the prompt at startup, then stays open
lily --once "do X"        # runs and exits (good for scripts)
lily --help               # banner + usage
lc                        # symlink alias
```

## Permissions

The macOS-side tools need three TCC grants on the worker user (one-time, only
if you want native control beyond Chrome):

- **Screen Recording** — for `screencapture` (foreground only on Tahoe)
- **Automation → System Events** — for `applescript`
- **Accessibility (granted to `osascript`)** — for clicks/keystrokes via System Events

The Chrome extension doesn't need any of these.

Run `./scripts/doctor.sh` — it tests all four (incl. extension connection)
and opens the right Privacy panes if anything's missing.

## Repo layout

```
lilycomputer/
├── setup.sh                 one-step installer (or curl|bash)
├── crates/
│   ├── lily/                TUI client (ratatui)
│   ├── lilyd/               daemon (axum + WebSocket bridge)
│   ├── lily-core/           shared types
│   └── lily-actions/        macOS tool implementations
├── extension/               Chrome MV3 extension
│   ├── manifest.json
│   ├── background.js        service worker — WebSocket + chrome.* dispatch
│   ├── options.html         live status page
│   └── options.js
├── scripts/
│   ├── install.sh           per-mode installer (client / daemon / both)
│   ├── doctor.sh            diagnostics + permission opener
│   ├── grant-perms.md       manual permission walkthrough
│   ├── uninstall.sh
│   └── mock_extension.py    fake extension for testing without Chrome
├── assets/
│   └── launchagent.plist.tmpl
├── README.md                this file
├── SETUP.md                 detailed setup notes
└── LICENSE                  MIT
```

## See also

- `SETUP.md` — detailed, step-by-step manual setup
- `scripts/grant-perms.md` — fix permission issues
- `extension/README.md` — extension internals

## License

MIT — see `LICENSE`.
