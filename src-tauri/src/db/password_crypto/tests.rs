//! password_crypto 单测：加解密往返/随机 nonce/错误密钥、
//! 主密钥 settings→文件迁移、并发首用一致、非法遗留值与损坏文件停用、文件权限 600。

use super::*;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

/// 建临时数据库（含 settings 表）
async fn temp_db() -> (tempfile::TempDir, Db) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at TEXT)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let db = Db {
        pool: pool.clone(),
        feed_pool: pool,
        key_path: dir.path().join("master.key"),
        vault_path: dir.path().join("vault.key"),
        master_key: tokio::sync::Mutex::new(None),
    };
    (dir, db)
}

#[test]
fn test_encrypt_decrypt_roundtrip() {
    let key = [7u8; 32];
    let plaintext = "MySecret123!@#";
    let encrypted = encrypt(&key, plaintext).unwrap();
    assert_ne!(encrypted, plaintext, "密文不应等于明文");
    let decrypted = decrypt(&key, &encrypted).unwrap();
    assert_eq!(decrypted, plaintext, "解密应还原明文");
}

#[test]
fn test_encrypt_random_nonce() {
    let key = [7u8; 32];
    let e1 = encrypt(&key, "same").unwrap();
    let e2 = encrypt(&key, "same").unwrap();
    assert_ne!(e1, e2, "每次加密应使用随机 nonce，密文不同");
}

#[test]
fn test_decrypt_wrong_key_fails() {
    let key1 = [1u8; 32];
    let key2 = [2u8; 32];
    let encrypted = encrypt(&key1, "secret").unwrap();
    let result = decrypt(&key2, &encrypted);
    assert!(result.is_err(), "错误密钥解密应失败");
}

#[tokio::test]
async fn test_master_key_migrates_from_settings_to_file() {
    let (_dir, db) = temp_db().await;
    // 模拟旧版本：master_key 在 settings 表
    let legacy_key = [9u8; 32];
    let encoded = BASE64.encode(legacy_key);
    sqlx::query("INSERT INTO settings (key, value) VALUES ('master_key', ?)")
        .bind(&encoded)
        .execute(&db.pool)
        .await
        .unwrap();

    // 首次调用应迁移：生成密钥文件 + 删除 settings 残留
    let key = get_master_key(&db).await.unwrap();
    assert_eq!(key, legacy_key, "迁移后应沿用旧密钥（不重新生成）");

    let file_content = std::fs::read_to_string(&db.key_path).unwrap();
    assert_eq!(file_content.trim(), encoded, "密钥文件内容应与旧值一致");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&db.key_path)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "密钥文件权限应为 600");
    }

    let leftover: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'master_key'")
            .fetch_optional(&db.pool)
            .await
            .unwrap();
    assert!(leftover.is_none(), "settings 表不应再残留 master_key");

    // 再次调用应从文件读取，结果一致
    let key2 = get_master_key(&db).await.unwrap();
    assert_eq!(key2, legacy_key);
}

#[tokio::test]
async fn test_master_key_generates_new_when_absent() {
    let (_dir, db) = temp_db().await;
    let key = get_master_key(&db).await.unwrap();
    assert_eq!(key.len(), 32);
    assert!(db.key_path.exists(), "密钥文件应已生成");
    // 无旧值时不应写 settings 表
    let leftover: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'master_key'")
            .fetch_optional(&db.pool)
            .await
            .unwrap();
    assert!(leftover.is_none());
}

/// P0-1：并发首次生成主密钥必须得到同一密钥（tokio Mutex 串行化 + create_new 原子落盘）
#[tokio::test]
async fn test_master_key_concurrent_generation_same_key() {
    let (_dir, db) = temp_db().await;
    let db = std::sync::Arc::new(db);
    let db1 = db.clone();
    let db2 = db.clone();
    let (k1, k2) = tokio::join!(async { get_master_key(&db1).await.unwrap() }, async {
        get_master_key(&db2).await.unwrap()
    },);
    assert_eq!(
        k1, k2,
        "并发首次生成必须得到同一密钥（不能各自生成互相覆盖）"
    );
    assert!(db.key_path.exists(), "密钥文件应已生成");
    let file_key = read_key_file(&db.key_path).unwrap();
    assert_eq!(k1, file_key, "文件内容应与返回密钥一致");
}

/// P1-7：settings 表遗留 master_key 非法时绝不静默重生（否则存量密码全部不可解密）
#[tokio::test]
async fn test_master_key_legacy_invalid_errors() {
    let (_dir, db) = temp_db().await;
    // 模拟旧版本：master_key 在 settings 表但格式非法（长度不对）
    sqlx::query("INSERT INTO settings (key, value) VALUES ('master_key', ?)")
        .bind("not-a-valid-32-byte-key")
        .execute(&db.pool)
        .await
        .unwrap();
    let err = get_master_key(&db).await.unwrap_err();
    assert!(
        err.to_string().contains("停用"),
        "非法遗留值应报错停用而非静默重生: {}",
        err
    );
    assert!(!db.key_path.exists(), "非法遗留值不应生成新密钥文件");
}

/// P2-5：密钥文件损坏时绝不静默覆盖（原行为：重新生成新密钥导致存量密码全废）
#[tokio::test]
async fn test_master_key_corrupt_file_errors() {
    let (_dir, db) = temp_db().await;
    std::fs::write(&db.key_path, "corrupted-content").unwrap();
    let err = get_master_key(&db).await.unwrap_err();
    assert!(
        err.to_string().contains("停用"),
        "损坏的密钥文件应报错停用而非静默覆盖: {}",
        err
    );
    assert_eq!(
        std::fs::read_to_string(&db.key_path).unwrap(),
        "corrupted-content",
        "损坏文件不应被覆盖"
    );
}

/// P3-2：密钥文件权限必须"创建即 600"，且覆盖写后收紧回 600
#[test]
fn test_write_key_file_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("master.key");

    // 1. 新建：内容写入且权限 600（OpenOptions.mode 在创建时即生效，无 644 窗口期）
    write_key_file(&path, "first-value").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "first-value");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600,
            "新建密钥文件应立即为 600"
        );
    }

    // 2. 文件已被放宽到 644 时覆盖写：内容更新且权限收紧回 600
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    write_key_file(&path, "second-value").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "second-value");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600,
            "覆盖写后应收紧回 600"
        );
    }
}
