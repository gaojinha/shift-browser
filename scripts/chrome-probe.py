#!/usr/bin/env python3
"""真 Chrome CDP 探针（F-CEF-05）：独立临时 profile，不碰用户正在用的 Chrome。

用法：
  chrome-probe.py fp  <outdir> [--headless]        采 tls.peet.ws / httpbin 回显 + navigator 自检
  chrome-probe.py cf  <outdir> <runs> <url>...     逐站点访问并记录 Cloudflare 挑战三态（原始证据落盘；CF_GAP=秒 控制两次访问间隔，默认 3；CHROME_EXTRA 追加启动参数）
  chrome-probe.py hdr <outdir> [--headless]        本地回显服务器采 DNT/Sec-GPC 请求头（默认 profile vs 预置 enable_do_not_track）

不联网以外的副作用：只写 outdir 与 /tmp 临时 profile。数据全部来自实测，不编。
"""
import asyncio
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request

import websockets

CHROME = os.environ.get("CHROME_BIN", "google-chrome")

CHALLENGE_JS = r"""
(() => {
  const t = document.title || '';
  const body = (document.body && document.body.innerText || '').slice(0, 600);
  const html = document.documentElement.outerHTML;
  return JSON.stringify({
    url: location.href, title: t, body_head: body,
    has_turnstile: /cf-turnstile|challenges\.cloudflare\.com\/turnstile/i.test(html),
    has_challenge_form: /id="challenge-form"|cf-chl-|_cf_chl_opt|challenge-platform/i.test(html),
    has_cf_iframe: !!document.querySelector('iframe[src*="challenges.cloudflare.com"]'),
    title_is_jam: /just a moment|请稍候|attention required|access denied|verify you are human|checking your browser/i.test(t),
    body_len: (document.body && document.body.innerText || '').length,
  });
})()
"""

NAV_JS = r"""
(async () => {
  const o = {
    ua: navigator.userAgent, webdriver: navigator.webdriver,
    languages: navigator.languages, platform: navigator.platform,
    doNotTrack: navigator.doNotTrack, gpc: navigator.globalPrivacyControl,
    plugins: navigator.plugins.length, hw: navigator.hardwareConcurrency,
  };
  if (navigator.userAgentData) {
    o.uad_brands = navigator.userAgentData.brands;
    o.uad_mobile = navigator.userAgentData.mobile;
    o.uad_platform = navigator.userAgentData.platform;
    try {
      const h = await navigator.userAgentData.getHighEntropyValues(['fullVersionList','platformVersion','architecture','bitness','model','wow64']);
      o.uad_high = h;
    } catch (e) { o.uad_high_err = String(e); }
  }
  return JSON.stringify(o);
})()
"""


class Cdp:
    def __init__(self, ws):
        self.ws, self.i, self.pending, self.events = ws, 0, {}, asyncio.Queue()

    async def pump(self):
        async for m in self.ws:
            d = json.loads(m)
            if "id" in d and d["id"] in self.pending:
                self.pending.pop(d["id"]).set_result(d)
            else:
                self.events.put_nowait(d)

    async def send(self, method, params=None, sid=None):
        self.i += 1
        fut = asyncio.get_event_loop().create_future()
        self.pending[self.i] = fut
        msg = {"id": self.i, "method": method, "params": params or {}}
        if sid:
            msg["sessionId"] = sid
        await self.ws.send(json.dumps(msg))
        r = await asyncio.wait_for(fut, 60)
        if "error" in r:
            raise RuntimeError(f"{method}: {r['error']}")
        return r.get("result", {})


class Chrome:
    def __init__(self, headless, extra=(), prefs=None):
        self.headless, self.extra = headless, list(extra) + os.environ.get("CHROME_EXTRA", "").split()
        self.prof = tempfile.mkdtemp(prefix="fp-probe-")
        if prefs:
            os.makedirs(f"{self.prof}/Default", exist_ok=True)
            json.dump(prefs, open(f"{self.prof}/Default/Preferences", "w"))

    async def start(self):
        args = [CHROME, f"--user-data-dir={self.prof}", "--remote-debugging-port=0",
                "--no-first-run", "--no-default-browser-check", "--window-size=1280,860",
                "--ozone-platform=x11"] + self.extra
        if self.headless:
            args.append("--headless=new")
        self.proc = subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        port_file = os.path.join(self.prof, "DevToolsActivePort")
        for _ in range(100):
            if os.path.exists(port_file):
                break
            time.sleep(0.2)
        port = open(port_file).read().split("\n")[0]
        ver = json.load(urllib.request.urlopen(f"http://127.0.0.1:{port}/json/version", timeout=10))
        self.version = ver["Browser"]
        self.ws = await websockets.connect(ver["webSocketDebuggerUrl"], max_size=None)
        self.cdp = Cdp(self.ws)
        self.pump = asyncio.create_task(self.cdp.pump())
        t = await self.cdp.send("Target.createTarget", {"url": "about:blank"})
        a = await self.cdp.send("Target.attachToTarget", {"targetId": t["targetId"], "flatten": True})
        self.sid = a["sessionId"]
        for d in ("Page", "Network", "Runtime"):
            await self.cdp.send(f"{d}.enable", sid=self.sid)

    async def stop(self):
        try:
            await self.cdp.send("Browser.close")
        except Exception:
            pass
        self.proc.terminate()
        try:
            self.proc.wait(10)
        except Exception:
            self.proc.kill()
        shutil.rmtree(self.prof, ignore_errors=True)

    async def drain(self):
        while not self.cdp.events.empty():
            self.cdp.events.get_nowait()

    async def goto(self, url, wait=20, settle=0):
        """导航并收集主文档响应；返回 (主文档响应信息, 事件里 Document 响应列表)。"""
        await self.drain()
        await self.cdp.send("Page.navigate", {"url": url}, self.sid)
        docs, loaded = [], False
        end = time.time() + wait
        while time.time() < end:
            try:
                ev = await asyncio.wait_for(self.cdp.events.get(), 1)
            except asyncio.TimeoutError:
                if loaded:
                    break
                continue
            if ev.get("sessionId") != self.sid:
                continue
            m, p = ev.get("method"), ev.get("params", {})
            if m == "Network.responseReceived" and p.get("type") == "Document":
                r = p["response"]
                h = {k.lower(): v for k, v in r.get("headers", {}).items()}
                docs.append({"url": r["url"], "status": r["status"], "protocol": r.get("protocol"),
                             "server": h.get("server"), "cf_ray": h.get("cf-ray"),
                             "cf_mitigated": h.get("cf-mitigated"), "frame": p.get("frameId")})
            if m == "Page.loadEventFired":
                loaded = True
        if settle:
            await asyncio.sleep(settle)
        return docs

    async def js(self, expr):
        r = await self.cdp.send("Runtime.evaluate", {"expression": expr, "awaitPromise": True,
                                                      "returnByValue": True}, self.sid)
        return r["result"].get("value")

    async def text(self):
        return await self.js("document.body ? document.body.innerText : document.documentElement.innerText")


async def cmd_fp(outdir, headless):
    os.makedirs(outdir, exist_ok=True)
    c = Chrome(headless)
    await c.start()
    try:
        tag = "headless" if headless else "headed"
        meta = {"mode": tag, "version": c.version, "time": time.strftime("%Y-%m-%d %H:%M:%S %z")}
        await c.goto("https://tls.peet.ws/api/all", 25, 1)
        open(f"{outdir}/{tag}-peet.json", "w").write(await c.text())
        await c.goto("https://httpbin.org/headers", 25, 1)
        open(f"{outdir}/{tag}-headers.json", "w").write(await c.text())
        await c.goto("https://www.cloudflare.com/cdn-cgi/trace", 25, 1)
        meta["exit_trace"] = await c.text()
        meta["navigator"] = json.loads(await c.js(NAV_JS))
        json.dump(meta, open(f"{outdir}/{tag}-meta.json", "w"), ensure_ascii=False, indent=2)
        print(json.dumps(meta, ensure_ascii=False, indent=2))
    finally:
        await c.stop()


async def cmd_cf(outdir, runs, urls):
    os.makedirs(outdir, exist_ok=True)
    c = Chrome(False)
    await c.start()
    results = []
    try:
        for url in urls:
            for n in range(1, runs + 1):
                # 每次清 cookie/缓存，保证"首访"口径
                await c.cdp.send("Network.clearBrowserCookies", sid=c.sid)
                await c.cdp.send("Network.clearBrowserCache", sid=c.sid)
                t0 = time.time()
                try:
                    docs = await c.goto(url, 30, 0)
                    # 给 JS challenge 自动通过的时间，再取页面状态
                    await asyncio.sleep(8)
                    state = json.loads(await c.js(CHALLENGE_JS))
                    # 8 秒后若仍在挑战页，再等 12 秒看是否自动放行
                    if state["title_is_jam"] or state["has_cf_iframe"]:
                        await asyncio.sleep(12)
                        state2 = json.loads(await c.js(CHALLENGE_JS))
                    else:
                        state2 = None
                    err = None
                except Exception as e:
                    docs, state, state2, err = [], None, None, repr(e)
                results.append({"url": url, "run": n, "elapsed_s": round(time.time() - t0, 1),
                                "docs": docs, "state_8s": state, "state_20s": state2, "error": err})
                print(json.dumps(results[-1], ensure_ascii=False), flush=True)
                json.dump(results, open(f"{outdir}/cf-raw.json", "w"), ensure_ascii=False, indent=1)
                await asyncio.sleep(float(os.environ.get("CF_GAP", "3")))
    finally:
        await c.stop()


ECHO_HTML = b"<html><body><pre id=o></pre></body></html>"


async def cmd_hdr(outdir, headless):
    """本地回显服务器：记录真 Chrome 默认发出的 DNT/Sec-GPC 头 + 页面里 navigator 的值。"""
    import http.server
    import threading
    os.makedirs(outdir, exist_ok=True)
    seen = {}

    class H(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            seen[self.path] = dict(self.headers.items())
            self.send_response(200)
            self.send_header("Content-Type", "text/html")
            self.end_headers()
            self.wfile.write(ECHO_HTML)

        def log_message(self, *a):
            pass

    srv = http.server.HTTPServer(("127.0.0.1", 0), H)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    port = srv.server_address[1]
    res = {}
    for label, prefs in (("default", None), ("dnt_on", {"enable_do_not_track": True})):
        c = Chrome(headless, prefs=prefs)
        await c.start()
        try:
            await c.goto(f"http://127.0.0.1:{port}/{label}", 10, 1)
            res[label] = {"version": c.version, "request_headers": seen.get(f"/{label}"),
                          "navigator": json.loads(await c.js(NAV_JS))}
        finally:
            await c.stop()
    srv.shutdown()
    for v in res.values():
        hk = {k.lower() for k in v["request_headers"]}
        v["has_dnt_header"] = "dnt" in hk
        v["has_sec_gpc_header"] = "sec-gpc" in hk
    json.dump(res, open(f"{outdir}/hdr-{'headless' if headless else 'headed'}.json", "w"), ensure_ascii=False, indent=2)
    print(json.dumps(res, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    a = sys.argv[1:]
    if len(a) >= 2 and a[0] == "fp":
        asyncio.run(cmd_fp(a[1], "--headless" in a))
    elif len(a) >= 2 and a[0] == "hdr":
        asyncio.run(cmd_hdr(a[1], "--headless" in a))
    elif len(a) >= 4 and a[0] == "cf":
        asyncio.run(cmd_cf(a[1], int(a[2]), a[3:]))
    else:
        sys.exit(__doc__)
