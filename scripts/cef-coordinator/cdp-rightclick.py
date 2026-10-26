#!/usr/bin/env python3
"""按 URL 子串选 CDP target，在指定坐标发一次右键（用于验页面右键菜单）。

用法:
    cdp-rightclick.py <port> <url子串> [x] [y]

说明：CDP 注入的是 renderer 层事件，能触发页面 `contextmenu` 监听；
CEF 的 Alloy 运行时不弹原生菜单，所以这是验证「内容页自绘右键菜单」的正路。
"""
import asyncio
import json
import sys
import urllib.request

import websockets


async def main() -> int:
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    port = int(sys.argv[1])
    match = sys.argv[2]
    x = int(sys.argv[3]) if len(sys.argv) > 3 else 500
    y = int(sys.argv[4]) if len(sys.argv) > 4 else 300

    try:
        tabs = json.loads(
            urllib.request.urlopen(f"http://127.0.0.1:{port}/json", timeout=6).read()
        )
    except Exception as e:  # noqa: BLE001
        print(f"CDP 连接失败: {e}")
        return 3

    page = next((t for t in tabs if match in t.get("url", "")), None)
    if not page:
        print("NO MATCH:", [t.get("url") for t in tabs])
        return 2

    try:
        ws = await websockets.connect(page["webSocketDebuggerUrl"], max_size=2**24)
        for i, (method, params) in enumerate(
            [
                (
                    "Input.dispatchMouseEvent",
                    {"type": "mousePressed", "x": x, "y": y, "button": "right", "clickCount": 1},
                ),
                (
                    "Input.dispatchMouseEvent",
                    {"type": "mouseReleased", "x": x, "y": y, "button": "right", "clickCount": 1},
                ),
            ]
        ):
            await ws.send(json.dumps({"id": i + 1, "method": method, "params": params}))
            while True:
                msg = json.loads(await ws.recv())
                if msg.get("id") == i + 1:
                    break
        await ws.close()
    except Exception as e:  # noqa: BLE001
        print(f"CDP 右键失败: {e}")
        return 3
    print("sent right click")
    return 0


raise SystemExit(asyncio.run(main()))
