#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
fc21-matrix.py —— F-CEF-21 面板互斥矩阵 真机 9×8=72 对全表验收

对着**运行中的 CEF 工具栏**（独立实例 + CDP）逐对验证：
  ① 开 A        → 根类只剩 A-open，高度 == 122 + A_extra
  ② 不关 A 再开 B → 根类只剩 B-open，高度 == 122 + B_extra
两层都必须动：JS 侧 `removePanelClasses` 管 class，Rust 侧 `clear_all_extras_except` 管撑高。
所以触发一律走**面板自己的 setter**（setter 内既加 class 又 `act()` 通知 Rust），
不许直接改 class——那样只测了半个矩阵。

用法：
  fc21-matrix.py --port 9421 --url-sub toolbar.html --out .ai/evidence/<dir> [--only A->B,...] [--max 1.5]

退出码：0=72/72 全过；1=存在失败对；2=入口探测失败/连不上 CDP。
"""
import argparse
import json
import sys
import time
import urllib.request

import websockets

# 面板名 -> (开表达式, 关表达式, 期望 extra_px)
# 期望值以 src-tauri/src/engine/cef/shell.rs 的 Panel::ALL / extra_px 为准（2026-10-27 c74904a）
PANELS = {
    "pw":       ("setPwPanel(true)",       "setPwPanel(false)",        260),
    "dl":       ("setDlPanel(true)",       "setDlPanel(false)",        300),
    "menu":     ("setMenuPanel(true)",     "setMenuPanel(false)",      340),
    "hist":     ("setHistPanel(true)",     "setHistPanel(false)",      340),
    "site":     ("setSitePanel(true)",     "setSitePanel(false)",      150),
    "settings": ("setSettingsPanel(true)", "setSettingsPanel(false)",  320),
    "bmmanage": ("setBmManage(true)",      "setBmManage(false)",       380),
    "find":     ("window.__shiftFindOpen()",  "window.__shiftFindClose()", 44),
    "cert":     ("setCertPanel(true)",     "setCertPanel(false)",      360),
}
BASE_H = 122  # 根态收起高度

# 9 个 setter 必须都在全局可达（toolbar.html 的 <script> 是顶层非 IIFE）
PROBE_JS = "JSON.stringify(%s)" % json.dumps(
    {n: "(typeof %s)" % (p[0].split("(")[0].replace("window.", "")) for n, p in PANELS.items()}
    if False
    else {
        "pw": "typeof setPwPanel",
        "dl": "typeof setDlPanel",
        "menu": "typeof setMenuPanel",
        "hist": "typeof setHistPanel",
        "site": "typeof setSitePanel",
        "settings": "typeof setSettingsPanel",
        "bmmanage": "typeof setBmManage",
        "find": "typeof window.__shiftFindOpen",
        "cert": "typeof setCertPanel",
    }
)
READ_JS = ("[document.documentElement.className.split(' ').filter(Boolean),"
           " document.documentElement.getBoundingClientRect().height]")
CLEAR_JS = ";".join(p[1] for p in PANELS.values()) + ";'cleared'"


class Toolbar:
    """一条持久 CDP 连接，省掉每对重连的开销。"""

    def __init__(self, port, url_sub):
        tabs = json.loads(urllib.request.urlopen(
            "http://127.0.0.1:%d/json" % port, timeout=8).read())
        page = next((t for t in tabs if url_sub in t.get("url", "")
                     or url_sub in t.get("title", "")), None)
        if not page:
            raise RuntimeError("找不到工具栏 target，现有：%s"
                               % [t.get("url") for t in tabs])
        self.ws_url = page["webSocketDebuggerUrl"]
        self.ws = None
        self._id = 0

    async def open(self):
        self.ws = await websockets.connect(self.ws_url, max_size=2 ** 24)

    async def close(self):
        if self.ws:
            await self.ws.close()

    async def ev(self, expr):
        """Runtime.evaluate，返回 byValue 结果；异常抛出不吞。"""
        self._id += 1
        mid = self._id
        await self.ws.send(json.dumps({
            "id": mid, "method": "Runtime.evaluate",
            "params": {"expression": expr, "returnByValue": True,
                       "awaitPromise": False}}))
        deadline = time.time() + 10
        while time.time() < deadline:
            msg = json.loads(await self.ws.recv())
            if msg.get("id") != mid:
                continue
            if "error" in msg:
                raise RuntimeError("CDP 错误：%s" % msg["error"])
            res = msg.get("result", {}).get("result", {})
            if res.get("subtype") == "error":
                raise RuntimeError("JS 抛错：%s" % res.get("description"))
            return res.get("value")
        raise RuntimeError("CDP 超时（10s）未返回 id=%d" % mid)

    async def settle(self, max_wait=1.5):
        """等稳态：高度连续两次读数一致才认，避免读到过渡值。返回最后一次 (cls, h)。"""
        prev = None
        t0 = time.time()
        while time.time() - t0 < max_wait:
            cur = await self.ev(READ_JS)
            cls, h = cur[0], int(cur[1])
            if prev is not None and prev[1] == h:
                return cls, h
            prev = (cls, h)
            await asyncio_sleep(0.12)
        return prev[0], prev[1]


async def asyncio_sleep(sec):
    import asyncio
    await asyncio.sleep(sec)


async def run(args):
    tb = Toolbar(args.port, args.url_sub)
    await tb.open()

    # ---- 入口探测：9 个 setter 全可达才开跑，否则 exit 2（别拿半个矩阵充数）----
    probe = json.loads(await tb.ev(PROBE_JS))
    missing = {k: v for k, v in probe.items() if v != "function"}
    if missing:
        print("❌ 面板入口不可达，停止（不跑假全表）：%s" % missing, file=sys.stderr)
        await tb.close()
        return 2

    # ---- 基准高度实读（不许硬编期望）----
    await tb.ev(CLEAR_JS)
    base_cls, base_h = await tb.settle(args.max)
    print("基准：cls=%s h=%d（期望 %d）" % (base_cls, base_h, BASE_H))
    base = base_h  # 用实测基准算期望，代码常量另作对照

    names = list(PANELS.keys())
    pairs = [(a, b) for a in names for b in names if a != b]

    if args.only:
        want = set()
        for spec in args.only.split(","):
            a, b = spec.split("->")
            want.add((a.strip(), b.strip()))
        pairs = [p for p in pairs if p in want]

    rows = []
    fails = []
    for a, b in pairs:
        await tb.ev(CLEAR_JS)
        await tb.settle(args.max)

        # ① 开 A
        await tb.ev(PANELS[a][0])
        cls_a, h_a = await tb.settle(args.max)
        exp_cls_a, exp_h_a = ["%s-open" % a], base + PANELS[a][2]
        ok_a = (cls_a == exp_cls_a and h_a == exp_h_a)

        # ② 不关 A，直接开 B（核心：交叉叠加是否被清干净）
        await tb.ev(PANELS[b][0])
        cls_b, h_b = await tb.settle(args.max)
        exp_cls_b, exp_h_b = ["%s-open" % b], base + PANELS[b][2]
        ok_b = (cls_b == exp_cls_b and h_b == exp_h_b)

        row = {
            "a": a, "b": b,
            "exp_a_cls": exp_cls_a, "exp_a_h": exp_h_a,
            "got_a_cls": cls_a, "got_a_h": h_a, "pass_a": ok_a,
            "exp_cls": exp_cls_b, "exp_h": exp_h_b,
            "got_cls": cls_b, "got_h": h_b, "pass": ok_b,
            "pass_both": ok_a and ok_b,
            "ts": time.strftime("%F %T"),
        }
        rows.append(row)
        if not (ok_a and ok_b):
            fails.append(row)
            print("FAIL %-9s->%-9s A:cls=%s/h=%d(exp %d) B:cls=%s/h=%d(exp %d)"
                  % (a, b, cls_a, h_a, exp_h_a, cls_b, h_b, exp_h_b))
        else:
            print("ok   %-9s->%-9s h=%d" % (a, b, h_b))

    # ---- 落盘 ----
    out = args.out
    import os
    os.makedirs(out, exist_ok=True)
    with open(os.path.join(out, "matrix-9x8.jsonl"), "w", encoding="utf-8") as f:
        for r in rows:
            f.write(json.dumps(r, ensure_ascii=False) + "\n")
    summary = {
        "total": len(rows), "pass": sum(1 for r in rows if r["pass_both"]),
        "fail": len(fails), "base_h_measured": base, "base_h_expected": BASE_H,
        "pairs": [[r["a"], r["b"]] for r in fails],
        "cmd": " ".join(sys.argv), "ts": time.strftime("%F %T"),
    }
    with open(os.path.join(out, "matrix-summary.json"), "w", encoding="utf-8") as f:
        json.dump(summary, f, ensure_ascii=False, indent=2)
    await tb.close()

    print("\n合计 %d/%d 通过；失败对：%s"
          % (summary["pass"], summary["total"],
             "无" if not fails else ["%s->%s" % (a, b) for a, b in summary["pairs"]]))
    return 0 if not fails else 1


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, required=True)
    ap.add_argument("--url-sub", default="toolbar.html")
    ap.add_argument("--out", required=True)
    ap.add_argument("--only", help="只跑指定对，如 pw->menu,site->settings")
    ap.add_argument("--max", type=float, default=1.5, help="单步等稳态上限（秒）")
    args = ap.parse_args()
    import asyncio
    sys.exit(asyncio.run(run(args)))


if __name__ == "__main__":
    main()
