#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
ff12-mkpw.py — 给 F-CEF-12 真机验收造测试密码（协调器取证用）

铁律：
  · **只操作 --data-dir 指定的隔离库**（XDG_DATA_HOME 隔离），绝不碰真库
    ~/.local/share/com.shiftbrowser.app/。本脚本对真库路径**拒绝执行**。
  · 加密格式从源码逐行核实（src-tauri/src/db/password_crypto.rs）：
      master.key   = base64_standard(32 随机字节)，read_key_file() 只 trim+decode+长度校验
      passwords.password 列 = base64_standard( nonce(12B) || AES-256-GCM(无 AAD) 密文 )
      is_protected() 只看 vault.key 是否存在 → 隔离目录不放 vault.key，走 master.key 直读分支
  · 表列名从真库 pragma 实查（2026-10-26 13:40）：
      id / domain / username / password / notes / created_at / updated_at
    （**不是 site**，别再猜列名）

用法：
  ff12-mkpw.py --data-dir /tmp/ff12-data/com.shiftbrowser.app [--n 3] [--show-sentinel]
"""
import argparse, base64, hashlib, os, secrets, sqlite3, sys
from pathlib import Path

REAL_DB = Path.home() / ".local" / "share" / "com.shiftbrowser.app"
# 哨兵明文：DOM 断言靠它（第 2 步验"明文不进 DOM"），删不掉也不外传
SENTINEL = "S3NTINEL-pw-9f3c2b-do-not-leak"
TABLE = "passwords"
COLS = "id, domain, username, password, notes, created_at, updated_at"


def aesgcm_encrypt(key: bytes, nonce: bytes, plaintext: bytes) -> bytes:
    """AES-256-GCM，无 AAD，返回 密文||tag(16B)（与 Rust aes_gcm 的 Vec 输出一致）"""
    try:
        from cryptography.hazmat.primitives.ciphers.aead import AESGCM
    except ImportError:
        sys.exit("缺 cryptography 库：apt install python3-cryptography（或 pip）")
    return AESGCM(key).encrypt(nonce, plaintext, None)


def enc_field(key: bytes, plain: str) -> str:
    """复刻 encrypt()：base64( nonce(12) || ciphertext||tag )"""
    nonce = secrets.token_bytes(12)
    return base64.b64encode(nonce + aesgcm_encrypt(key, nonce, plain.encode())).decode()


def dec_field(key: bytes, encoded: str) -> str:
    """反向自查用：脚本自己加密的数据必须自己能解开，否则不配去测别人的解密"""
    raw = base64.b64decode(encoded)
    from cryptography.hazmat.primitives.ciphers.aead import AESGCM
    return AESGCM(key).decrypt(raw[:12], raw[12:], None).decode()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--data-dir", required=True, help="隔离数据目录（含 shift-browser.db 的那层）")
    ap.add_argument("--n", type=int, default=3, help="插入条数（含哨兵那条）")
    ap.add_argument("--show-sentinel", action="store_true", help="打印哨兵明文（只本地判读用）")
    a = ap.parse_args()

    d = Path(a.data_dir).resolve()
    # —— 硬闸：别把手伸进真库 ——
    if REAL_DB.resolve() in d.parents or d == REAL_DB.resolve():
        sys.exit(f"拒绝执行：{d} 落在真库目录内，只准用隔离目录")
    db = d / "shift-browser.db"
    if not db.exists():
        sys.exit(f"库不存在：{db}\n  ⇒ 先跑一次实例让它建表（迁移），再来插数据")

    # —— 主密钥：应用首用会自己生成 master.key；没有就我生成一个合法的 ——
    kf = d / "master.key"
    if kf.exists():
        key = base64.b64decode(kf.read_text().strip())
        if len(key) != 32:
            sys.exit(f"master.key 长度 {len(key)} != 32，与 read_key_file 要求不符")
        src = "沿用应用生成的 master.key"
    else:
        key = secrets.token_bytes(32)
        kf.write_text(base64.b64encode(key).decode())
        os.chmod(kf, 0o600)
        src = "本脚本新写 master.key（base64 32B, 0600）"

    con = sqlite3.connect(db)
    have = con.execute(f"select count(*) from {TABLE}").fetchone()[0]
    print(f"  库: {db}\n  密钥: {src}\n  现有条数: {have}")

    rows = [(SENTINEL, "sentinel.example.test", "alice"),
            ("pw-second-4471", "second.example.test", "bob"),
            ("pw-third-8890", "third.example.test", None)]
    ins = con.execute(f"select count(*) from sqlite_master where type='table' and name='{TABLE}'").fetchone()[0]
    if not ins:
        sys.exit(f"库里没有 {TABLE} 表——迁移没跑过，先起一次实例")
    for plain, domain, user in rows[: max(1, a.n)]:
        # 幂等：(domain, username) 上有唯一索引，重复插会炸，先查
        dup = con.execute(f"select id from {TABLE} where domain=? and ifnull(username,'')=?",
                          (domain, user or "")).fetchone()
        if dup:
            print(f"  跳过（已存在 id={dup[0]}）: {domain} / {user}")
            continue
        blob = enc_field(key, plain)
        # 写前先自查往返：加完密马上解回来对不对，错了就别去测别人的解密
        if dec_field(key, blob) != plain:
            sys.exit("自证失败：本脚本加密后解密不一致，格式复刻有误")
        con.execute(
            f"insert into {TABLE}(domain, username, password, notes, created_at, updated_at) "
            "values (?,?,?,?, datetime('now'), datetime('now'))",
            (domain, user, blob, "coord-test"))
        print(f"  插入: {domain} / {user} / 密文 {len(blob)} 字符")
    con.commit()
    print(f"  现条数: {con.execute(f'select count(*) from {TABLE}').fetchone()[0]}")
    if a.show_sentinel:
        print(f"  哨兵明文（本地判读，勿入台账/截图）: {SENTINEL}")
    print("  ✔ 造数据完成（真库未受影响）")


if __name__ == "__main__":
    main()
