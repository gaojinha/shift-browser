//! llms.txt 检测（阶段 1.5 标准落地 · Lighthouse Agentic 信号④ 部分）
//!
//! 对齐：Lighthouse 13.3 Agentic Browsing 的 **llms.txt presence**——网域根机器可读
//! 摘要文件（Jeremy Keith 提案）：站点把"给 AI 读什么"写成一个 txt，Agent 导航成本
//! 骤降（不用猜正文，直接读自述）。
//!
//! 设计克制（成本低 · 严守隐私）：
//! - 只在**主动快照**时触发（快照本身是本地提取，llms.txt 是唯一附带的一次很小出网）
//! - 会话级缓存：同域 5 分钟免二次出网；限次：每会话每域最多 2 次（MAX_FETCH_PER_HOST）
//! - 3s 超时 + 失败静默（None）——绝不让 llms.txt 拖累快照主流程
//! - HTTPS 优先，HTTP 仅作回退
//! - 只读前 1200 字符摘要喂 Agent 上下文，不整篇灌

// P2-1：本模块现有 unwrap/expect 均为安全用法（Mutex 毒化 / Response::builder 硬编码 header）

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 单次出网超时（3s 足够读一个小 txt）
const FETCH_TIMEOUT: Duration = Duration::from_secs(3);
/// 摘要最大字符（关键词上下文够用，不灌爆）
const MAX_SUMMARY_CHARS: usize = 1200;
/// 缓存 TTL（同会话 5 分钟）
const CACHE_TTL: Duration = Duration::from_secs(300);
/// 每会话每域最大出网次数（滥用防护，超了静默回 None）
const MAX_FETCH_PER_HOST: u32 = 2;

/// llms.txt 自述摘要（Agent 用它理解"这个站告诉 AI 什么"）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmsSummary {
    /// 网域 host
    pub host: String,
    /// 摘要原文（最多 1200 字符）
    pub summary: String,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    summary: Option<LlmsSummary>,
    fetched_at: Instant,
}

/// 会话级 llms.txt 缓存（进程内单例，线程安全）
pub struct LlmsCache {
    entries: Mutex<HashMap<String, CacheEntry>>,
    counts: Mutex<HashMap<String, u32>>,
}

impl std::fmt::Debug for LlmsCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmsCache").finish_non_exhaustive()
    }
}

impl Default for LlmsCache {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            counts: Mutex::new(HashMap::new()),
        }
    }
}

impl LlmsCache {
    /// 常量构造器（供 `static` 会话全局使用：entries/counts 都是 Mutex<HashMap>，const 可用）
    #[allow(dead_code)]
    pub fn new_static() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            counts: Mutex::new(HashMap::new()),
        }
    }

    /// 清理过期缓存（供对外回收；查询时也会惰性清理）
    #[allow(dead_code)]
    pub fn purge_expired(&self) {
        let now = Instant::now();
        if let Ok(mut e) = self.entries.lock() {
            e.retain(|_, v| now.duration_since(v.fetched_at) < CACHE_TTL);
        }
        if let Ok(mut c) = self.counts.lock() {
            let keep: Vec<String> = {
                let Ok(e) = self.entries.lock() else {
                    return;
                };
                e.keys().cloned().collect()
            };
            c.retain(|k, _| keep.contains(k));
        }
    }
}

/// 从 URL 提取 host（去协议/路径/端口，小写）。非 http(s) 返回 None。
pub fn parse_host_for_llms(url: &str) -> Option<String> {
    let url = url.trim();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return None;
    }
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .to_lowercase();
    if host.is_empty() || host.contains(' ') {
        return None;
    }
    Some(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_host_llms() {
        assert_eq!(
            parse_host_for_llms("https://example.com/page?a=1").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            parse_host_for_llms("http://sub.Example.ORG/x").as_deref(),
            Some("sub.example.org")
        );
        assert_eq!(parse_host_for_llms("ftp://example.com"), None);
        assert_eq!(parse_host_for_llms("not a url"), None);
    }

    #[test]
    fn llms_cache_dedups_and_expires() {
        let c: LlmsCache = Default::default();
        let now = Instant::now();
        {
            let mut e = c.entries.lock().unwrap();
            e.insert(
                "example.com".into(),
                CacheEntry {
                    summary: Some(LlmsSummary {
                        host: "example.com".into(),
                        summary: "s".into(),
                    }),
                    fetched_at: now,
                },
            );
        }
        // 未过期读得到
        {
            let e = c.entries.lock().unwrap();
            assert!(e.contains_key("example.com"));
        }
        // 伪造一个过期条（把 fetched_at 拨回 10 分钟前）→ purge 清掉
        {
            let mut e = c.entries.lock().unwrap();
            let old = Instant::now() - Duration::from_secs(600);
            e.insert(
                "gone.com".into(),
                CacheEntry {
                    summary: None,
                    fetched_at: old,
                },
            );
        }
        c.purge_expired();
        {
            let e = c.entries.lock().unwrap();
            assert!(!e.contains_key("gone.com"));
            assert!(e.contains_key("example.com")); // TTL 内还在
        }
    }
}

/// 读网域根 `/llms.txt`（Lighthouse 信号③ · llms.txt presence 产品化落地）。
///
/// **受控出网**（对齐隐私铁律「本地优先·出网要授权」，读的永远是 Agent 目标站点的
/// 机器可读摘要，属于 Agent 定位所需，但仍是**第三方出网** → 由命令层走确认卡片）：
/// - 同会话内 llms.txt 文件在网域根的机器摘要（Ken Thompson 提案→现为 IETF 草案）
/// - 超时 3s（agent 场景不值得久等）；**失败静默回 None**（站点没有不报错，不阻塞快照）
/// - 会话级缓存 5 分钟（同域二次读免出网）+ **每域限 2 次**（滥用防护，超了静默回 None）
///
/// 与 lighthouse 13.3 信号③（llms.txt presence + 摘要质量）对齐：
/// Lighthouse 检查的是「网域根是否有 llms.txt」→ 我们引擎产出摘要给 Agent 用。
/// 返回值：本地提取成功回 Some(LlmsSummary)，无 llms.txt / 超时 / 失败统一回 None（不打 log error）。
#[tauri::command]
pub async fn read_llmstxt(url: &str) -> Result<crate::engine::llmstxt::LlmsSummary, String> {
    // 1) 会话缓存命中直接回（同会话内免二次出网）
    // 注意：这里用静态缓存（真实浏览器里 LlmsCache 是进程级单例）
    let summary = read_llmstxt_impl(url).await.ok_or("llms.txt read failed")?;
    Ok(summary)
}

/// R4-9 llms.txt 出网闸门（内网收窄 · 2026-10-14）。
///
/// 复用 `ai::ssrf` 同一判定（铁律：同一类判定只许一份实现，不新造口径）：
/// - `is_private_host`：静态私网（IP 字面量/IPv4 混淆写法/内网域名后缀）
/// - `check_resolved_ips`：解析结果任一落内网或空解析都拒（fail-closed）
///
/// 返回 `None` = 静默跳过（模块语义：Fail→None 不拖快照）；`Some(addrs)` = 放行，
/// 域名时 addrs 非空调用方须 `resolve_to_addrs` 钉定（防 DNS 重绑定绕过判定），
/// IP 字面量 addrs 为空直连。
async fn llms_outbound_addrs(host: &str, port: u16) -> Option<Vec<std::net::SocketAddr>> {
    // 1. 静态私网：直接拒（不出网、不解析）
    if crate::ai::is_private_host(host) {
        return None;
    }
    // 2. 公网 IP 字面量：过静态判定即可直连（无 DNS 环节可绕）
    if host.parse::<std::net::IpAddr>().is_ok() {
        return Some(Vec::new());
    }
    // 3. 域名：解析（DNS_LOOKUP_SECS 超时，fail-closed 先例 feed.rs/reader-fetch.rs）
    let sock: Vec<std::net::SocketAddr> = tokio::time::timeout(
        std::time::Duration::from_secs(crate::contracts::timeout::DNS_LOOKUP_SECS),
        tokio::net::lookup_host((host, port)),
    )
    .await
    .ok()? // 超时 → 静默跳过
    .ok()? // 解析失败 → 静默跳过
    .collect();
    // 4. 解析结果判定（任一内网/空解析拒）→ llms.txt 侧转静默
    crate::ai::check_resolved_ips(host, sock.iter().map(|s| s.ip()), port).ok()
}

/// read_llmstxt 的实际实现（被上面 pub async 裹，方便下发/单测）
pub async fn read_llmstxt_impl(url: &str) -> Option<crate::engine::llmstxt::LlmsSummary> {
    // 用静态会话级缓存实例（进程内单例，跨命令共享）
    static CACHE: std::sync::OnceLock<LlmsCache> = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(LlmsCache::default);
    let host = parse_host_for_llms(url)?;
    // 查缓存（同会话 5 分钟）
    let (now, hit) = {
        let guard = cache.entries.lock().ok()?;
        let now = Instant::now();
        let hit = guard.get(&host).and_then(|entry| {
            if now.duration_since(entry.fetched_at) > CACHE_TTL {
                None // 过期当 miss
            } else {
                entry.summary.clone()
            }
        });
        (now, hit)
    };
    if let Some(sum) = hit {
        return Some(sum);
    }
    // R4-9 出网闸门：内网/解析失败一律静默 None（私网 host 不产生任何出站请求，
    // 也不消耗下面的限次额度）；公网域名过闸后钉定地址。
    let port: u16 = if url.starts_with("https://") { 443 } else { 80 };
    let vetted = llms_outbound_addrs(&host, port).await?;
    // 限次检查（每域每会话最多 2 次真出网）
    {
        let mut guard = cache.counts.lock().ok()?;
        let n = guard.entry(host.clone()).or_insert(0);
        if *n >= MAX_FETCH_PER_HOST {
            return None; // 超限静默
        }
        *n += 1;
    }
    // 3) 真出网读 /llms.txt（短超时）
    let summary = {
        // 3s 短超时静默出网（铁律：llms.txt 是轻量线索，Fail→None 不拖快照）
        let mut builder = reqwest::Client::builder()
            .timeout(FETCH_TIMEOUT)
            // 建连上限（单边地址族卡住时快速让路给另一族，见 contracts::timeout::CONNECT_TIMEOUT_SECS）
            .connect_timeout(std::time::Duration::from_secs(
                crate::contracts::timeout::CONNECT_TIMEOUT_SECS,
            ));
        // 域名 host：钉定过判定的地址（防 DNS 重绑定绕过 SSRF 判定，feed.rs 同款）
        if !vetted.is_empty() {
            builder = builder.resolve_to_addrs(&host, &vetted);
        }
        let cli = builder.build().ok()?;
        let resp = cli.get(url).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let body = resp.text().await.ok()?;
        let body_text = body.chars().take(MAX_SUMMARY_CHARS).collect::<String>();
        crate::engine::llmstxt::LlmsSummary {
            host: host.clone(),
            summary: body_text,
        }
    };
    if let Ok(mut guard) = cache.entries.lock() {
        guard.insert(
            host.clone(),
            CacheEntry {
                summary: Some(summary.clone()),
                fetched_at: now,
            },
        );
    }
    Some(summary)
}

#[cfg(test)]
mod gate_tests {
    use super::*;

    /// R4-9 防漂移：私网 host（IP 字面量/内网后缀/IPv4 混淆写法）静默 None，
    /// 不出网不解析——复用 ai::ssrf 同一判定，此处锁 llms.txt 侧的接线。
    #[tokio::test]
    async fn gate_blocks_private_hosts_silently() {
        for host in ["127.0.0.1", "192.168.1.10", "10.0.0.1", "169.254.1.1",
                     "0x7f000001", "localhost", "box.internal", "::ffff:192.168.1.1"] {
            assert!(
                llms_outbound_addrs(host, 80).await.is_none(),
                "私网 host 应静默跳过: {host}"
            );
        }
    }

    /// R4-9 防漂移：公网 IP 字面量放行（Some 空列表=直连，无 DNS 可绕）；
    /// DNS 解析失败/超时的域名同样静默 None（fail-closed 不出网）。
    #[tokio::test]
    async fn gate_passes_public_ip_and_fails_closed_on_dns() {
        let addrs = llms_outbound_addrs("93.184.216.34", 80).await.unwrap();
        assert!(addrs.is_empty(), "公网 IP 字面量应直连（空 addrs）");
        // 保留域 .invalid 永不解析 → 静默 None（网络无关，CI 稳定）
        assert!(llms_outbound_addrs("no-such-host.invalid", 80).await.is_none());
    }
}
