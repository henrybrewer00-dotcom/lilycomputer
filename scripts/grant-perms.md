# One-time permission grants (worker user)

These must be done **once**, while logged in as the worker user (`rhettbrewer`).

> **The single most common reason "I gave permission but it doesn't work":**
> the daemon was already running when you granted the permission, so it
> still has the old (denied) state cached. After granting, you **must** restart
> the daemon:
>
> ```bash
> launchctl kickstart -k gui/$(id -u)/computer.lily.daemon
> ```
>
> Or just run `./scripts/doctor.sh` — it kickstarts for you and tells you
> exactly what's still missing.

## 1. Screen & System Audio Recording

Required for `screencapture` (Lily's "eyes").

1. **System Settings → Privacy & Security → Screen & System Audio Recording**
   (the doctor script will open this for you).
2. Click `+` and add `~/.local/bin/lilyd`. Toggle it on.
3. Kickstart the daemon (see above).

## 2. Accessibility

Required for `System Events` clicks/keystrokes.

1. **System Settings → Privacy & Security → Accessibility**.
2. Add `~/.local/bin/lilyd`. Toggle on.
3. macOS may also prompt for `osascript` — toggle that on too if you see it.
4. Kickstart the daemon.

## 3. Automation (per-app, granted on demand)

The first AppleScript that talks to Mail / Safari / Chrome etc. will prompt:
"lilyd wants to control 'X'." Click **OK**. You can audit later under
**Privacy & Security → Automation**.

## 4. Verify

```bash
./scripts/doctor.sh
```

Should print all green checkmarks. If `screencapture` returns 0 bytes,
permission #1 isn't applied. If clicks/keys do nothing, permission #2 isn't
applied.
