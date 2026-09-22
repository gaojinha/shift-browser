-- 证书管理 / 监测 / 验证（三大件 · 2026-09-22）
--
-- certificates      ：每个 host:port 的证书快照（指纹/主题/签发者/有效期/SAN/链）
--                     指纹变更 → 记 change_count 并写 cert_events（疑似中间人）
-- cert_exceptions   ：用户放行的证书例外（"继续前往"，按 host+port+指纹锁定）
-- cert_events       ：事件流（临期 / 变更 / 校验失败 / 用户放行）

CREATE TABLE IF NOT EXISTS certificates (
  host                TEXT    NOT NULL,
  port                INTEGER NOT NULL DEFAULT 443,
  fingerprint_sha256  TEXT    NOT NULL,
  subject             TEXT,
  issuer              TEXT,
  not_before          TEXT,
  not_after           TEXT,
  dns_names           TEXT,
  ip_addresses        TEXT,
  errors              INTEGER NOT NULL DEFAULT 0,   -- TlsCertificateFlags 位图
  chain_json          TEXT,                          -- 证书链（issuer_name 数组）
  first_seen          TEXT    NOT NULL DEFAULT (datetime('now')),
  last_seen           TEXT    NOT NULL DEFAULT (datetime('now')),
  last_checked        TEXT,
  change_count        INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (host, port)
);

CREATE TABLE IF NOT EXISTS cert_exceptions (
  host                TEXT    NOT NULL,
  port                INTEGER NOT NULL DEFAULT 443,
  fingerprint_sha256  TEXT    NOT NULL,
  added_at            TEXT    NOT NULL DEFAULT (datetime('now')),
  expires_at          TEXT,
  reason              TEXT,
  PRIMARY KEY (host, port, fingerprint_sha256)
);

CREATE TABLE IF NOT EXISTS cert_events (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  host        TEXT    NOT NULL,
  port        INTEGER NOT NULL DEFAULT 443,
  kind        TEXT    NOT NULL,                      -- expiry_warn / expired / cert_changed / verify_error / accepted / first_seen
  severity    TEXT    NOT NULL DEFAULT 'info',       -- info / warn / danger
  detail      TEXT,
  created_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_cert_events_host   ON cert_events(host, created_at);
CREATE INDEX IF NOT EXISTS idx_certificates_expiry ON certificates(not_after);
