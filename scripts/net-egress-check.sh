#!/usr/bin/env bash
# net-egress-check.sh —— 指纹/通过率采样前的出口前置校验（只读，不改任何网络配置）
#
# 【由来】2026-10-26 查明本机有两条出口，且都不是国内家宽直连：
#   ① 系统默认 = Cloudflare WARP（warp-svc + nft 表 cloudflare-warp + ip rule 65743），
#      Cloudflare 自家 trace 直接标 `warp=on` —— 从它出去打 CF 站点等于「自己测自己」；
#   ② mihomo（127.0.0.1:7897 / :1080，mode:rule 但规则全量走节点）出口 = 美国加州洛杉矶。
#   以往所有指纹与 CF 通过率实测都走 ②，所以「放行率」不可外推到国内真实网络。
#
# 【用法】采样前跑：  scripts/net-egress-check.sh [代理URL]
#   不带参数 = 验系统默认路径；带参数（如 http://127.0.0.1:7897）= 验该代理路径。
#   输出一行 KEY=VAL，可直接并进采集 meta。
#
# 【铁律】只报不动：本脚本绝不 warp-cli disconnect、绝不改路由、绝不改 mihomo 配置。
set -u

PXY="${1:-}"
TRACE_URL="https://www.cloudflare.com/cdn-cgi/trace"

fetch() {  # $1=url  $2=超时
  if [ -n "$PXY" ]; then
    timeout "${2:-12}" curl -sS --proxy "$PXY" "$1" 2>/dev/null
  else
    timeout "${2:-12}" curl -sS --noproxy '*' "$1" 2>/dev/null
  fi
}

t="$(fetch "$TRACE_URL" 12)"
if [ -z "$t" ]; then
  echo "EGRESS=unreachable  说明:trace 取不到，采样前必须先把出口弄通"
  exit 1
fi

warp=$(printf '%s\n' "$t" | awk -F= '/^warp=/{print $2}')
colo=$(printf '%s\n' "$t" | awk -F= '/^colo=/{print $2}')
loc=$(printf '%s\n' "$t"  | awk -F= '/^loc=/{print $2}')
ip=$(printf '%s\n' "$t"   | awk -F= '/^ip=/{print $2}')
# 国内归属另查（ipip 对国内/国外判定更贴运营商视角）
cnloc=$(fetch "https://myip.ipip.net" 10 | head -1)

verdict=""
case "$warp" in
  on)  verdict="❌ CF 自家隧道（warp=on）——CF 通过率数据一律作废，不可用于 M3 关口";;
  off) if [ "$loc" = "CN" ]; then
         verdict="✅ 国内出口——可用于「国内口径」采样"
       else
         verdict="⚠️ 海外出口（loc=$loc colo=$colo）——通过率只作海外口径，不得代表国内真实网络"
       fi;;
  *)   verdict="⚠️ warp 字段异常（$warp），人工确认后再采样";;
esac

echo "EGRESS_PATH=$([ -n "$PXY" ] && echo "proxy:$PXY" || echo "system-default")"
echo "WARP=$warp  COLO=$colo  LOC=$loc  IP=<已脱敏，入库时替换>"
echo "IP_RAW=$ip"
echo "国内视角归属: ${cnloc:-查询失败}"
echo "判定: $verdict"
