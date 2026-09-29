//! 阅读模式抓取管线 + SSRF 防护（R1 拆分 · 2026-09-25）
//!
//! 用系统 curl 抓 HTML（TLS 指纹可信），手动跟随重定向（最多 5 跳），
//! 每一跳都解析+校验公网并生成 --resolve pin，DNS-rebinding 无法在校验后换内网。
//! readability::extract 为纯解析（blocking，跑在 spawn_blocking 里）。

/// 阅读模式网络抓取超时
const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// 抓取并解析正文（带超时 + 净化）
/// 2026-09-19 修复：不再用 readability::extractor::scrape（其内置 reqwest 0.11 blocking，
/// 无 UA 配置且兼容性差，对部分站点误报 UnexpectedError）。
/// 改为系统 curl 抓 HTML（TLS 指纹可信，手动跟随重定向逐跳 pin），再调 extract 纯解析。
/// （注：此前注释误写"reqwest 0.12"，实际为 curl，2026-09-29 订正）
pub(super) async fn scrape_article(url: &str) -> Result<readability::extractor::Product, String> {
    // P2：只允许 http/https——fetch_article/enter_reader_mode 共用入口，
    // 显式拒绝 file:/javascript: 等 scheme（readability 内部 reqwest 不认其他
    // scheme，但入口拦截更清晰，防下游意外处理）
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("只支持普通网页".to_string());
    }
    // SSRF 防护（2026-09-19 修复）：拒绝内网/环回/链路本地/保留地址。
    // 此前任意 URL 都能让本机 reqwest 去抓，可被用来探测内网/云元数据（169.254.169.254）。
    validate_public_url(url).await?;
    let url = url.to_string();
    // 日志脱敏（2026-09-29）：只记 scheme://host，路径/query 常含搜索词/token。
    // 此前 `url.clone()` 名为 url_for_log 实为完整 URL，直接落日志。
    let url_for_log = crate::redact_url(&url);
    let html = match tokio::time::timeout(FETCH_TIMEOUT, fetch_article_html(&url)).await {
        Ok(Ok(html)) => html,
        Ok(Err(e)) => {
            log::warn!("[reader] 抓取/提取失败: {} -> {}", url_for_log, e);
            return Err(e);
        }
        Err(_) => return Err("抓取正文超时（20 秒），可能是慢站或网络问题".to_string()),
    };
    let parsed = url::Url::parse(&url).map_err(|e| format!("URL 解析失败: {}", e))?;
    tokio::task::spawn_blocking(move || {
        readability::extractor::extract(&mut html.as_bytes(), &parsed)
            .map_err(|e| format!("正文提取失败: {}", e))
    })
    .await
    .map_err(|e| {
        log::warn!("[reader] 正文提取线程异常: {} -> {}", url_for_log, e);
        "正文提取线程异常".to_string()
    })?
    .map_err(|e| {
        log::warn!("[reader] 正文提取失败: {} -> {}", url_for_log, e);
        e
    })
}

async fn fetch_article_html(url: &str) -> Result<String, String> {
    const BROWSER_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36";
    let site_ua = url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(crate::useragent::ua_for))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| BROWSER_UA.to_string());
    match curl_fetch_with_redirects(url, Some(site_ua.as_str())).await {
        Ok(h) => Ok(h),
        Err(e) if e.contains("412") => Err(format!(
            "{}（站点风控拒绝，未重试；请在浏览器页面完成验证后读取）",
            e
        )),
        Err(_) => curl_fetch_with_redirects(url, None).await,
    }
}

/// 用系统 curl 抓取，手动跟随重定向并逐跳 SSRF 校验
async fn curl_output_limited(
    command: &mut tokio::process::Command,
    max_bytes: usize,
) -> Result<(std::process::ExitStatus, Vec<u8>), String> {
    use tokio::io::AsyncReadExt;

    let mut child = command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("调用 curl 失败: {}", e))?;
    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill().await;
        let _ = child.wait().await;
        return Err("无法读取 curl 响应".to_string());
    };
    let mut output = Vec::with_capacity(max_bytes.min(64 * 1024));
    let mut chunk = [0u8; 16 * 1024];
    loop {
        let n = match stdout.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(format!("读取 curl 响应失败: {}", e));
            }
        };
        if output.len().saturating_add(n) > max_bytes {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(format!("网页响应超过 {} 字节上限", max_bytes));
        }
        output.extend_from_slice(&chunk[..n]);
    }
    let status = child
        .wait()
        .await
        .map_err(|e| format!("等待 curl 退出失败: {}", e))?;
    Ok((status, output))
}

pub(crate) async fn curl_fetch_with_redirects(
    url: &str,
    ua: Option<&str>,
) -> Result<String, String> {
    let mut cur = url.to_string();
    // 建连上限走 contracts 单源：单边地址族卡住时快速失败换下一 --resolve 地址
    let connect_to = crate::contracts::timeout::CONNECT_TIMEOUT_SECS.to_string();
    for _ in 0..5 {
        // P0-6：每一跳（含上一跳 Location）都先解析+校验并生成 --resolve pin。
        // curl 建连只能连本次解析出且校验为公网的地址，不再自行二次解析，
        // DNS-rebinding 无法在校验之后把目标换成 127.0.0.1/169.254.169.254。
        let pins = curl_pin_args(&cur).await?;
        // 单跳探测：拿状态码 + 重定向目标（curl 不带 -L 不自动跟随）
        let mut cmd = tokio::process::Command::new("curl");
        cmd.args([
            "-s",
            "--compressed",
            "--max-time",
            "20",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code} %{redirect_url}",
        ]);
        cmd.arg("--connect-timeout").arg(&connect_to);
        for p in &pins {
            cmd.args(["--resolve", p]);
        }
        if let Some(u) = ua {
            cmd.args(["-A", u]);
        }
        cmd.arg(&cur);
        let (status, stdout) = curl_output_limited(
            &mut cmd,
            crate::contracts::limits::READER_MAX_REDIRECT_METADATA_BYTES,
        )
        .await?;
        if !status.success() {
            return Err(format!("curl 重定向探测失败: {:?}", status.code()));
        }
        let meta = String::from_utf8_lossy(&stdout);
        let mut parts = meta.trim().splitn(2, ' ');
        let code: u16 = parts.next().unwrap_or("0").parse().unwrap_or(0);
        let loc = parts.next().unwrap_or("").trim().to_string();
        if (300..400).contains(&code) && !loc.is_empty() {
            // 重定向目标的公网校验在下一轮循环顶部随 pin 一起完成
            cur = loc;
            continue;
        }
        if !(200..300).contains(&code) {
            return Err(format!("抓取失败: HTTP {}", code));
        }
        // 2xx：拿正文（同一 host，沿用同一 pin 集合）
        let mut cmd2 = tokio::process::Command::new("curl");
        cmd2.args(["-s", "--compressed", "--max-time", "20"]);
        cmd2.arg("--connect-timeout").arg(&connect_to);
        for p in &pins {
            cmd2.args(["--resolve", p]);
        }
        if let Some(u) = ua {
            cmd2.args(["-A", u]);
        }
        cmd2.arg(&cur);
        let (status, stdout) = curl_output_limited(
            &mut cmd2,
            crate::contracts::limits::READER_MAX_ARTICLE_BYTES,
        )
        .await?;
        if !status.success() {
            return Err(format!("抓取失败: curl 退出码 {:?}", status.code()));
        }
        let html = String::from_utf8_lossy(&stdout).to_string();
        if html.is_empty() {
            return Err("抓取结果为空".to_string());
        }
        return Ok(html);
    }
    Err("重定向次数过多".to_string())
}

/// 异步解析一跳 URL 并生成 curl `--resolve` pin（P0-6，重定向逐跳调用）。
/// - IP 字面量：校验公网后返回空列表（无 DNS 面，curl 直连即可）
/// - 域名：用系统解析器只解析这一次，任一结果落内网即整体拒绝（双答 DNS
///   混入 127.0.0.1/169.254.169.254 时拒绝），全部公网才返回 pin 列表；
///   curl 拿到 pin 后建连阶段不再二次解析，TOCTOU 闭合
async fn curl_pin_args(url: &str) -> Result<Vec<String>, String> {
    let parsed = url::Url::parse(url).map_err(|_| "URL 不合法".to_string())?;
    let raw = parsed
        .host_str()
        .ok_or_else(|| "URL 缺少主机名".to_string())?;
    let host = raw.trim_start_matches('[').trim_end_matches(']');
    let port = parsed.port_or_known_default().unwrap_or(80);
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        reject_private_ip(&ip)?;
        return Ok(Vec::new());
    }
    let sock: Vec<_> = tokio::time::timeout(
        std::time::Duration::from_secs(crate::contracts::timeout::DNS_LOOKUP_SECS),
        tokio::net::lookup_host((host, port)),
    )
    .await
    .map_err(|_| format!("DNS 解析超时: {}", host))?
    .map_err(|e| format!("DNS 解析失败: {} ({})", host, e))?
    .collect();
    let addrs = crate::ai::check_resolved_ips(host, sock.iter().map(|s| s.ip()), port)?;
    Ok(addrs
        .iter()
        .map(|sa| curl_resolve_value(host, sa.port(), sa.ip()))
        .collect())
}

/// 生成单条 curl `--resolve` 值：`host:port:ip`（IPv6 不带方括号，curl 可识别）
fn curl_resolve_value(host: &str, port: u16, ip: std::net::IpAddr) -> String {
    format!("{}:{}:{}", host, port, ip)
}

/// SSRF 校验：目标主机必须是公网地址。
/// - IP 字面量（127.0.0.1 / 10.x / 192.168.x / 169.254.x / ::1 / fc00:: / fe80:: 等）直接拒绝
/// - 域名 host 做 DNS 解析后逐一校验，任一解析结果落在内网即拒绝
pub(crate) async fn validate_public_url(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|e| {
        log::warn!("[reader] URL 解析失败: {} -> {}", crate::redact_url(url), e);
        "URL 格式不合法".to_string()
    })?;
    let raw_host = parsed
        .host_str()
        .ok_or_else(|| "URL 缺少主机名".to_string())?;
    // url crate 对 IPv6 返回 "[::1]"，去括号后再解析/查询
    let host = raw_host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    // IP 字面量：直接判
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        let r = reject_private_ip(&ip);
        if r.is_err() {
            log::warn!("[reader] SSRF 拦截 IP 字面量: {} -> {}", host, ip);
        }
        return r;
    }
    // 域名：异步解析全部 A/AAAA，统一走 ai::check_resolved_ips——任一落内网
    // 或解析结果为空都拒绝（P0-6 fail-closed；旧实现空结果会被误放行）。
    // DNS 超时（2026-09-29）：lookup_host 无内置超时，包 DNS_LOOKUP_SECS，
    // 超时按解析失败拒绝（fail-closed）。
    let sock: Vec<_> = tokio::time::timeout(
        std::time::Duration::from_secs(crate::contracts::timeout::DNS_LOOKUP_SECS),
        tokio::net::lookup_host((host.as_str(), 80)),
    )
    .await
    .map_err(|_| {
        log::warn!(
            "[reader] DNS 解析超时（{}s）: {}",
            crate::contracts::timeout::DNS_LOOKUP_SECS,
            host
        );
        format!("DNS 解析超时: {}", host)
    })?
    .map_err(|e| {
        log::warn!("[reader] DNS 解析失败: {} -> {}", host, e);
        format!("DNS 解析失败: {}", host)
    })?
    .collect();
    if let Err(e) = crate::ai::check_resolved_ips(&host, sock.iter().map(|s| s.ip()), 80) {
        log::warn!("[reader] SSRF 拦截域名解析: {}", e);
        return Err(e);
    }
    Ok(())
}

fn reject_private_ip(ip: &std::net::IpAddr) -> Result<(), String> {
    // P0-6：统一走 ai::is_private_ip。本函数旧私有实现漏判 IPv4-mapped IPv6
    // （::ffff:127.0.0.1 / ::ffff:169.254.169.254 会被误放行）及 v6 组播/站点本地。
    if crate::ai::is_private_ip(*ip) {
        Err("目标地址是内网/保留地址，已拦截（SSRF 防护）".to_string())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curl_resolve_value_formats_v4_and_brackets_free_v6() {
        // curl --resolve 格式 host:port:ip；IPv6 不带方括号
        let v4: std::net::IpAddr = "93.184.216.34".parse().unwrap();
        assert_eq!(
            curl_resolve_value("example.com", 443, v4),
            "example.com:443:93.184.216.34"
        );
        let v6: std::net::IpAddr = "2606:4700::1111".parse().unwrap();
        assert_eq!(
            curl_resolve_value("v6.example", 8443, v6),
            "v6.example:8443:2606:4700::1111"
        );
    }

    #[tokio::test]
    async fn curl_pin_args_rejects_internal_ip_literals() {
        // 本机/元数据/IPv6 环回字面量直接拒绝
        assert!(curl_pin_args("http://127.0.0.1:19701/").await.is_err());
        assert!(curl_pin_args("https://169.254.169.254/latest/meta-data")
            .await
            .is_err());
        assert!(curl_pin_args("https://[::1]/").await.is_err());
        assert!(curl_pin_args("https://[fe80::1]/").await.is_err());
    }

    #[tokio::test]
    async fn curl_pin_args_rejects_ipv4_mapped_v6_literal() {
        // 回归：旧 reject_private_ip 对 ::ffff:127.0.0.1 按"看似公网的 v6"误放行
        assert!(curl_pin_args("https://[::ffff:127.0.0.1]/").await.is_err());
        assert!(curl_pin_args("https://[::ffff:169.254.169.254]/")
            .await
            .is_err());
        assert!(
            reject_private_ip(&"::ffff:192.168.0.1".parse::<std::net::IpAddr>().unwrap()).is_err()
        );
    }

    #[tokio::test]
    async fn curl_pin_args_public_ip_literal_needs_no_pin() {
        // 公网 IP 字面量无 DNS 面：空 pin 列表，curl 直连
        assert_eq!(
            curl_pin_args("https://1.1.1.1/").await.unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            curl_pin_args("https://[2606:4700::1111]/").await.unwrap(),
            Vec::<String>::new()
        );
    }

    #[tokio::test]
    async fn curl_pin_args_rejects_bad_urls() {
        assert!(curl_pin_args("not a url").await.is_err());
        assert!(curl_pin_args("file:///etc/passwd").await.is_err());
    }
}
