#!/usr/bin/env python3
"""连 CEF 的 CDP，按 URL 关键字挑 target 求值（cef-eval.py 只会取第一个 page，
   而 CEF 的工具栏是独立 Webview（toolbar.html），验书签栏必须连它，连错=白测）。
用法: cdf-eval-target.py <port> <target关键字> <out.json> <js文件>
"""
import asyncio, json, sys, time, urllib.request
import websockets

port, want, out, jsfile = int(sys.argv[1]), sys.argv[2], sys.argv[3], sys.argv[4]
expr = open(jsfile, encoding='utf-8').read()

targets = json.load(urllib.request.urlopen(f"http://127.0.0.1:{port}/json", timeout=5))
page = [t for t in targets if t.get('type') == 'page' and want in (t.get('url') or '') + (t.get('title') or '')]
if not page:
    print("找不到含 %r 的 target，现有: %s" % (want, [t.get('url', '')[:60] for t in targets]))
    sys.exit(3)
t = page[0]
print(f"选中 target: {t.get('title')}  {t.get('url')[:70]}")


async def main():
    async with websockets.connect(t['webSocketDebuggerUrl'], max_size=40_000_000) as ws:
        n = 0

        async def send(method, params=None):
            nonlocal n
            n += 1
            await ws.send(json.dumps({"id": n, "method": method, "params": params or {}}))
            while True:
                m = json.loads(await ws.recv())
                if m.get("id") == n:
                    return m.get("result", {})

        await send("Runtime.enable")
        r = await send("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True})
        val = r.get("result", {}).get("value")
        err = r.get("exceptionDetails")
        meta = {"ts": time.strftime('%F %T%z'), "port": port, "target": t.get('url'), "target_title": t.get('title')}
        json.dump({"meta": meta, "eval": val, "exception": err}, open(out, 'w', encoding='utf-8'),
                  ensure_ascii=False, indent=1)
        print(json.dumps(val if val is not None else {"exception": err}, ensure_ascii=False, indent=1)[:1400])

asyncio.run(main())
