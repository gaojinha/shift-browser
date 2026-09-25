//! 启动环境变量与平台初始化（run() 最早段，任何 GTK/WebKit 初始化之前执行）

/// 读一个布尔设置（启动最早期，DB 连接池尚未建立 → 自建临时 runtime 直读 SQLite）。
/// 读不到（库不存在/表不存在/键缺失）时返回 `default`。
#[cfg(target_os = "linux")]
fn read_setting_bool(key: &str, default: bool) -> bool {
    let data_home = std::env::var("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("HOME")
                .map(|h| std::path::PathBuf::from(h).join(".local/share"))
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
        });
    let db_path = data_home.join("com.shiftbrowser.app").join("shift-browser.db");
    if !db_path.exists() {
        return default;
    }
    // 临时单线程 runtime（此时主 runtime 尚未建立）
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(_) => return default,
    };
    rt.block_on(async {
        use sqlx::sqlite::SqliteConnectOptions;
        let opts = SqliteConnectOptions::new()
            .filename(&db_path)
            .read_only(true);
        let pool = match sqlx::SqlitePool::connect_with(opts).await {
            Ok(p) => p,
            Err(_) => return default,
        };
        let row: Option<(String,)> =
            sqlx::query_as("SELECT value FROM settings WHERE key = ?")
                .bind(key)
                .fetch_optional(&pool)
                .await
                .ok()
                .flatten();
        pool.close().await;
        match row {
            Some((v,)) => v == "true",
            None => default,
        }
    })
}

/// 应用启动环境默认值 + Cookie 持久化。必须在 Builder 构建前调用。
pub(crate) fn apply() {
    // ===== WebKit DMA-BUF 线性格式兜底（P0-7，2026-09-24）=====
    // WEBKIT_DMABUF_RENDERER_BUFFER_FORMAT=XR24:0 用于规避 WebKitGTK 2.42+
    // AcceleratedBackingStore 竞态空指针崩溃（见 HANDOFF-2026-09-23 全记录）。
    // 原先只写在 .desktop 模板的 Exec= env 前缀里——从终端/IDE/MCP 客户端等
    // 非 desktop 入口启动时不生效。前移到进程启动最早处（任何 GTK/WebKit
    // 初始化之前），覆盖全部启动入口；用户显式设置过同名变量则不覆盖。
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DMABUF_RENDERER_BUFFER_FORMAT").is_none() {
        std::env::set_var("WEBKIT_DMABUF_RENDERER_BUFFER_FORMAT", "XR24:0");
        log::info!("[ShiftBrowser] 已注入 WEBKIT_DMABUF_RENDERER_BUFFER_FORMAT=XR24:0（启动入口兜底，可被环境变量覆盖）");
    }

    // ===== DMA-BUF 渲染器：**由设置控制**，默认不禁用（2026-09-26 反转）=====
    // 【2026-09-26 崩溃根因定论】WEBKIT_DISABLE_DMABUF_RENDERER=1 会使
    //   rendererBufferTransportMode() 为空 → AcceleratedBackingStore::create()
    //   返回 nullptr；微博等站点触发 enterAcceleratedCompositingMode 时，
    //   WebKitWebViewBase.cpp:2904 对空 backingStore 调 update()（release 无 ASSERT）
    //   → UI 主进程 SIGSEGV，整个浏览器退出（微博 100% 复现；不禁用则存活）。
    //   MiniBrowser 带同变量不崩仅因其测试页未进入合成模式。
    //
    // 【历史】2026-09-25 曾实测禁用 DMA-BUF 修复视频闪烁 + CPU 降 29%——
    //   但代价是加速合成路径空指针崩溃。**崩溃（P0）优先于闪烁（P2）**，
    //   故默认改回不禁用；用户仍可在「设置 → 高级 → 兼容性」手动开启
    //   （开启即接受微博类重合成站点崩溃风险）。
    //
    // 环境变量优先（用户显式设置则不覆盖）。
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        let disable = read_setting_bool("render_disable_dmabuf", false);
        if disable {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
        log::info!(
            "[ShiftBrowser] render_disable_dmabuf={} → WEBKIT_DISABLE_DMABUF_RENDERER={:?}",
            disable,
            std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER")
        );
    }

    // ===== 视频进程崩溃修复（2026-09-22）：降级硬件视频解码器 =====
    // 症状：播放某些 H.264 视频时 WebKitWebProcess 直接 abort：
    //   ** (WebKitWebProcess:PID): ERROR **: video meta uses 368x480 instead of 360x480
    // 成因：硬件解码器（V4L2 stateless / VA-API）输出帧宽按 16 字节对齐（360→368），
    // 而 WebKit 的 video meta 校验期望未对齐宽度，断言失败 → g_error() → 进程终止。
    // 规避：用 GStreamer 标准的 GST_PLUGIN_FEATURE_RANK 把硬件解码器 rank 压到 0，
    // 使其回退到软件解码（libav），彻底绕开对齐 padding 路径。
    // 用户可覆盖（机器上硬解正常、且想要 4K 硬解性能时，自行设置该变量）。
    if std::env::var_os("GST_PLUGIN_FEATURE_RANK").is_none() {
        std::env::set_var(
            "GST_PLUGIN_FEATURE_RANK",
            // rank 0 = 不参与自动选择；软件解码 avdec_* 保持默认 rank
            "v4l2slh264dec:0,v4l2slh265dec:0,v4l2slvp8dec:0,v4l2slvp9dec:0,v4l2slav1dec:0,\
             v4l2h264dec:0,v4l2h265dec:0,v4l2vp8dec:0,v4l2vp9dec:0,\
             vah264dec:0,vah265dec:0,vavp8dec:0,vavp9dec:0,vaav1dec:0,vajpegdec:0",
        );
    }
    log::info!(
        "[ShiftBrowser] GST_PLUGIN_FEATURE_RANK={:?}（视频进程崩溃修复）",
        std::env::var("GST_PLUGIN_FEATURE_RANK")
    );

    // ===== Cookie 持久化（修复"登录状态不保存"）=====
    // WebKitGTK 的 CookieManager 默认是内存模式，重启即丢——必须显式
    // set_persistent_storage 落盘。且该调用必须在任何 web process 创建之前
    // （tauri 的 setup 回调执行时主 webview 已创建，已太晚），故放在 run() 开头。
    // 存储路径与 app_data_dir 一致：$XDG_DATA_HOME/com.shiftbrowser.app/cookies.sqlite
    #[cfg(target_os = "linux")]
    {
        // P0-2：平台代码下沉到 engine::platform（GTK 初始化 + Cookie 持久化）
        let data_home = std::env::var("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::var("HOME")
                    .map(|h| std::path::PathBuf::from(h).join(".local/share"))
                    .unwrap_or_else(|_| std::path::PathBuf::from("."))
            });
        if let Some(cookie_path) =
            crate::engine::platform::init_cookie_persistence(&data_home)
        {
            // 安全：Cookie 库按 umask 落盘（常见 664），立即收紧到 600（另有周期重扫）
            if let Some(dir) = cookie_path.parent() {
                crate::harden_sensitive_files(dir);
            }
            // 此处 logger 插件尚未初始化，用 println! 保证启动日志可见
            println!(
                "[ShiftBrowser] Cookie 持久化已启用: {}",
                cookie_path.display()
            );
        }
    }

    // ===== 隐私防护 P3：TLS 指纹规范化（已回退，2026-09-18）=====
    // 回退原因：设置 G_TLS_GNUTLS_PRIORITY 后邮箱正文渲染为空 —— 该环境变量
    // 作用于进程内全部 TLS 连接，若站点部分资源依赖被裁掉的协议/套件组合，
    // 会表现为内容加载不全（空白）。网络底层改动风险远高于收益，故回退。
    // 保留结论：JA4 无法伪装成 Chrome（GnuTLS vs BoringSSL），
    // 与其在底层冒险，不如把预算放在页面内的加噪与归一化。
    // std::env::set_var(
    //     "G_TLS_GNUTLS_PRIORITY",
    //     "NORMAL:-VERS-ALL:+VERS-TLS1.3:+VERS-TLS1.2:-VERS-TLS1.0:-VERS-TLS1.1",
    // );
}
