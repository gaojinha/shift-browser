#!/usr/bin/env python3
"""严格同轮对照：同一段 probe3.js 在真 Chrome 与 CEF 上的产物逐项 diff。
分类：SAME / DIFF / PROBE_ARTIFACT(CDP 副作用) / GREASE(随机，不可作判据)"""
import json, re

P = '/tmp/opencode/cef05/'
c = json.load(open(P + 'cef-layer2.json', encoding='utf-8'))['eval']
h = json.load(open(P + 'chrome-layer2.json', encoding='utf-8'))['eval']

ARTIFACT = {'webdriver'}
KNOWN = {
    'UNMASKED_RENDERER': 'GPU 后端字符串（同机同卡应一致；不一致即内核 ANGLE 配置差异）',
    'uad_high': '高熵 UA-CH（含 fullVersionList，是 brands 差异的根源）',
    'uaData': 'brands 列表',
}

def s(v):
    return json.dumps(v, ensure_ascii=False, sort_keys=True)

print('## 浏览器层 同轮严格对照（同一段 JS、同机、同时段、同一 URL）\n')
print('| 项 | 真 Chrome 154.0.8037.57 | CEF 新构建（Vulkan） | 判定 |')
print('|---|---|---|---|')
diffs = []
for k in sorted(set(c) | set(h)):
    a, b = h.get(k, '∅'), c.get(k, '∅')
    sa, sb = s(a), s(b)
    if len(sa) > 80:
        sa = sa[:77] + '…'
    if len(sb) > 80:
        sb = sb[:77] + '…'
    if sa == sb:
        v = '✅ 同'
    elif k in ARTIFACT:
        v = '⚠️ 探针假象'
    else:
        v = '❌ **差异**'
        diffs.append((k, h.get(k), c.get(k)))
    print(f'| `{k}` | {sa} | {sb} | {v} |')

# TLS/H2 层
def load_peet(path):
    d = json.load(open(path, encoding='utf-8'))
    if 'dom_head' in d:
        m = re.search(r'<pre[^>]*>(.*?)</pre>', d['dom_head'], re.S)
        t = (m.group(1) if m else d['dom_head'])
        return json.loads(t[t.find('{'):])
    for k in ('json', 'data'):
        if k in d and isinstance(d[k], dict) and 'tls' in d[k]:
            return d[k]
    return d

cj = load_peet(P + 'chrome-headed/headed-peet.json')
ej = load_peet(P + 'cef-peet.json')

def lst(j, key):
    return [x['name'] if isinstance(x, dict) else str(x) for x in j['tls'][key]]

print('\n## TLS/H2 层 对照\n')
print('| 项 | 真 Chrome | CEF | 判定 |')
print('|---|---|---|---|')
ak = lambda j: j['http2']['akamai_fingerprint'] if isinstance(j['http2']['akamai_fingerprint'], str) else s(j['http2']['akamai_fingerprint'])
rows = [
    ('JA4', cj['tls']['ja4'], ej['tls']['ja4'], ''),
    ('peetprint', cj['tls']['peetprint_hash'], ej['tls']['peetprint_hash'], ''),
    ('Akamai H2', ak(cj), ak(ej), ''),
    ('套件数', len(lst(cj, 'ciphers')), len(ej['tls']['ciphers']), ''),
    ('扩展数', len(lst(cj, 'extensions')), len(ej['tls']['extensions']), ''),
    ('JA3 hash', cj['tls']['ja3_hash'], ej['tls']['ja3_hash'], '含 GREASE 随机码，不可作判据'),
    ('GREASE 套件码', [x for x in lst(cj, 'ciphers') if 'GREASE' in x],
     [x for x in lst(ej, 'ciphers') if 'GREASE' in x], '每轮随机'),
    ('GREASE 扩展码', [x for x in lst(cj, 'extensions') if 'GREASE' in x],
     [x for x in lst(ej, 'extensions') if 'GREASE' in x], '每轮随机'),
    ('服务端收到 UA', cj['user_agent'], ej['user_agent'], ''),
]
for name, a, b, note in rows:
    sa, sb = s(a), s(b)
    if len(sa) > 62:
        sa = sa[:59] + '…'
    if len(sb) > 62:
        sb = sb[:59] + '…'
    if str(a) == str(b):
        v = '✅ 同'
    elif 'GREASE' in name or 'JA3' in name:
        v = '🎲 随机抖动（**不计差异**）'
    else:
        v = '❌ **差异**'
    print(f'| {name} | {sa} | {sb} | {v}{("｜" + note) if note else ""} |')

print('\n## 真实差异汇总（剔除随机与探针假象后）')
for k, a, b in diffs:
    print(f'- **{k}**{("（" + KNOWN[k] + "）") if k in KNOWN else ""}\n    真 Chrome: {s(a)[:220]}\n    CEF     : {s(b)[:220]}')
