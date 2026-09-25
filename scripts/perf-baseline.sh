#!/usr/bin/env bash
# 性能基线采集（R1-8 · 2026-09-25）
#
# 指标：冷启动耗时 / 恢复 20 标签耗时 / 恢复 10 标签后 RSS / deb 体积
# 方法：同机重复 3 次取中位数；全程使用隔离 XDG profile（mktemp），不读写真实用户数据。
#
# 用法：
#   scripts/perf-baseline.sh                 # release 构建后跑 3 轮，报告写入 docs/test-reports/
#   scripts/perf-baseline.sh --skip-build    # 跳过 cargo build --release
#   scripts/perf-baseline.sh --runs 5 --settle 10 --out /tmp/perf.md
#
# 注意：测量会在当前图形会话真实弹出浏览器窗口（每次信号采到即自动关闭），
#       建议测量期间不要操作鼠标键盘；不要通过 SSH 无显示环境运行。
# -E/errtrace：让函数内部也继承 ERR trap，set -e 中止时能定位具体行
set -Eeuo pipefail

RUNS=3
SETTLE=8
DO_BUILD=1
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/docs/test-reports/perf-baseline-$(date +%F).md"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --runs) RUNS="$2"; shift 2 ;;
    --settle) SETTLE="$2"; shift 2 ;;
    --skip-build) DO_BUILD=0; shift ;;
    --out) OUT="$2"; shift 2 ;;
    *) echo "未知参数: $1" >&2; exit 2 ;;
  esac
done

BIN="$ROOT/src-tauri/target/release/shift-browser"
VERSION="$(grep -m1 '^version' "$ROOT/src-tauri/Cargo.toml" | sed 's/.*= *"//;s/"//')"
DEB="$ROOT/src-tauri/target/release/bundle/deb/Shift Browser_${VERSION}_amd64.deb"
READY_SIGNAL='onReady 完成 标签数='
LAUNCH_TIMEOUT=60

# 进度信息一律走 stderr：测量函数的数据走 stdout 且常被命令替换捕获，
# 进度日志混入 stdout 会污染数值（曾导致 v 变成多行日志文本）
log() { printf '\033[36m[perf]\033[0m %s\n' "$*" >&2; }

if [[ "$DO_BUILD" == 1 ]]; then
  log "cargo build --release（R1 后代码基线）"
  (cd "$ROOT/src-tauri" && cargo build --release --bin shift-browser)
fi
[[ -x "$BIN" ]] || { echo "缺少可执行文件: $BIN（先执行 tauri/cargo release 构建）" >&2; exit 1; }

# ---- 通用工具 ----

median() { # 任意个数字参数 → 中位数（奇数取中，偶数取中间两者均值，三位小数）
  printf '%s\n' "$@" | sort -n | awk '{a[NR]=$1}
    END{ if(NR%2) printf "%.3f", a[(NR+1)/2]; else printf "%.3f", (a[NR/2]+a[NR/2+1])/2 }'
}

join_vals() { # 用 " / " 拼接全部参数
  local out="" v
  for v in "$@"; do out="${out:+$out / }$v"; done
  printf '%s' "$out"
}

# 在隔离 profile 中启动应用；输出全局 APP_PID（=进程组首 pid）
# stdout/stderr 落盘到 profile：tauri-plugin-log 默认同时写 Stdout 与 LogDir，
# 而 LogDir 文件阈值仅 40KB（KeepOne，超了直接删除重建），多标签启动时信号行会随
# 滚动永久丢失；stdout 文件不滚动，是可靠的第二信号源。
start_app() { # $1=profile_dir
  local prof="$1"
  mkdir -p "$prof/data" "$prof/config" "$prof/cache"
  # 不重定向 XDG_RUNTIME_DIR/DBUS_SESSION_BUS_ADDRESS：
  # 否则丢失宿主 Wayland socket（退回 XWayland，渲染路径失真）且 single-instance 的 D-Bus 名异常。
  # 串行测量 + 每轮整组退出，共享 runtime dir 无锁冲突。
  setsid env \
    XDG_DATA_HOME="$prof/data" \
    XDG_CONFIG_HOME="$prof/config" \
    XDG_CACHE_HOME="$prof/cache" \
    "$BIN" >"$prof/stdout.log" 2>&1 &
  APP_PID=$!
}

LOG_REL='com.shiftbrowser.app/logs/Shift Browser.log'

# tauri-plugin-log 的滚动策略（默认 max_file_size 仅 40KB + KeepOne：超限直接
# 删除文件并同名重建）会在日志爆发时让信号行随旧 inode 永久消失——实测恢复 20
# 标签时文件 215→33→298→98 行反复滚动，建库首启也偶发命中。单靠 grep 轮询或
# tail 滚动文件都有漏抓窗口，因此同时监听两个源：
#   ① stdout.log（插件默认双通道之一，不滚动，可靠主源）
#   ② LogDir 滚动文件（兜底）
# 监听必须在启动应用【之前】挂好；tail -F 覆盖文件不存在/截断/重建。
SIGNAL_FIFO=
SIGNAL_WATCH_PID=
WATCH_PROF=

start_watch() { # $1=profile_dir（必须在 start_app 之前调用）
  local prof="$1" logdir
  WATCH_PROF="$prof"
  logdir="$prof/data/$(dirname "$LOG_REL")"
  mkdir -p "$logdir"
  SIGNAL_FIFO="$prof/signal.fifo"
  mkfifo -m 600 "$SIGNAL_FIFO"
  # FIFO 无读端时 tail 阻塞在 open() 不会提前消费日志；grep 打开读端后才开始读。
  # 多文件切换头（==> xxx <==）不含中文信号串，不会被 grep 误匹配；
  # 同一行两源各输出一份也无妨，grep -m1 取首份即返回。
  stdbuf -oL tail -n +1 -F \
    "$prof/stdout.log" "$prof/data/$LOG_REL" 2>/dev/null >"$SIGNAL_FIFO" &
  SIGNAL_WATCH_PID=$!
}

stop_watch() {
  if [[ -n "${SIGNAL_WATCH_PID:-}" ]]; then
    kill "$SIGNAL_WATCH_PID" 2>/dev/null || true
    wait "$SIGNAL_WATCH_PID" 2>/dev/null || true
    SIGNAL_WATCH_PID=
  fi
  if [[ -n "${SIGNAL_FIFO:-}" ]]; then rm -f "$SIGNAL_FIFO"; SIGNAL_FIFO=; fi
}

# 阻塞等待日志信号（固定字符串匹配），命中时 stdout 打印 epoch 秒；超时打印诊断并返回 1
await_signal() { # $1=pattern
  local pat="$1" line rc logf
  set +e
  line="$(timeout "$LAUNCH_TIMEOUT" grep -m1 --line-buffered -F -e "$pat" "$SIGNAL_FIFO")"
  rc=$?
  set -e
  if [[ $rc -eq 0 ]]; then
    date +%s.%N
    return 0
  fi
  # 诊断：区分"实例被 single-instance 静默杀掉"与"应用真的卡住"
  echo "[perf] 诊断：等待 [$pat] ${LAUNCH_TIMEOUT}s 超时（grep rc=$rc）" >&2
  if [[ -n "${APP_PID:-}" ]] && kill -0 "$APP_PID" 2>/dev/null; then
    echo "[perf] 诊断：应用进程 $APP_PID 仍存活（启动/恢复慢于 ${LAUNCH_TIMEOUT}s）" >&2
  else
    echo "[perf] 诊断：应用进程已退出（疑似 single-instance NameTaken 或启动崩溃）" >&2
  fi
  echo "[perf] 诊断：D-Bus 名 owner = $(gdbus call --session \
    --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus \
    --method org.freedesktop.DBus.NameHasOwner "$SI_DBUS_NAME" 2>/dev/null || echo 'gdbus不可用')" >&2
  logf="$WATCH_PROF/stdout.log"
  if [[ -f "$logf" ]]; then
    # stdout 通道不滚动，末尾 8 行能真实反映应用最后状态
    echo "[perf] 诊断：stdout 末尾内容：" >&2
    tail -8 "$logf" | sed 's/^/[perf]   /' >&2
  else
    echo "[perf] 诊断：profile 内无 stdout 日志（实例极早期退出）" >&2
  fi
  return 1
}

# 等待 single-instance 的 D-Bus 名释放：旧进程退出（尤其被 SIGKILL）后，
# dbus-daemon 清理 well-known name 有毫秒级延迟，新实例此时启动会因 NameTaken
# 被插件 std::process::exit(0) 静默杀掉（表现为等不到 onReady），必须等名字释放。
SI_DBUS_NAME='com.shiftbrowser.app.SingleInstance'
wait_name_released() {
  command -v gdbus >/dev/null || return 0
  for _ in $(seq 1 50); do
    local owned
    owned="$(gdbus call --session \
      --dest org.freedesktop.DBus \
      --object-path /org/freedesktop/DBus \
      --method org.freedesktop.DBus.NameHasOwner \
      "$SI_DBUS_NAME" 2>/dev/null || echo '(false,)')"
    [[ "$owned" == "(false,)" ]] && return 0
    sleep 0.1
  done
  log "警告：D-Bus 名 $SI_DBUS_NAME 5s 内未释放，后续测量数据不可信"
  return 1
}

stop_app() {
  [[ -n "${APP_PID:-}" ]] || return 0
  kill -TERM -- "-$APP_PID" 2>/dev/null || true
  # 等满 5s：应用 SIGTERM 后要保存"自动保存"会话，SIGKILL 打断会留下半成品状态
  for _ in $(seq 1 50); do
    kill -0 -- "-$APP_PID" 2>/dev/null || break
    sleep 0.1
  done
  if kill -0 -- "-$APP_PID" 2>/dev/null; then
    log "SIGTERM 后仍未退出，SIGKILL 清理"
    kill -KILL -- "-$APP_PID" 2>/dev/null || true
    sleep 0.5
  fi
  wait_name_released || true
  APP_PID=
  # 冷静期：让 Wayland/X 窗口销毁、WebKit 子进程回收完全落定，避免串到下一轮
  sleep 0.5
}
trap stop_app EXIT
# set -e 中止时定位具体行（曾出现 R10 段无诊断 EXIT=1 无法定位）
trap 'echo "[perf] 中止：第 ${LINENO} 行命令失败（exit $?）" >&2' ERR

# 向隔离 profile 的 sessions 表植入一条"自动保存"会话（N 个 shift://newtab 标签）。
# 关键：首启退出时应用自己也会保存一条 1 标签的"自动保存"（其提交晚于 D-Bus 名释放，
# 曾导致 seed 被覆盖、恢复出 1 个标签、"标签数=N"信号永不到达）。
# 因此先确认应用进程已退出（stop_app 已保证），再 DELETE 旧自动保存后插入并回读校验。
seed_session() { # $1=profile_dir $2=n
  python3 - "$1" "$2" <<'PY'
import json, sqlite3, sys
prof, n = sys.argv[1], int(sys.argv[2])
db = f"{prof}/data/com.shiftbrowser.app/shift-browser.db"
tabs = json.dumps([{"url": "shift://newtab", "title": None, "groupId": None} for _ in range(n)],
                  ensure_ascii=False)
con = sqlite3.connect(db, timeout=10)
try:
    con.execute("DELETE FROM sessions WHERE name = '自动保存' AND workspace_id IS NULL")
    con.execute(
        "INSERT INTO sessions (name, workspace_id, data) VALUES ('自动保存', NULL, ?)",
        (tabs,),
    )
    con.commit()
    row = con.execute(
        "SELECT json_array_length(data) FROM sessions "
        "WHERE name = '自动保存' AND workspace_id IS NULL ORDER BY id DESC LIMIT 1"
    ).fetchone()
finally:
    con.close()
if row is None or row[0] != n:
    sys.exit(f"seed 校验失败：期望最新自动保存含 {n} 个标签，实际 {row}")
PY
}

# 汇总进程整组（含 WebKitWebProcess/NetworkProcess 子进程）RSS，单位 KB
group_rss_kb() { # $1=root_pid
  ps -eo pid=,ppid=,rss= | awk -v root="$1" '
    { ppid[$1]=$2; rss[$1]=$3; children[$2]=children[$2]" "$1 }
    END {
      # 从 root 深度优先收集整棵树
      stack[++sp]=root
      while (sp>0) { p=stack[sp--]; sum+=rss[p]; n=split(children[p], c, " ");
        for (i=1;i<=n;i++) if (c[i]!="") stack[++sp]=c[i] }
      print sum
    }'
}

measure_cold_start() { # 输出单次秒数
  local prof t0 t1
  prof="$(mktemp -d "$ROOT/tmp-perf-cold-XXXX")"
  start_watch "$prof"
  t0="$(date +%s.%N)"; start_app "$prof"
  if ! t1="$(await_signal "$READY_SIGNAL")"; then
    stop_app; stop_watch; rm -rf "$prof"; return 1
  fi
  stop_app; stop_watch
  rm -rf "$prof"
  # 结尾必须带换行：消费端 read 遇"无换行 EOF"会返回非零，曾触发 set -e 误杀
  awk -v a="$t0" -v b="$t1" 'BEGIN{printf "%.3f\n", b-a}'
}

measure_restore() { # $1=tab_count $2=want_rss(0/1) → stdout: 秒数[ RSS_kb]
  local n="$1" with_rss="$2" prof t0 t1 rss=""
  prof="$(mktemp -d "$ROOT/tmp-perf-restore-XXXX")"

  # 1) 空库启动一次，让应用完成迁移建表，随即关闭
  start_watch "$prof"
  start_app "$prof"
  if ! await_signal "$READY_SIGNAL" >/dev/null; then
    stop_app; stop_watch; rm -rf "$prof"; return 1
  fi
  stop_app; stop_watch

  # 2) 植入含 N 标签的自动保存会话（内部清理旧记录并回读校验）
  seed_session "$prof" "$n"

  # 3) 再启动测恢复耗时（监听必须先于启动挂好，见 start_watch 注释）
  start_watch "$prof"
  t0="$(date +%s.%N)"; start_app "$prof"
  if ! t1="$(await_signal "$READY_SIGNAL$n")"; then
    stop_app; stop_watch; rm -rf "$prof"; return 1
  fi
  if [[ "$with_rss" == 1 ]]; then
    sleep "$SETTLE"   # 等 WebKit 各进程完成页面装载后再读稳态 RSS
    rss="$(group_rss_kb "$APP_PID")"
  fi
  stop_app; stop_watch
  rm -rf "$prof"
  # 结尾必须带换行（read 消费，无换行会误返回非零）
  awk -v a="$t0" -v b="$t1" -v r="$rss" 'BEGIN{printf "%.3f%s\n", b-a, (r==""?"":(" " r))}'
}

# 单次测量可能因桌面会话抖动/single-instance 注销竞态偶发失败：
# 清理现场后用全新 profile 重试一次；两次皆败才中止（避免假数据进报告）
measure_with_retry() {
  local attempt
  for attempt in 1 2; do
    if [[ "$1" == "cold" ]]; then
      measure_cold_start && return 0
    else
      measure_restore "$2" "$3" && return 0
    fi
    log "  第 ${attempt} 次尝试失败，全新 profile 重试"
    stop_app >/dev/null 2>&1 || true
  done
  return 1
}

# ---- 采集 ----

log "冷启动 ×$RUNS"
COLD=()
for i in $(seq 1 "$RUNS"); do
  v="$(measure_with_retry cold)"; COLD+=("$v"); log "  #$i ${v}s"
done

log "恢复 20 标签 ×$RUNS"
R20=()
for i in $(seq 1 "$RUNS"); do
  v="$(measure_with_retry restore 20 0)"; R20+=("$v"); log "  #$i ${v}s"
done

log "恢复 10 标签 ×$RUNS（settle ${SETTLE}s 后采 RSS）"
R10=(); RSS=()
for i in $(seq 1 "$RUNS"); do
  read -r v r < <(measure_with_retry restore 10 1)
  [[ -n "$v" ]] || { echo "第 $i 轮恢复 10 标签测量两轮均失败" >&2; exit 1; }
  R10+=("$v"); RSS+=("$r")
  log "  #$i ${v}s  RSS=${r}KB"
done

DEB_SIZE="N/A"
if [[ -f "$DEB" ]]; then
  DEB_SIZE="$(stat -c %s "$DEB" | awk '{printf "%.2f", $1/1048576}') MiB"
fi

# ---- 报告 ----

m_cold="$(median "${COLD[@]}")"
m_r20="$(median "${R20[@]}")"
m_r10="$(median "${R10[@]}")"
m_rss="$(median "${RSS[@]}")"
raw_cold="$(join_vals "${COLD[@]}")"
raw_r20="$(join_vals "${R20[@]}")"
raw_r10="$(join_vals "${R10[@]}")"
raw_rss="$(join_vals "${RSS[@]}")"
m_rss_mib="$(awk -v k="$m_rss" 'BEGIN{printf "%.1f", k/1024}')"
COMMIT="$(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo unknown)"
CPU="$(awk -F': ' '/model name|Hardware|Processor/{print $2; exit}' /proc/cpuinfo)"
MEM="$(awk '/MemTotal/{printf "%.1f GiB", $2/1048576}' /proc/meminfo)"

mkdir -p "$(dirname "$OUT")"
cat > "$OUT" <<EOF
# 性能基线报告（R1-8）

- 采集时间：$(date '+%Y-%m-%d %H:%M:%S %z')
- 代码版本：${VERSION}（commit ${COMMIT}，工作树含 R1 批次未提交改动）
- 机器：$(uname -srm)
- CPU：${CPU}（$(nproc) 核）
- 内存：${MEM}
- 图形会话：${XDG_SESSION_TYPE:-unknown}（DISPLAY=${DISPLAY:-无}）
- 采集脚本：\`scripts/perf-baseline.sh\`（同机 ${RUNS} 次取中位数）

## 结果

| 指标 | 全部原始值（${RUNS} 轮） | 中位数（基线） |
|---|---|---|
| 冷启动（空 profile，进程起 → onReady 完成） | ${raw_cold}s | **${m_cold}s** |
| 恢复 20 标签（shift://newtab 离线页） | ${raw_r20}s | **${m_r20}s** |
| 恢复 10 标签耗时 | ${raw_r10}s | **${m_r10}s** |
| 恢复 10 标签稳态 RSS（主+WebKit 全进程组，settle ${SETTLE}s） | ${raw_rss}KB | **${m_rss_mib} MiB** |
| deb 安装包体积（${VERSION}） | — | **${DEB_SIZE}** |

## 方法与口径

1. **隔离**：每轮使用独立 mktemp XDG profile（data/config/cache/runtime 全重定向），
   不触碰真实用户数据；轮间进程整组 SIGTERM/SIGKILL，single-instance 串行无冲突。
2. **就绪信号**：前端 \`onReady 完成 标签数=N\` 经 front_log 由 tauri-plugin-log 同时
   写入 LogDir 文件与进程 stdout；LogDir 默认阈值仅 40KB（KeepOne，超限删除重建），
   多标签恢复时信号行会随旧文件永久丢失，故脚本以【不滚动的 stdout 落盘文件】为主信号源、
   LogDir 为兜底，在每次启动前挂 \`tail -F\` 流式监听，命中即停表；
   表值 = 进程启动到该信号的墙钟时间。
3. **恢复场景**：先空库启动建表并退出（等进程组完全退出，保证其退出保存已提交），
   清除默认工作区旧的"自动保存"记录后，插入 N 个 \`shift://newtab\` 内置页的会话
   （零网络依赖、结果可复现）并回读校验标签数，再二次启动计时。
4. **RSS**：按进程树汇总主进程与全部 WebKitWebProcess/NetworkProcess 的 /proc RSS，
   onReady 后再等 ${SETTLE}s 取稳态值。
5. **局限**：内置页内存占用低于真实网页，RSS 基线用于版本间回归对比，不代表重网页场景；
   冷启动含桌面会话抖动，跨机器比较无意义，仅同机回归有效。

## 复现

\`\`\`bash
scripts/perf-baseline.sh             # 自动 release 构建 + 3 轮测量，覆盖输出本文件
scripts/perf-baseline.sh --skip-build
\`\`\`
EOF

log "报告已写入: $OUT"
printf '\n中位数：冷启动 %ss | 恢复20 %ss | 恢复10 %ss | RSS %s MiB | deb %s\n' \
  "$m_cold" "$m_r20" "$m_r10" "$m_rss_mib" "$DEB_SIZE"
