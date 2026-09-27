// 系统能力提示词唯一来源（权限/证书/托盘/断网/内网守卫/用户/锁屏/自检/定时/默认浏览器/UA）
// 规则：只组装文本，不读库，不出网，不读设置，由调用方传参。注释与代码保持一致。

// 1. 自检报告解读（消费 selfcheck.rs）
pub fn selfcheck_explain(report_json: &str) -> String {
    format!(
        "你是浏览器体检医生，把下面【自检报告】翻成人话。\n\n【报告】\n{report}\n\n规则：\n健康项一句话带过；红项逐条说“是什么+影响啥+点哪修”，修法只说界面里真实存在的入口，不要编命令。\n最后给一句总体结论：能用/凑合用/赶紧修。\n200字内，不寒暄。",
        report = report_json.trim(),
    )
}

// 2. 证书错误处置（消费 tls.rs + tls/cert.rs）
pub fn cert_explain(
    host: &str,
    port: u16,
    reasons: &str,
    issuer: &str,
    not_after: &str,
    fingerprint: &str,
    is_exception: bool,
) -> String {
    format!(
        "你是证书安全顾问，用户打不开一个网站。\n\n【站点】{host}:{port}\n【错误原因】{reasons}\n【证书】签发者：{issuer}，到期：{not_after}，指纹：{fingerprint}\n【是否历史放行过】{hist}\n\n规则：\n过期/自签/错名/吊销，各给一句话“能不能进+怎么进最安全”。\n放行前必须说清后果：中间人能看到明文。\n只给建议，不替用户点放行。",
        hist = if is_exception { "是" } else { "否" },
    )
}

// 3. 断网四层诊断（消费 netmon.rs）
pub fn net_diagnose(down_kb: u32, up_kb: u32, ping_ms: i32, online: bool, symptom: &str) -> String {
    format!(
        "你是断网诊断员，按四层定位。\n\n【状态】下行{down}KB/s，上行{up}KB/s，延迟{ping}ms，在线{online}\n【现象】{symptom}\n\n逐层判，每层一句话：\n1. 网卡有没有流量；2. 网关通不通；3. 域名解不解析；4. 出网是不是被拦。\n结论只说断在哪一层+下一步点哪，不要一次倒四层命令。",
        down = down_kb,
        up = up_kb,
        ping = if ping_ms < 0 {
            "未知".to_string()
        } else {
            format!("{ping_ms}")
        },
        online = if online { "是" } else { "否" },
    )
}

// 4. 定时提示词生成（消费 auto_tasks.rs）
pub fn auto_agent_build(wish: &str) -> String {
    format!(
        "你是定时任务助手，帮用户写到点执行的提示词。\n\n【需求】{wish}\n【可用】打开侧栏发问、多步动作需人工确认、每次执行都是新会话无记忆\n\n输出一条可直接粘贴的提示词，要求：目标明确、带输出格式、注明信息来源，\n不写需要跨天记忆的东西。三选一模板往需求上套：资讯digest/页面总结/到点提醒。",
        wish = wish.trim(),
    )
}

// 5. 权限申请解读（消费 perms.rs）
pub fn perm_explain(host: &str, kind: &str, kind_zh: &str) -> String {
    format!(
        "你是权限顾问，网站正在要权限。\n\n【站点】{host}\n【权限】{kind}（{kind_zh}）\n\n一句话说它拿去干啥，一句话说放行的风险，一句话给建议（放/拒/用完即删）。\n银行、政务、陌生站默认建议拒，不劝放行。"
    )
}

// 6. 内网拦截说明（消费 net_guard.rs）
pub fn netguard_explain(url: &str, rule: &str) -> String {
    format!(
        "你是内网守卫解说员，用户打不开一个地址。\n\n【地址】{url}\n【命中】{rule}\n\n说清三件事：为啥拦、是不是误伤、白名单该怎么写（一行示例）。\n危险地址不教绕过，只说找网管。"
    )
}

// 7. UA兼容诊断（消费 useragent.rs）
pub fn ua_diagnose(host: &str, ua: &str) -> String {
    format!(
        "你是兼容性助手，网站提示“浏览器不受支持”。\n\n【站点】{host}\n【当前UA】{ua}\n\n先判：是认不出Chrome、还是点名要Windows/移动端、还是特性检测穿帮。\n给三个可试的UA，按顺序试，每换一个说清回退办法（清空即回默认）。\n不让用户手写UA，只让选。"
    )
}

// 8. 账号安全建议（消费 db/users + 锁屏）
pub fn account_security(
    failures: u32,
    seconds: u64,
    lock_enabled: bool,
    session: &str,
    backup: &str,
) -> String {
    format!(
        "你是账号安全助手，只看本地事实。\n\n【事实】防爆破{failures}次锁{seconds}秒、启动锁屏{lock}、会话{session}、备份{backup}\n\n三句话：现在什么水平、一处最该补的、多久备一次份。\n不讲大道理，不推销密码理论。",
        lock = if lock_enabled { "开" } else { "关" },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selfcheck_200字不寒暄() {
        let p = selfcheck_explain(r#"{"healthy":false}"#);
        assert!(p.contains("200字内"));
        assert!(p.contains(r#"{"healthy":false}"#));
    }

    #[test]
    fn cert_说清后果不代点() {
        let p = cert_explain("ex.com", 443, "证书已过期", "CA", "2026-01-01", "AB", false);
        assert!(p.contains("中间人能看到明文"));
        assert!(p.contains("不替用户点放行"));
        assert!(p.contains("否"));
    }

    #[test]
    fn net_延迟未知不断言() {
        let p = net_diagnose(0, 0, -1, false, "打不开网页");
        assert!(p.contains("未知"));
        assert!(p.contains("否"));
        assert!(p.contains("四层"));
    }

    #[test]
    fn auto_agent_三模板() {
        let p = auto_agent_build("每天早8点给我三条科技资讯");
        assert!(p.contains("资讯digest"));
        assert!(p.contains("无记忆"));
    }

    #[test]
    fn perm_默认拒() {
        let p = perm_explain("bank.com", "geolocation", "地理位置");
        assert!(p.contains("默认建议拒"));
    }

    #[test]
    fn netguard_不教绕过() {
        let p = netguard_explain("http://192.168.1.1/", "私有地址");
        assert!(p.contains("不教绕过"));
    }

    #[test]
    fn ua_不让手写() {
        let p = ua_diagnose("x.com", "UA");
        assert!(p.contains("不让用户手写UA"));
    }

    #[test]
    fn account_三句话() {
        let p = account_security(5, 30, true, "已登录", "未备份");
        assert!(p.contains("5次锁30秒"));
        assert!(p.contains("三句话"));
    }
}
