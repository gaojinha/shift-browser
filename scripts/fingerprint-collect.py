#!/usr/bin/env python3
"""指纹采集/对比（F-CEF-05）。输入为浏览器访问回显页后保存的 HTML/JSON，不联网、不编数。

用法：
  summarize <peet.html|json> [headers.html|json]   -> 打印归一化 JSON（去掉 ip / trace id）
  compare <baseline.json> <candidate.json>         -> 打印逐项差异表（markdown）

采集（真 Chrome 基线示例）：
  google-chrome --headless=new --no-sandbox --virtual-time-budget=8000 \
    --dump-dom https://tls.peet.ws/api/all > peet.html
  google-chrome --headless=new --no-sandbox --virtual-time-budget=8000 \
    --dump-dom https://httpbin.org/headers > headers.html
CEF 构建则在应用内打开同两个 URL，导出页面文本后喂给 summarize。
"""
import html
import json
import re
import sys

VOLATILE_HEADERS = {"X-Amzn-Trace-Id", "Host"}


def load_json(path):
    raw = open(path, encoding="utf-8").read()
    m = re.search(r"<pre[^>]*>(.*?)</pre>", raw, re.S)
    return json.loads(html.unescape(m.group(1)) if m else raw)


def summarize(peet_path, headers_path=None):
    d = load_json(peet_path)
    tls, h2 = d["tls"], d["http2"]
    out = {
        "user_agent": d.get("user_agent"),
        "http_version": d.get("http_version"),
        "ja3_hash": tls.get("ja3_hash"),
        "ja4": tls.get("ja4"),
        "peetprint_hash": tls.get("peetprint_hash"),
        "cipher_count_no_grease": len([c for c in tls.get("ciphers", []) if "GREASE" not in c]),
        "extension_count": len(tls.get("extensions", [])),
        "akamai": h2.get("akamai_fingerprint"),
        "akamai_hash": h2.get("akamai_fingerprint_hash"),
    }
    if headers_path:
        hs = load_json(headers_path)["headers"]
        out["headers"] = {k: v for k, v in hs.items() if k not in VOLATILE_HEADERS}
    return out


def compare(base, cand):
    rows = []
    for k in ("ja4", "ja3_hash", "peetprint_hash", "akamai", "cipher_count_no_grease",
              "extension_count", "user_agent"):
        a, b = base.get(k), cand.get(k)
        rows.append((k, a, b, "一致" if a == b else "差异"))
    bh, ch = base.get("headers", {}), cand.get("headers", {})
    for k in sorted(set(bh) | set(ch)):
        a, b = bh.get(k, "（缺）"), ch.get(k, "（缺）")
        rows.append((f"header:{k}", a, b, "一致" if a == b else "差异"))
    print("| 项 | 基线 | 候选 | 判定 |\n|---|---|---|---|")
    for k, a, b, v in rows:
        print(f"| {k} | `{a}` | `{b}` | {v} |")


if __name__ == "__main__":
    if len(sys.argv) >= 3 and sys.argv[1] == "summarize":
        print(json.dumps(summarize(*sys.argv[2:4]), ensure_ascii=False, indent=2))
    elif len(sys.argv) == 4 and sys.argv[1] == "compare":
        compare(load_json(sys.argv[2]), load_json(sys.argv[3]))
    else:
        sys.exit(__doc__)
