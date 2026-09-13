#!/usr/bin/env bash
# ShiftBrowser 数据库在线备份脚本
# 用法：scripts/backup.sh [保留份数，默认14]
# 定时任务示例（每天凌晨3点）：
#   0 3 * * * /home/gao/shift-browser-tauri/scripts/backup.sh >> /tmp/sb-backup.log 2>&1
# 说明：用 python3 sqlite3 的 backup API 做在线一致性备份，
#       应用运行中（WAL 模式）也能安全备份，无需停服务。

set -euo pipefail

APP_DIR="${HOME}/.local/share/com.shiftbrowser.app"
DB="${APP_DIR}/shift-browser.db"
BACKUP_DIR="${APP_DIR}/backups"
KEEP="${1:-14}"

if [ ! -f "$DB" ]; then
  echo "[backup] 数据库不存在: $DB"
  exit 1
fi

mkdir -p "$BACKUP_DIR"
chmod 700 "$BACKUP_DIR"

TS="$(date +%Y%m%d-%H%M%S)"
OUT="${BACKUP_DIR}/shift-browser-${TS}.db"

python3 - "$DB" "$OUT" <<'PYEOF'
import sqlite3, sys
src, dst = sys.argv[1], sys.argv[2]
src_c = sqlite3.connect(src)
dst_c = sqlite3.connect(dst)
src_c.backup(dst_c)
dst_c.close()
src_c.close()
PYEOF

chmod 600 "$OUT"

# 只保留最近 KEEP 份，旧的删掉
ls -1t "${BACKUP_DIR}"/shift-browser-*.db 2>/dev/null | tail -n +$((KEEP + 1)) | xargs -r rm -f

SIZE="$(du -h "$OUT" | cut -f1)"
echo "[backup] 完成: $OUT ($SIZE)，保留 $KEEP 份"