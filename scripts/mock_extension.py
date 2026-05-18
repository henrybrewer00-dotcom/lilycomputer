#!/usr/bin/env python3
"""
Mock Lily Chrome extension.

Connects to ws://127.0.0.1:7777/ws/chrome and answers commands with fake data,
so you can test the agent loop end-to-end without installing the real extension.

Usage:
    python3 scripts/mock_extension.py

Then in another terminal, run `lily` and ask Lily to use a browser_* tool.
"""
import asyncio
import base64
import json
import sys

try:
    from websockets.asyncio.client import connect
except ImportError:
    try:
        from websockets import connect
    except ImportError:
        print("error: pip install websockets", file=sys.stderr); sys.exit(1)

# 1x1 transparent PNG, base64-encoded.
TINY_PNG_B64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkAAIAAAoAAv/lxKUAAAAASUVORK5CYII="


def handle(msg):
    cmd = msg.get("cmd")
    args = msg.get("args") or {}
    base = {"id": msg.get("id"), "ok": True}
    if cmd == "screenshot":
        return {**base, "summary": "mock 1x1 PNG", "image": f"data:image/png;base64,{TINY_PNG_B64}"}
    if cmd == "navigate":
        return {**base, "summary": f"navigated → {args.get('url','')}", "url": args.get("url",""), "title": "Mock Page"}
    if cmd == "read_page":
        return {**base, "summary": "Mock Page · 42 chars",
                "url": "https://example.com", "title": "Mock Page",
                "text": "This is a mock page.\nMost recent: Alice\nNewer: Bob"}
    if cmd == "query":
        return {**base, "summary": "2 matches",
                "matches": [{"index":0,"tag":"a","text":"Inbox","href":"https://example.com/inbox","aria_label":"Inbox","id":None,"classes":None},
                            {"index":1,"tag":"a","text":"Compose","href":"https://example.com/compose","aria_label":None,"id":None,"classes":None}]}
    if cmd == "click":
        return {**base, "summary": f"clicked {args.get('selector','')}"}
    if cmd == "type":
        return {**base, "summary": f"typed {len(args.get('text','') )} chars"}
    if cmd == "tabs":
        return {**base, "summary": "1 tab",
                "tabs":[{"id":1,"title":"Mock","url":"https://example.com","active":True,"window_id":1}]}
    if cmd == "scroll":
        return {**base, "summary": "scrolled"}
    if cmd in ("back", "forward", "reload"):
        return {**base, "summary": f"{cmd}"}
    if cmd == "wait_for":
        return {**base, "summary": f"waited for {args.get('selector','')}"}
    if cmd == "hints":
        return {**base, "summary": "2 hints",
                "hints":[{"id":1,"text":"Inbox","tag":"a","role":"link"},
                         {"id":2,"text":"Compose","tag":"button","role":"button"}]}
    return {**base, "ok": False, "summary": f"mock has no handler for {cmd}"}


async def main():
    url = "ws://127.0.0.1:7777/ws/chrome"
    print(f"mock extension → {url}")
    async with connect(url) as ws:
        print("✓ attached. waiting for commands.")
        async for raw in ws:
            try:
                msg = json.loads(raw)
            except Exception as e:
                print(f"  bad frame: {e}")
                continue
            cmd = msg.get("cmd","?")
            args_short = json.dumps(msg.get("args", {}))[:80]
            print(f"  ← {cmd}({args_short})")
            resp = handle(msg)
            print(f"  → ok={resp['ok']}  {resp.get('summary','')[:80]}")
            await ws.send(json.dumps(resp))


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        pass
