//! 启动环境变量与平台初始化（run() 最早段，任何 GTK/WebKit 初始化之前执行）

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

    // ===== DMA-BUF 渲染器：不再默认禁用（2026-09-23 回退）=====
    // 【原方案】禁用它（WEBKIT_DISABLE_DMABUF_RENDERER=1）可修 Intel HD 630 上的
    // 网页/视频闪烁（WebKitGTK 2.42+ 默认走 DMA-BUF 路径，与 i915 配合不良）。
    //
    // 【为何回退】core dump 定位到崩溃在 WebKit 自身的
    //   AcceleratedBackingStore::update()（AcceleratedBackingStore.cpp:808）
    //   ← enterAcceleratedCompositingMode ← IPC EnterAcceleratedCompositingMode
    // 即「网页请求进入加速合成模式」时，m_legacyMainFrameProcess 已失效仍被解引用
    // （WebKit 生命周期竞态 bug，rdi=0）。而**禁用 DMA-BUF 会让加速合成走
    // SHM/EGLImage 兜底路径**，更容易踩中该竞态 → 网页删文件等操作触发 SIGSEGV。
    //
    // 结论：不禁用 = 走 WebKit 默认 DMA-BUF 路径，绕开该竞态。
    // 若视频闪烁复现，可显式设 `WEBKIT_DISABLE_DMABUF_RENDERER=1` 启动（二者取舍）。
    log::info!(
        "[ShiftBrowser] WEBKIT_DISABLE_DMABUF_RENDERER={:?}（未设置=走默认 DMA-BUF 渲染）",
        std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER")
    );

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
