#!/bin/bash
# 网站兼容性批量测试：经本地 HTTP API 驱动 open_and_read 逐站读取
# 用法：./scripts/site-compat-test.sh
# 要求：浏览器已启动（API 在 127.0.0.1:19701 就绪）
TOKEN=$(cat ~/.local/share/com.shiftbrowser.app/api-token)
test_site() {
  local name="$1" url="$2"
  local start=$(date +%s)
  local resp=$(curl -s -m 60 -X POST http://127.0.0.1:19701/api -H "Authorization: Bearer $TOKEN" -d "{\"cmd\":\"open_and_read\", \"url\":\"$url\", \"close\":true, \"max_chars\":500}")
  local end=$(date +%s)
  local ok=$(echo "$resp" | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('ok'))" 2>/dev/null)
  local read_ok=$(echo "$resp" | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('data',{}).get('read_ok'))" 2>/dev/null)
  local err=$(echo "$resp" | python3 -c "import json,sys; d=json.load(sys.stdin); print(str(d.get('error',''))[:80])" 2>/dev/null)
  echo "$name | ${url} | ok=$ok read_ok=$read_ok $((end-start))s | err=$err"
}
test_site "百度-搜索" "https://www.baidu.com"
test_site "必应-搜索" "https://www.bing.com"
test_site "GitHub-代码" "https://github.com"
test_site "B站-视频" "https://www.bilibili.com"
test_site "知乎-社交" "https://www.zhihu.com"
test_site "微博-社交" "https://weibo.com"
test_site "豆瓣-社交" "https://www.douban.com"
test_site "淘宝-电商" "https://www.taobao.com"
test_site "京东-电商" "https://www.jd.com"
test_site "网易-新闻" "https://www.163.com"
test_site "澎湃-新闻" "https://www.thepaper.cn"
test_site "掘金-技术" "https://juejin.cn"
test_site "MDN-文档" "https://developer.mozilla.org"
test_site "CSDN-技术" "https://www.csdn.net"
test_site "Gitee-代码" "https://gitee.com"
