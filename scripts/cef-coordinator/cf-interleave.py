#!/usr/bin/env python3
"""CEF 与真 Chrome 交错跑 Cloudflare 实站对照（F-CEF-05 第二轮）。

两侧都直连 page 的 WebSocket（不传 sessionId），同一段采集代码；每次访问前清 cookie+缓存，
等 load 后固定 8s 再取状态（Turnstile 挂载窗口），仍像挑战页则再等 12s 复查。
用法: cf-interleave.py <outdir> <cef_port> <chrome_port>
"""
import asyncio, json, os, sys, time, urllib.request
import websockets

OUT, CEF_PORT, CHROME_PORT = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
SITES = [("nodeseek", "https://www.nodeseek.com/", 3), ("chatgpt", "https://chatgpt.com/", 3),
         ("nowsecure", "https://nowsecure.nl/", 3), ("indeed", "https://www.indeed.com/", 3),
         ("linux.do", "https://linux.do/", 6)]

STATE_JS = r"""(() => {
  const t = document.title || '';
  const b = (document.body && document.body.innerText || '');
  const html = document.documentElement.outerHTML;
  return JSON.stringify({
    url: location.href, title: t, body_head100: b.slice(0, 100), body_len: b.length,
    has_challenge_platform: /challenge-platform/i.test(html),
    has_turnstile_script: /challenges\.cloudflare\.com\/turnstile/i.test(html),
    has_cf_iframe: !!document.querySelector('iframe[src*="challenges.cloudflare.com"]'),
    title_is_jam: /just a moment|请稍候|attention required|access denied|verify you are human|checking your browser|un instant/i.test(t)
  });
})()"""


class Side:
    def __init__(self, name, port):
        self.name, self.port, self.mid = name, port, 0

    async def connect(self):
        tabs = json.loads(urllib.request.urlopen(f"http://127.0.0.1:{self.port}/json", timeout=6).read())
        page = next(p for p in tabs if p.get("type") == "page")
        self.ws = await websockets.connect(page["webSocketDebuggerUrl"], max_size=2 ** 25)
        self.events = []
        self.reader = asyncio.create_task(self._read())
        self.pending = {}
        for d in ("Page", "Network", "Runtime"):
            await self.call(f"{d}.enable")

    async def _read(self):
        async for m in self.ws:
            d = json.loads(m)
            if "id" in d and d["id"] in self.pending:
                self.pending.pop(d["id"]).set_result(d)
            else:
                self.events.append(d)

    async def call(self, method, params=None, timeout=45):
        self.mid += 1
        fut = asyncio.get_event_loop().create_future()
        self.pending[self.mid] = fut
        await self.ws.send(json.dumps({"id": self.mid, "method": method, "params": params or {}}))
        r = await asyncio.wait_for(fut, timeout)
        if "error" in r:
            raise RuntimeError(f"{method}: {r['error']}")
        return r.get("result", {})

    async def visit(self, url):
        await self.call("Network.clearBrowserCookies")
        await self.call("Network.clearBrowserCache")
        await self.call("Page.navigate", {"url": "about:blank"})
        await asyncio.sleep(0.5)
        self.events.clear()
        t0 = time.time()
        await self.call("Page.navigate", {"url": url})
        loaded, end = False, time.time() + 30
        while time.time() < end and not loaded:
            await asyncio.sleep(0.5)
            loaded = any(e.get("method") == "Page.loadEventFired" for e in self.events)
        docs = []
        for e in self.events:
            if e.get("method") == "Network.responseReceived" and e["params"].get("type") == "Document":
                r = e["params"]["response"]
                h = {k.lower(): v for k, v in r.get("headers", {}).items()}
                docs.append({"url": r["url"], "status": r["status"], "cf_mitigated": h.get("cf-mitigated"),
                             "cf_ray": h.get("cf-ray")})
        await asyncio.sleep(8)
        s8 = await self.state()
        s20 = None
        if s8 and (s8["title_is_jam"] or s8["has_cf_iframe"] or s8["body_len"] == 0):
            await asyncio.sleep(12)
            s20 = await self.state()
        return {"loaded": loaded, "elapsed_s": round(time.time() - t0, 1), "docs": docs,
                "state_8s": s8, "state_20s": s20}

    async def state(self):
        try:
            r = await self.call("Runtime.evaluate", {"expression": STATE_JS, "returnByValue": True})
            return json.loads(r["result"]["value"])
        except Exception as e:
            return {"error": repr(e), "title_is_jam": False, "has_cf_iframe": False, "body_len": -1}


def classify(res):
    fin = res["state_20s"] or res["state_8s"]
    if not fin or "error" in fin:
        return "error"
    if fin["title_is_jam"] and not fin["has_cf_iframe"] and fin["body_len"] < 200 and res["state_20s"]:
        return "hard_block_or_stuck"
    if fin["has_cf_iframe"] or fin["title_is_jam"]:
        return "turnstile_or_challenge_shown"
    first = res["docs"][0] if res["docs"] else {}
    if first.get("cf_mitigated") and res["state_20s"]:
        return "pass_after_challenge"
    return "pass"


async def main():
    os.makedirs(OUT, exist_ok=True)
    sides = [Side("cef", CEF_PORT), Side("chrome", CHROME_PORT)]
    for s in sides:
        await s.connect()
    plan = []
    rounds = max(n for _, _, n in SITES)
    for i in range(rounds):
        for name, url, n in SITES:
            if i < n:
                plan.append((name, url, i + 1))
    results, t_start = [], time.time()
    for name, url, run in plan:
        order = sides if run % 2 else sides[::-1]  # 每轮交换先后手
        for s in order:
            try:
                res = await s.visit(url)
            except Exception as e:
                res = {"error": repr(e), "state_8s": None, "state_20s": None, "docs": []}
            res.update(browser=s.name, site=name, url=url, run=run, at=time.strftime("%F %T"))
            res["verdict"] = classify(res) if not res.get("error") else "error"
            results.append(res)
            print(f'{res["at"]} {s.name:6} {name:9} #{run} {res["verdict"]:28} '
                  f'{(res["docs"][0]["status"] if res["docs"] else "-")} '
                  f'{(res["state_8s"] or {}).get("title", "")[:30]}', flush=True)
            json.dump({"started": time.strftime("%F %T", time.localtime(t_start)), "results": results},
                      open(f"{OUT}/cf-raw.json", "w"), ensure_ascii=False, indent=1)
    print("DONE total_min", round((time.time() - t_start) / 60, 1))


asyncio.run(main())
