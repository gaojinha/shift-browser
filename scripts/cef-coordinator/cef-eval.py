#!/usr/bin/env python3
"""连 CEF 的 CDP 端口跑一段 JS，把结果连 meta 一起落盘（协调器自采，非 copilot 交付）。
用法: cef-eval.py <port> <out.json> <js表达式文件>
"""
import asyncio, json, os, socket, subprocess, sys, time
import urllib.request
import websockets

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 9333
OUT = sys.argv[2] if len(sys.argv) > 2 else "/tmp/opencode/cef05/eval.json"
JSFILE = sys.argv[3] if len(sys.argv) > 3 else "/tmp/opencode/cef05/probe.js"


def exit_ip():
    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.connect(("8.8.8.8", 80))
        local = s.getsockname()[0]
        s.close()
    except Exception:
        return "<unknown>"
    try:
        return json.loads(urllib.request.urlopen(
            "https://api.ipify.org?format=json", timeout=6).read())["ip"]
    except Exception:
        return "<unresolved>"


async def main():
    expr = open(JSFILE, encoding="utf-8").read()
    tabs = json.loads(urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json", timeout=6).read())
    pages = [t for t in tabs if t.get("type") == "page"]
    if not pages:
        print("NO PAGE TARGET", file=sys.stderr)
        return 2
    # 优先非壳页（about:/本地 shell）
    page = next((p for p in pages if p["url"].startswith("http")), pages[0])
    ws = await websockets.connect(page["webSocketDebuggerUrl"], max_size=2 ** 24)
    mid = 0

    async def call(method, params=None, sid=None, timeout=45):
        nonlocal mid
        mid += 1
        m = {"id": mid, "method": method}
        if params:
            m["params"] = params
        if sid:
            m["sessionId"] = sid
        await ws.send(json.dumps(m))
        while True:
            got = json.loads(await asyncio.wait_for(ws.recv(), timeout))
            if got.get("id") == mid:
                if "error" in got:
                    raise RuntimeError(f"{method}: {got['error']}")
                return got.get("result", {})

    # CEF 的 remote debugging 直连 page 的 ws 即该 target 会话，不能再传 sessionId
    # （实测传了报 -32001 Session with given id not found）
    sid = None

    await call("Runtime.enable", sid=sid)
    r = await call(
        "Runtime.evaluate",
        {"expression": expr, "returnByValue": True, "awaitPromise": True,
         "timeout": 20000},
        sid=sid)
    val = r.get("result", {}).get("value")
    exc = r.get("exceptionDetails")
    if exc:
        val = {"__eval_error__": str(exc.get("exception", {}))[:400]}

    doc = await call(
        "Runtime.evaluate",
        {"expression": "document.documentElement.outerHTML.slice(0, 400000)",
         "returnByValue": True}, sid=sid)
    dom = (doc.get("result", {}).get("value") or "")

    meta = {
        "collected_by": "wang-ergou（协调器自采，非 copilot 交付）",
        "collected_at": time.strftime("%F %T"),
        "binary": "src-tauri/target/debug/shift-browser (00:51, --features cef-engine)",
        "git_head": subprocess.run(["git", "-C", "/home/gao/shift-browser-cef-poc",
                                    "rev-parse", "--short", "HEAD"],
                                   capture_output=True, text=True).stdout.strip(),
        "cef_engine_flags": "--ozone-platform=x11 --use-angle=vulkan "
                            "--use-vulkan=swiftshader --enable-features=Vulkan",
        "page_target_url": page["url"],
        "cdp_browser": page.get("browser", ""),
        "exit_ip": exit_ip(),
        "note": "SwiftShader 软件光栅，非真 GPU；与真 Chrome 的 renderer 字符串必然不同",
    }
    json.dump({"meta": meta, "eval": val, "dom_head": dom[:120000]},
              open(OUT, "w", encoding="utf-8"), ensure_ascii=False, indent=1)
    await ws.close()
    print(f"已落盘 {OUT}")
    print(json.dumps(val, ensure_ascii=False, indent=1)[:3000])
    return 0


sys.exit(asyncio.run(main()))
