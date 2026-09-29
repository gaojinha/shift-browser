//! lib.rs 测试（R2-1 拆分 · 2026-09-24）
//!
//! 契约守卫：断言 generate_handler! 注册的每个命令都有 #[tauri::command] 定义。
//! TS 类型导出：把 Rust struct 导出为 TS interface（生成物 src/types/generated/）。

mod tests {
    /// P1-2 契约守卫（2026-09-22）：断言 generate_handler! 注册的每个命令
    /// 在源码中都有对应的 #[tauri::command] 定义（防"定义了命令但漏注册"）。
    #[test]
    fn all_registered_commands_are_defined() {
        // 本文件即 lib.rs，include_str! 取自源码
        let src = include_str!("lib.rs");
        // 提取 generate_handler! 块
        let start = src
            .find("generate_handler![")
            .expect("未找到 generate_handler");
        let block_start = start + "generate_handler![".len();
        let rest = &src[block_start..];
        let end = rest.find("])").expect("generate_handler 未闭合");
        let block = &rest[..end];
        // 注册的命令名（形如 a::b,）
        let registered: Vec<String> = block
            .split(',')
            .filter_map(|seg| {
                let t = seg.trim();
                if t.is_empty() {
                    return None;
                }
                // 取最后一段（模块::函数）
                let name = t.split("::").last()?.trim();
                if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !name.is_empty() {
                    Some(name.to_string())
                } else {
                    None
                }
            })
            .collect();
        assert!(
            registered.len() > 100,
            "注册命令数异常: {}",
            registered.len()
        );

        // 收集全部源码里的 #[tauri::command] 定义（含子模块）
        let sources = [
            include_str!("lib.rs"),
            include_str!("tabs.rs"),
            include_str!("tabs/commands/mod.rs"),
            include_str!("tabs/commands/lifecycle_cmds.rs"),
            include_str!("tabs/commands/nav_cmds.rs"),
            include_str!("tabs/commands/tab_state_cmds.rs"),
            include_str!("tabs/commands/ui_cmds.rs"),
            include_str!("tls.rs"),
            include_str!("perms.rs"),
            include_str!("useragent.rs"),
            include_str!("extensions.rs"),
            include_str!("extensions/adblock.rs"),
            include_str!("extensions/privacy.rs"),
            include_str!("extensions/privacy_cmd.rs"),
            include_str!("extensions/manager.rs"),
            include_str!("newtab.rs"),
            include_str!("ai.rs"),
            // P2（2026-09-26）：网页随手问，本机 opencode 代理
            include_str!("webqa.rs"),
            include_str!("opencode_chat.rs"),
            include_str!("opencode_chat/models.rs"),
            include_str!("webzh.rs"),
            include_str!("api.rs"),
            include_str!("api/dashboard.rs"),
            include_str!("reader.rs"),
            include_str!("translate.rs"),
            include_str!("gpc.rs"),
            include_str!("audit.rs"),
            include_str!("mcp_cmd.rs"),
            include_str!("agent_cmd.rs"),
            include_str!("content_cmd.rs"),
            include_str!("default_browser.rs"),
            include_str!("netmon.rs"),
            include_str!("db/bookmarks.rs"),
            include_str!("db/bookmarks_html.rs"),
            include_str!("db/history.rs"),
            include_str!("db/sessions.rs"),
            include_str!("db/settings.rs"),
            include_str!("db/passwords.rs"),
            include_str!("db/vault.rs"),
            include_str!("db/sync.rs"),
            include_str!("db/workspaces.rs"),
            include_str!("db/tab_groups.rs"),
            include_str!("db/downloads.rs"),
            include_str!("db/userscripts.rs"),
            include_str!("db/users/mod.rs"),
            include_str!("db/users/session.rs"),
            include_str!("db/users/profile.rs"),
            include_str!("db/users/import_export.rs"),
            include_str!("db/users/security.rs"),
            include_str!("engine/llmstxt.rs"),
            include_str!("app/misc_cmds.rs"),
        ];
        let mut defined: std::collections::HashSet<String> = std::collections::HashSet::new();
        for s in sources {
            let mut idx = 0;
            while let Some(pos) = s[idx..].find("#[tauri::command]") {
                let at = idx + pos + "#[tauri::command]".len();
                // 找后续的 fn 名
                if let Some(fpos) = s[at..].find("fn ") {
                    let after = &s[at + fpos + 3..];
                    let name: String = after
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect();
                    if !name.is_empty() {
                        defined.insert(name);
                    }
                }
                idx = at;
            }
        }

        // 断言：注册的每个命令都有定义
        let mut missing: Vec<&String> = registered
            .iter()
            .filter(|c| !defined.contains(*c))
            .collect();
        missing.sort();
        assert!(
            missing.is_empty(),
            "以下命令已注册但未找到 #[tauri::command] 定义: {:?}",
            missing
        );
    }

    #[test]
    fn callback_whitelist_accepts_normal() {
        assert!(crate::proto_shift::valid_callback("cb_1"));
        assert!(crate::proto_shift::valid_callback("a"));
        assert!(crate::proto_shift::valid_callback("_"));
        assert!(crate::proto_shift::valid_callback("x".repeat(64).as_str()));
    }

    #[test]
    fn callback_whitelist_rejects_injection() {
        // 恶意 callback 注入 JS：拼进变量名 eval 到特权 webview
        assert!(!crate::proto_shift::valid_callback(""));
        assert!(!crate::proto_shift::valid_callback("x".repeat(65).as_str()));
        assert!(!crate::proto_shift::valid_callback("cb;alert(1)//"));
        assert!(!crate::proto_shift::valid_callback("cb'));alert(1);//"));
        assert!(!crate::proto_shift::valid_callback("cb\"));alert(1);//"));
        assert!(!crate::proto_shift::valid_callback("cb\nalert(1)"));
        assert!(!crate::proto_shift::valid_callback("cb-1"));
        assert!(!crate::proto_shift::valid_callback("cb.1"));
        assert!(!crate::proto_shift::valid_callback("中文"));
    }

    #[test]
    fn nonce_injected_only_into_plain_inline_script_tags() {
        // 精确匹配 <script>：内联脚本拿 nonce；外部 module 脚本由 'self' 授权不动；
        // 闭合标签不得被改
        let html = "<head><script>var a=1;</script>\
<script type=\"module\" crossorigin src=\"/assets/x.js\"></script></head>";
        let out = crate::proto_shift::inject_script_nonce(html, "abc123");
        assert!(out.contains("<script nonce=\"abc123\">var a=1;</script>"));
        assert!(out.contains("<script type=\"module\" crossorigin src=\"/assets/x.js\"></script>"));
        // 只注入一次（闭合标签未被误匹配）
        assert_eq!(out.matches("nonce=\"abc123\"").count(), 1);
    }

    #[test]
    fn newtab_style_two_inline_scripts_both_get_nonce() {
        // newtab 渲染后：IPC 桥 + 业务脚本两个无属性 <script>
        let html = "<script>(function(){})();</script><div>x</div><script>boot();</script>";
        let out = crate::proto_shift::inject_script_nonce(html, "n2");
        assert_eq!(out.matches("<script nonce=\"n2\">").count(), 2);
    }

    #[test]
    fn csp_nonce_replaces_only_script_src_unsafe_inline() {
        let csp = crate::proto_shift::csp_with_script_nonce("Zm9vYmFy");
        assert!(
            csp.contains("script-src 'self' 'nonce-Zm9vYmFy'"),
            "script-src 应带 nonce: {csp}"
        );
        assert!(
            !csp.contains("script-src 'self' 'unsafe-inline'"),
            "script-src 不得再留 unsafe-inline"
        );
        // style-src 维持内联（本次收面只针对脚本）
        assert!(csp.contains("style-src 'self' 'unsafe-inline'"));
        // 其余指令不丢
        for directive in [
            "object-src 'none'",
            "base-uri 'self'",
            "connect-src 'self'",
            "default-src 'self'",
        ] {
            assert!(csp.contains(directive), "CSP 丢指令 {directive}: {csp}");
        }
    }

    #[test]
    fn fresh_nonce_is_random_attribute_safe_base64() {
        let a = crate::proto_shift::fresh_script_nonce();
        let b = crate::proto_shift::fresh_script_nonce();
        // 16 字节 → 标准 base64（24 字符含填充），字符集不含引号/尖括号/空白，
        // 可安全放进 HTML 属性与 CSP 令牌
        assert_eq!(a.len(), 24);
        for s in [&a, &b] {
            assert!(s
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '='));
            assert!(!s.contains(['"', '\'', '<', '>', ' ']));
        }
        assert_ne!(a, b, "nonce 不可重复");
    }

    /// 生成物与契约镜像一致性：改 contracts.rs 未重生成即红（P0-2）
    #[test]
    fn contract_mirror() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/constants.ts");
        let text = std::fs::read_to_string(&path).expect("读取 src/constants.ts 失败");
        // 头注释必须含自动生成标记
        assert!(
            text.contains("自动生成"),
            "/constants.ts 头注释缺自动生成标记"
        );
        assert!(text.contains("请勿手改"), "constants.ts 头注释缺请勿手改");
        // 事件名：键名与值双包含（单引号生成物转义不影响键名包含判断）
        for (_, v) in crate::contracts::events::ALL {
            assert!(text.contains(v), "constants.ts 缺事件值 {v}");
        }
        for (k, _) in crate::contracts::events::ALL {
            assert!(text.contains(k), "constants.ts 缺事件键 {k}");
        }
        // 设置键：键名与默认值双包含（避免单引号转义误判整行）
        for d in crate::contracts::settings::SETTINGS {
            assert!(text.contains(d.key), "constants.ts 缺设置键 {}", d.key);
            // 空默认值不强制断言（太宽泛易误判），非空才断
            if !d.default.is_empty() {
                assert!(
                    text.contains(d.default),
                    "constants.ts 缺设置默认值 {}={}",
                    d.key,
                    d.default
                );
            }
        }
        // 超时：前端 ALL 里的语义键必须出现（值在 ALL 表里已列，非裸数字）
        for (k, _) in crate::contracts::timeout::ALL {
            assert!(text.contains(k), "constants.ts 缺超时键 {k}");
        }
        // 建连上限：后端专用（不进前端 ALL），取值必须收敛在合理区间——
        // 太小内网/弱网误杀，太大则单边地址族卡住时吃光整单总超时（2026-09-29 双栈复盘）
        let ct = crate::contracts::timeout::CONNECT_TIMEOUT_SECS;
        assert!(
            (1..=15).contains(&ct),
            "CONNECT_TIMEOUT_SECS={ct} 超出合理区间 1..=15"
        );
        // 快捷键：前端组合键串必须出现
        for (_, v) in crate::contracts::shortcuts::ALL {
            assert!(text.contains(v), "constants.ts 缺快捷键 {v}");
        }
        // 技能：id 必须出现
        for s in crate::contracts::skills::SKILLS {
            assert!(text.contains(s.id), "constants.ts 缺技能 {}", s.id);
        }
    }

    /// 快捷键双写一致性：SHORTCUTS 结构体表派生组合键串必须与 ALL 表一一对应（P1-2 止血）
    #[test]
    fn shortcuts_all_derived_equals_manual() {
        fn derive(s: &crate::contracts::shortcuts::Shortcut) -> String {
            let mut parts: Vec<String> = Vec::new();
            if s.ctrl {
                parts.push("ctrl".to_string());
            }
            if s.shift {
                parts.push("shift".to_string());
            }
            parts.push(s.key.to_string());
            parts.join("+")
        }
        let derived: Vec<String> = crate::contracts::shortcuts::SHORTCUTS
            .iter()
            .map(derive)
            .collect();
        let manual: Vec<String> = crate::contracts::shortcuts::ALL
            .iter()
            .map(|(_, v)| v.to_string())
            .collect();
        assert_eq!(derived.len(), manual.len(), "快捷键数量不一致");
        // 允许顺序不同但内容必须一致（当前顺序一致，若未来顺序调整也不误判为漂移）
        let mut d = derived.clone();
        let mut m = manual.clone();
        d.sort();
        m.sort();
        assert_eq!(d, m, "SHORTCUTS 派生组合键与 ALL 表不一致");
    }

    /// 技能双写一致性：SKILLS 与 ALL 四元组前四字段必须一致（P1-3 止血，且 ALL 丢 uses 必现）
    #[test]
    fn skills_all_consistent() {
        let from_struct: Vec<(&str, &str, &str, &str)> = crate::contracts::skills::SKILLS
            .iter()
            .map(|s| (s.id, s.name, s.icon, s.desc))
            .collect();
        let manual = crate::contracts::skills::ALL.to_vec();
        assert_eq!(from_struct, manual, "SKILLS 与 ALL 四元组不一致");
        // uses 字段必须非空（防止投影丢字段后未来加技能忘补 uses）
        for s in crate::contracts::skills::SKILLS {
            assert!(!s.uses.is_empty(), "技能 {} uses 为空", s.id);
        }
    }

    /// gdk 大小写 keyval 派生校验：字母键大写为小写-0x20，功能键自身（P2-3 正确性兜底）
    #[test]
    fn gdk_keyval_shift_derived() {
        for s in crate::contracts::shortcuts::SHORTCUTS {
            let lower = s.gdk_keyval;
            let upper = s.gdk_keyval_shift;
            // 功能键 0xFF00 段：大小写相同
            if (0xFF00..=0xFFFF).contains(&lower) {
                assert_eq!(lower, upper, "功能键 {} 大小写应相同", s.id);
                continue;
            }
            // 字母键：小写 0x61-0x7A，大写 0x41-0x5A，差 0x20
            if (0x61..=0x7A).contains(&lower) {
                assert_eq!(upper, lower - 0x20, "字母键 {} 大小写派生错", s.key);
            } else if (0x41..=0x5A).contains(&lower) {
                assert_eq!(upper, lower, "大写键 {} 不应再派生", s.key);
            } else if lower == upper {
                // Tab 等非字母键允许相同
            } else {
                // 其他键大写应为小写-0x20或相同，兜底断言两值在合理区间
                assert!(
                    (0x20..=0xFF).contains(&upper),
                    "键 {} gdk 值异常: {:#x}/{:#x}",
                    s.key,
                    lower,
                    upper
                );
            }
        }
    }
}

/// TS 类型契约导出（P2）：把 Rust struct 导出为 TS interface，
/// 消除前端 `src/types/*.ts` 手写与本端 struct 的漂移。
/// 生成物落在 `src/types/generated/`（由 cargo test 触发）。
mod ts_export_tests {
    use ts_rs::TS;

    #[test]
    fn export_bindings() {
        crate::tabs::Tab::export().expect("导出 Tab 失败");
        crate::db::bookmarks::Bookmark::export().expect("导出 Bookmark 失败");
        crate::db::history::HistoryEntry::export().expect("导出 HistoryEntry 失败");
        crate::db::downloads::Download::export().expect("导出 Download 失败");
        crate::db::passwords::PasswordMeta::export().expect("导出 PasswordMeta 失败");
        crate::db::sessions::Session::export().expect("导出 Session 失败");
        crate::db::sessions::SessionTab::export().expect("导出 SessionTab 失败");
        crate::extensions::manager::ExtensionInfo::export().expect("导出 ExtensionInfo 失败");
        crate::engine::content::Snapshot::export().expect("导出 Snapshot 失败");
        crate::engine::content::AriaElement::export().expect("导出 AriaElement 失败");
        crate::engine::content::InteractiveStats::export().expect("导出 InteractiveStats 失败");
        crate::engine::content::FormField::export().expect("导出 FormField 失败");
        crate::engine::content::LinkInfo::export().expect("导出 LinkInfo 失败");
        crate::engine::webmcp::WebmcpToolDef::export().expect("导出 WebmcpToolDef 失败");
    }
}
