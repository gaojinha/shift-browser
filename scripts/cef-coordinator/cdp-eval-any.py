#!/usr/bin/env python3
"""按 URL 子串选 CDP target，执行一段 JS 并打印返回值。

为什么落库：原先只在 /tmp/opencode 下，而 /tmp 是 tmpfs（重启即失），
验收记录里的复现命令会变成空话。

用法:
    cdp-eval-any.py <port> <url子串> <js表达式>
例:
    cdp-eval-any.py 9362 toolbar.html "document.title"
    cdp-eval-any.py 9362 example.com "JSON.stringify({url:location.href})"

退出码: 0 成功；2 找不到 target；3 CDP 连接失败。
"""
import asyncio
import json
import sys
import urllib.request

import websockets


async def main() -> int:
    if len(sys.argv) < 4:
        print(__doc__)
        return 2
    port = int(sys.argv[1])
    match = sys.argv[2]
    expr = sys.argv[3]

    try:
        tabs = json.loads(
            urllib.request.urlopen(f"http://127.0.0.1:{port}/json", timeout=6).read()
        )
    except Exception as e:  # noqa: BLE001 - 命令行工具，直接报错退出
        print(f"CDP 连接失败: {e}")
        return 3

    page = next((t for t in tabs if match in t.get("url", "")), None)
    if not page:
        print("NO MATCH:", [t.get("url") for t in tabs])
        return 2

    try:
        ws = await websockets.connect(page["webSocketDebuggerUrl"], max_size=2**24)
        await ws.send(
            json.dumps(
                {
                    "id": 1,
                    "method": "Runtime.evaluate",
                    "params": {
                        "expression": expr,
                        "returnByValue": True,
                        "awaitPromise": True,
                    },
                }
            )
        )
        while True:
            msg = json.loads(await ws.recv())
            if msg.get("id") == 1:
                res = msg.get("result", {}).get("result", {})
                val = res.get("value")
                # 表达式返回字符串时直接打印，脚本侧 grep 更直观；
                # 返回对象/异常时保留完整 result（含 exceptionDetails）。
                if isinstance(val, (str, int, float, bool)):
                    print(val)
                else:
                    print(json.dumps(msg.get("result", {}), ensure_ascii=False))
                break
        await ws.close()
    except Exception as e:  # noqa: BLE001
        print(f"CDP 求值失败: {e}")
        return 3
    return 0


raise SystemExit(asyncio.run(main()))
