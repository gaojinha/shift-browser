//! 整页汉化（2026-09-26）
//!
//! 把「翻译当前页」从"发给 AI、译文显示在对话框"改成"译文直接长在页面上"。
//!
//! 三段式管线：
//!   1. `page_collect_translatable` —— 注入 page_collect.js 扫 DOM，收集该翻的
//!      文本（已在页面上打好 `__sbId` 标记）。
//!   2. `opencode_translate_batch` —— 前端分批调用，走本机 opencode（4096）
//!      做 AI 翻译。**不复用 opencode_ask 的流式链路**：整页要的是结构化
//!      JSON 数组（id → 译文），聊天流式协议在这儿只会碍事。
//!   3. `page_apply_translation` —— 注入 page_apply.js 按 `__sbId` 写回，
//!      或一键还原。
//!
//! 编排（分批 / 进度 / 取消）在前端 store：分批策略得随页面大小和实测速度调，
//! 写死后端就得重编。Rust 只提供三块可单测的能力。
//!
//! 隐私：走本机 4096，页面内容不出本机。凭据发现沿用 P2 那套
//! （从 /proc 读 opencode serve 的环境），不落盘。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};


/// 一批里最多放多少字符。
///
/// opencode 侧是本机大模型，一次 prompt 越长越慢、越容易输出跑偏。
/// 2000 字符实测能在一次响应里稳定吐回完整 JSON 数组；再大就要分两次，
/// 反而更慢。这个数是量出来的，不是拍的。
const BATCH_MAX_CHARS: usize = 2000;
/// 一批最多多少段。段数太多时模型容易漏项，译文和 id 对不上。
const BATCH_MAX_SEGMENTS: usize = 25;
/// 单次 opencode 请求超时。整页翻译慢，但单批不该超过 2 分钟。
const BATCH_TIMEOUT_SECS: u64 = 120;

// ---------------------------------------------------------------- 1. 收集

#[derive(Debug, Serialize)]
pub struct CollectedSegment {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub chars: usize,
}

#[derive(Debug, Serialize)]
pub struct CollectedAttr {
    pub id: String,
    pub attr: String,
    pub orig: String,
}

#[derive(Debug, Serialize)]
pub struct CollectResult {
    pub ok: bool,
    pub msg: Option<String>,
    pub data: Option<Value>,
}

#[tauri::command]
pub async fn page_collect_translatable(app: AppHandle, tab_id: Option<String>) -> CollectResult {
    let Some(wv) = resolve_webview(&app, tab_id.as_deref()) else {
        return CollectResult {
            ok: false,
            msg: Some("找不到标签页（或标签已休眠，切回该标签后重试）".into()),
            data: None,
        };
    };
    // 扫全页 DOM，大页面要几秒；2 秒的老超时是给轻量探测用的，这里放宽。
    match crate::api::handlers::eval_value_public(&wv, include_str!("page_collect.js"), 30).await {
        Some(v) => {
            let total = v.get("total").and_then(|x| x.as_u64()).unwrap_or(0);
            let attr_total = v.get("attrTotal").and_then(|x| x.as_u64()).unwrap_or(0);
            log::info!(
                "[webzh] 收集完成：文本 {total} 段 / 属性 {attr_total} 项 / 滤掉 {} 段",
                v.get("skipped").and_then(|x| x.as_u64()).unwrap_or(0)
            );
            CollectResult { ok: true, msg: None, data: Some(v) }
        }
        None => CollectResult {
            ok: false,
            msg: Some("页面脚本执行失败或超时（页面可能还在加载，或有 CSP 限制）".into()),
            data: None,
        },
    }
}

// ---------------------------------------------------------------- 2. 写回

#[derive(Debug, Deserialize, Serialize)]
pub struct ApplyPair {
    pub id: String,
    pub orig: String,
    pub translated: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ApplyAttr {
    pub attr: String,
    pub orig: String,
    pub translated: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ApplyPayload {
    #[serde(default)]
    pub pairs: Vec<ApplyPair>,
    #[serde(default)]
    pub attrs: Vec<ApplyAttr>,
}

#[derive(Debug, Serialize)]
pub struct ApplyResult {
    pub ok: bool,
    pub msg: Option<String>,
    pub data: Option<Value>,
}

/// `mode = "apply" | "restore"`
#[tauri::command]
pub async fn page_apply_translation(
    app: AppHandle,
    tab_id: Option<String>,
    mode: String,
    payload: Option<ApplyPayload>,
) -> ApplyResult {
    let Some(wv) = resolve_webview(&app, tab_id.as_deref()) else {
        return ApplyResult {
            ok: false,
            msg: Some("找不到标签页（或标签已休眠，切回该标签后重试）".into()),
            data: None,
        };
    };
    if mode != "apply" && mode != "restore" {
        return ApplyResult { ok: false, msg: Some("mode 只能是 apply 或 restore".into()), data: None };
    }

    let args = match (mode.as_str(), &payload) {
        ("apply", Some(p)) => serde_json::to_string(p).unwrap_or_else(|_| "{}".to_string()),
        _ => "{}".to_string(),
    };

    // page_apply.js 导出成一个函数（不自带调用），由这里传参执行。
    // 直接用最朴素的形式：包一层 IIFE 取出函数体再调。多写花样只会自己骗自己。
    let script = format!(
        "(() => {{ const f = {body}; return f({mode_arg}, {args}); }})()",
        body = include_str!("page_apply.js").trim(),
        mode_arg = serde_json::to_string(&mode).unwrap_or_else(|_| "\"apply\"".into()),
        args = args,
    );

    match crate::api::handlers::eval_value_public(&wv, &script, 20).await {
        Some(v) => ApplyResult { ok: true, msg: None, data: Some(v) },
        None => ApplyResult { ok: false, msg: Some("写回页面失败或超时".into()), data: None },
    }
}

// ---------------------------------------------------------------- 3. 翻译

#[derive(Debug, Deserialize)]
pub struct TranslateBatchReq {
    /// 本批待翻的条目：id + 原文
    pub items: Vec<BatchItem>,
    /// 页面标题，喂给模型当语境
    #[serde(default)]
    pub page_title: Option<String>,
    /// 复用已有会话（前端多批时传回来，避免每批新建一个会话）
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BatchItem {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct TranslateBatchResult {
    pub ok: bool,
    pub msg: Option<String>,
    /// id → 译文。模型漏掉的 id 不会出现在这里，前端据此判断失败率。
    pub data: Option<Vec<BatchItem>>,
    pub session_id: Option<String>,
}

/// AI 翻译一批。用本机 opencode（4096）。
#[tauri::command]
pub async fn opencode_translate_batch(
    db: tauri::State<'_, crate::db::Db>,
    req: TranslateBatchReq,
) -> crate::error::CommandResult<TranslateBatchResult> {
    if req.items.is_empty() {
        return Ok(TranslateBatchResult {
            ok: false,
            msg: Some("待翻译条目为空".into()),
            data: None,
            session_id: None,
        });
    }
    if req.items.len() > BATCH_MAX_SEGMENTS * 2 {
        return Ok(TranslateBatchResult {
            ok: false,
            msg: Some(format!("单批条目过多（{}），请分批", req.items.len())),
            data: None,
            session_id: None,
        });
    }

    let creds = crate::webqa::resolve_credentials_pub(db).await?;
    let http = crate::webqa::client_pub(&creds)?;
    let base = crate::webqa::base_url_pub();
    let agent = req.agent.unwrap_or_else(|| crate::webqa::DEFAULT_AGENT.to_string());

    // 翻译会话跟"随手问"分开：整页汉化会刷几百条消息，混进问答会话
    // 会把用户的聊天历史淹掉，也会污染后续追问的上下文。
    let sid = match crate::webqa::ensure_session_pub(&http, req.session_id.as_deref(), "网页汉化").await {
        Ok((s, _)) => s,
        Err(e) => {
            return Ok(TranslateBatchResult {
                ok: false,
                msg: Some(e.to_string()),
                data: None,
                session_id: None,
            })
        }
    };

    let prompt = build_translate_prompt_pub(&req.items, req.page_title.as_deref());

    // 复用 P2 的非流式单轮请求：订阅 SSE → POST prompt → 累积到回答完成
    match crate::webqa::ask_once(&http, &base, &sid, &agent, &prompt, BATCH_TIMEOUT_SECS).await {
        Ok(raw) => {
            let items = parse_translate_response(&raw, &req.items);
            let got = items.len();
            let want = req.items.len();
            if got == 0 {
                log::warn!("[webzh] 批次翻译失败：模型输出无法解析，前 200 字：{}", truncate(&raw, 200));
            } else if got < want {
                log::warn!("[webzh] 批次翻译漏项：{got}/{want}，其余按原文保留");
            }
            Ok(TranslateBatchResult {
                ok: true,
                msg: None,
                data: Some(items),
                session_id: Some(sid),
            })
        }
        Err(e) => Ok(TranslateBatchResult {
            ok: false,
            msg: Some(e.to_string()),
            data: None,
            session_id: Some(sid),
        }),
    }
}

/// 把本批条目切成 ≤(BATCH_MAX_CHARS 字符 / BATCH_MAX_SEGMENTS 段) 的批次。
///
/// 前端调用前会调它拿到分批方案，这样"一次要发几次"在前后端口径一致。
#[tauri::command]
pub fn plan_translate_batches(req: TranslateBatchReq) -> Value {
    json!({
        "ok": true,
        "data": plan_batches(&req.items),
        "total_items": req.items.len(),
    })
}

pub fn plan_batches(items: &[BatchItem]) -> Vec<Vec<BatchItem>> {
    // 先做段内切分：单段超长时按自然边界切成子块（id 带 .序号 后缀），
    // 否则一个 8000 字符的长段会整段丢给模型——翻译质量掉、还容易超时。
    let mut expanded: Vec<BatchItem> = Vec::new();
    for it in items {
        let c = it.text.chars().count();
        if c <= BATCH_MAX_CHARS {
            expanded.push(it.clone());
        } else {
            for (i, piece) in split_long_text(&it.text, BATCH_MAX_CHARS).into_iter().enumerate() {
                expanded.push(BatchItem {
                    id: format!("{}.{}", it.id, i),
                    text: piece,
                });
            }
        }
    }

    let mut out: Vec<Vec<BatchItem>> = Vec::new();
    let mut cur: Vec<BatchItem> = Vec::new();
    let mut cur_chars = 0usize;
    for it in expanded {
        let c = it.text.chars().count();
        let would = cur_chars + c;
        if !cur.is_empty() && (would > BATCH_MAX_CHARS || cur.len() >= BATCH_MAX_SEGMENTS) {
            out.push(std::mem::take(&mut cur));
            cur_chars = 0;
        }
        cur.push(it);
        cur_chars += c;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// 把超长文本切成不超过 `max_chars` 字符的多块，**尽量在自然边界切**。
///
/// 切点优先级：段落换行 > 句末标点 > 空格 > 硬切。
/// 硬切只在实在找不到边界时用（连续无空格长串），但会尽量避免把词切两半。
///
/// 切点处吃掉的空白/标点归入前一块：拼接回来时译文直接相连，不需要
/// 担心边界残留的空格把中文译文撑出缝来。
pub fn split_long_text(text: &str, max_chars: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return vec![text.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut start = 0usize;

    while start < chars.len() {
        if chars.len() - start <= max_chars {
            out.push(chars[start..].iter().collect());
            break;
        }
        let hard_end = (start + max_chars).min(chars.len());
        let mut cut = hard_end;
        // 在 [start+下限, hard_end] 区间内往回找自然边界
        let floor = start + (max_chars / 2).max(1);
        for i in (floor..hard_end).rev() {
            let c = chars[i];
            if c == '\n' {
                cut = i + 1;
                break;
            }
            if c == ' ' {
                cut = i + 1;
                break;
            }
            // 句末标点：把标点留给前一块
            if matches!(c, '.' | '。' | '!' | '！' | '?' | '？' | ';' | '；') {
                cut = i + 1;
                break;
            }
        }
        // 切出来空块（边界恰在起点）→ 强制前进一步，防死循环
        if cut <= start {
            cut = hard_end;
        }
        out.push(chars[start..cut].iter().collect());
        start = cut;
    }
    out
}

pub fn build_translate_prompt_pub(items: &[BatchItem], page_title: Option<&str>) -> String {
    // 编号列表比 JSON 输入更省 token、也更好读；输出则强制 JSON，
    // 因为要按 id 对号入座。
    let list: Vec<String> = items
        .iter()
        .map(|i| format!("{}::{}", i.id, i.text))
        .collect();
    format!(
        "你是网页汉化引擎。把下面网页片段翻译成简体中文，规则：\n\
         1. 只输出译文，不加解释、不加引号、不加 markdown 代码块。\n\
         2. 严格输出 JSON 数组，元素形如 {{\"id\":\"t0\",\"t\":\"译文\"}}，顺序与输入一致。\n\
         3. id 必须原样带回，不得改写、不得增删。\n\
         4. 保留原文中的专有名词、产品名、代码标识符、URL、数字和单位。\n\
         5. 已是中文的片段原样返回。\n\
         {}\n\
         \n输入片段（每行 id::原文）：\n{}",
        page_title
            .map(|t| format!("6. 页面标题是「{t}」，用它的领域语境选词。\n"))
            .unwrap_or_default(),
        list.join("\n")
    )
}

/// 从模型输出里抠出 JSON 数组。
///
/// LLM 输出的脏是常态：可能裹 ```json 代码块、可能前后带一句"好的，以下是"、
/// 可能用全角引号。这里逐层剥离 + 括号配平，找到第一个能 `serde_json` 吃下的
/// 数组。解析不出来就返回空 vec，让上层按"本批失败、保留原文"处理——
/// **宁可少翻一段，也不能把乱码写进别人的页面**。
pub fn parse_translate_response(raw: &str, items: &[BatchItem]) -> Vec<BatchItem> {
    let candidates = extract_json_candidates(raw);
    for c in &candidates {
        if let Ok(v) = serde_json::from_str::<Vec<Value>>(c) {
            let mut out = Vec::new();
            for e in v {
                let id = e.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
                let t = e.get("t")
                    .or_else(|| e.get("translated"))
                    .or_else(|| e.get("text"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                if id.is_empty() || t.is_empty() {
                    continue;
                }
                // 只接受本批要翻的 id：模型幻觉出的 id 会被忽略
                if !items.iter().any(|i| i.id == id) {
                    continue;
                }
                // 模型回了个空壳（同文）或明显没翻（还全是原样英文）也丢掉
                if t.trim().is_empty() {
                    continue;
                }
                out.push(BatchItem { id, text: t });
            }
            if !out.is_empty() {
                return out;
            }
        }
    }
    Vec::new()
}

/// 依次产出候选 JSON 数组片段：整体 → 剥代码块 → 括号配平截取。
fn extract_json_candidates(raw: &str) -> Vec<String> {
    let mut cands = Vec::new();
    let trimmed = raw.trim();
    cands.push(trimmed.to_string());

    // 剥 ```json ... ```
    if let Some(a) = trimmed.find("```") {
        let rest = &trimmed[a + 3..];
        let rest = match rest.find('\n') {
            Some(nl) => &rest[nl + 1..],
            None => rest,
        };
        if let Some(b) = rest.find("```") {
            let body = &rest[..b];
            cands.push(body.trim().to_string());
            cands.push(body.to_string());
        }
    }

    // 括号配平：从第一个 '[' 到配平的 ']'
    if let Some(start) = trimmed.find('[') {
        let mut depth = 0i32;
        let mut in_str = false;
        let mut esc = false;
        for (i, ch) in trimmed[start..].char_indices() {
            let c = ch as u32;
            if in_str {
                if esc {
                    esc = false;
                } else if c == 0x5c {
                    esc = true;
                } else if c == 0x22 {
                    in_str = false;
                }
                continue;
            }
            match ch {
                '"' => in_str = true,
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        cands.push(trimmed[start..=start + i].to_string());
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    cands
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

// ---------------------------------------------------------------- 4. api.sock 出口

/// `page_collect_translatable` 的 api.sock 包装（供 MCP / 脚本驱动真机验收）。
///
/// 为什么要再暴露一个口子：这俩是 Tauri 命令，前端 invoke 调得到，但**自动化
/// 验收调不到**——api.sock 和 Tauri invoke 是两条互不相通的路。P1 的
/// `read_selection` 当初也是同样理由暴露出去的。实现共用，不复制。
pub(super) async fn collect_for_api(app: &AppHandle, req: &Value) -> Value {
    let tab_id = req.get("tab_id").and_then(|v| v.as_str()).map(str::to_string);
    let r = page_collect_translatable(app.clone(), tab_id).await;
    json!({
        "ok": r.ok,
        "msg": r.msg,
        "data": r.data,
    })
}

/// `page_apply_translation` 的 api.sock 包装。参数同上（req 里带 mode/payload）。
pub(super) async fn apply_for_api(app: &AppHandle, req: &Value) -> Value {
    let tab_id = req.get("tab_id").and_then(|v| v.as_str()).map(str::to_string);
    let mode = req
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("apply")
        .to_string();
    let payload: Option<ApplyPayload> = req.get("payload").and_then(|v| {
        if v.is_null() {
            None
        } else {
            serde_json::from_value::<ApplyPayload>(v.clone()).ok()
        }
    });
    let r = page_apply_translation(app.clone(), tab_id, mode, payload).await;
    json!({
        "ok": r.ok,
        "msg": r.msg,
        "data": r.data,
    })
}

// ---------------------------------------------------------------- 公共辅助

fn resolve_webview(app: &AppHandle, tab_id: Option<&str>) -> Option<tauri::Webview> {
    let req = json!({ "tab_id": tab_id });
    let label = match crate::api::handlers::target_label_public(app, &req) {
        Ok(l) => l,
        Err(_) => return None,
    };
    app.get_webview(&label)
}
// ---------------------------------------------------------------- live 测试出口

/// 供 `tests/webzh_live.rs`：把被测的 prompt 构造暴露出去，
/// 免得测试里再抄一份（抄的那份过不了就是假绿）。
pub use parse_translate_response as parse_translate_response_pub;
pub use split_long_text as split_long_text_pub;

#[cfg(test)]
mod tests {
    use super::*;

    fn items(v: &[(&str, &str)]) -> Vec<BatchItem> {
        v.iter()
            .map(|(a, b)| BatchItem { id: (*a).to_string(), text: (*b).to_string() })
            .collect()
    }

    #[test]
    fn 分批按字符数和段数双上限() {
        // 每段 30 字符，10 段 = 300 字符 → 字符上限不会触发，段数上限触发
        let its: Vec<BatchItem> = (0..60)
            .map(|i| BatchItem { id: format!("t{i}"), text: "x".repeat(30) })
            .collect();
        let b = plan_batches(&its);
        assert!(b.len() >= 2, "60 段必须切多批");
        for x in &b {
            assert!(x.len() <= BATCH_MAX_SEGMENTS, "单批段数超上限: {}", x.len());
        }
    }

    #[test]
    fn 分批不丢段不重段() {
        let its = items(&[
            ("t0", "short"),
            ("t1", &"x".repeat(900)),
            ("t2", "another"),
            ("t3", &"y".repeat(1500)),
        ]);
        let b = plan_batches(&its);
        let flat: Vec<String> = b.iter().flatten().map(|i| i.id.clone()).collect();
        assert_eq!(flat.len(), 4, "分批后条目数必须守恒");
        let mut uniq = flat.clone();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), 4, "不允许重复");
    }

    #[test]
    fn 超长单段也单独成批不丢() {
        // 段内切分（2026-09-26，高哥提醒"超大文件可以分段翻译"）之后，
        // 超长段会以 t0.0 / t0.1 / … 子块形式出现，id 不再是 t0 本身。
        // 这里断言的是"内容没丢、且能追溯回原 id"。
        let its = items(&[("t0", &"z".repeat(5000)), ("t1", "small")]);
        let b = plan_batches(&its);
        let flat: Vec<String> = b.iter().flatten().map(|i| i.id.clone()).collect();
        assert!(
            flat.iter().any(|i| i.starts_with("t0.")),
            "超长段应被切分成 t0.N 子块，实际: {:?}",
            flat
        );
        assert!(flat.contains(&"t1".to_string()), "旁边的正常段不能丢");
        // 切出来的内容拼起来应等于原文（t0 全是 z，用总长验更省事）
        let z_len: usize = b
            .iter()
            .flatten()
            .filter(|i| i.id.starts_with("t0."))
            .map(|i| i.text.chars().count())
            .sum();
        assert_eq!(z_len, 5000, "子块拼起来的长度必须等于原段长度");
    }

    #[test]
    fn 解析裸json数组() {
        let raw = r#"[{"id":"t0","t":"你好"},{"id":"t1","t":"世界"}]"#;
        let its = items(&[("t0", "hello"), ("t1", "world")]);
        let got = parse_translate_response(raw, &its);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].text, "你好");
    }

    #[test]
    fn 解析裹代码块的输出() {
        let raw = "好的，以下是翻译：\n```json\n[{\"id\":\"t0\",\"t\":\"你好\"}]\n```\n希望有帮助！";
        let its = items(&[("t0", "hello")]);
        let got = parse_translate_response(raw, &its);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].text, "你好");
    }

    #[test]
    fn 解析前后有废话的输出() {
        let raw = "Sure! Here you go: [{\"id\":\"t0\",\"t\":\"你好\"}] Hope this helps!";
        let its = items(&[("t0", "hello")]);
        let got = parse_translate_response(raw, &its);
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn 模型乱加id要过滤掉() {
        let raw = r#"[{"id":"t0","t":"你好"},{"id":"t99","t":"幻觉"}]"#;
        let its = items(&[("t0", "hello")]);
        let got = parse_translate_response(raw, &its);
        assert_eq!(got.len(), 1, "不在本批的 id 必须丢弃");
        assert_eq!(got[0].id, "t0");
    }

    #[test]
    fn 空译文丢弃不写进页面() {
        let raw = r#"[{"id":"t0","t":""},{"id":"t1","t":"  "},{"id":"t2","t":"你好"}]"#;
        let its = items(&[("t0", "a"), ("t1", "b"), ("t2", "c")]);
        let got = parse_translate_response(raw, &its);
        assert_eq!(got.len(), 1, "空译文绝不能进页面");
        assert_eq!(got[0].id, "t2");
    }

    #[test]
    fn 完全无法解析时返回空不报错() {
        let its = items(&[("t0", "hello")]);
        assert!(parse_translate_response("抱歉，我无法完成这个请求。", &its).is_empty());
    }

    #[test]
    fn 字符串里的方括号不破坏配平() {
        let raw = r#"[{"id":"t0","t":"用 [下标] 取值"}]"#;
        let its = items(&[("t0", "use [index]")]);
        let got = parse_translate_response(raw, &its);
        assert_eq!(got.len(), 1);
        assert!(got[0].text.contains('['));
    }

    #[test]
    fn prompt强制带回id且含语境() {
        let its = items(&[("t0", "Hello")]);
        let p = build_translate_prompt_pub(&its, Some("Kubernetes 文档"));
        assert!(p.contains("t0::Hello"), "输入必须带 id");
        assert!(p.contains("Kubernetes 文档"), "页面标题要进 prompt");
        assert!(p.contains("原样带回"), "必须强调 id 不可改");
    }

    #[test]
    fn 超长段被切分且不丢字() {
        // 3000 个字符的连续文本（无边界可找，测硬切也不丢）
        let long: String = "a".repeat(3000);
        let pieces = split_long_text(&long, 1000);
        assert!(pieces.len() >= 3, "应切成多块，实际 {}", pieces.len());
        let joined: String = pieces.concat();
        assert_eq!(joined, long, "切分拼接后必须与原文完全一致，不能丢字或多字");
    }

    #[test]
    fn 切分优先在自然边界() {
        let t = "First sentence here. Second sentence follows. Third one closes it.";
        let pieces = split_long_text(t, 25);
        assert!(pieces.len() > 1);
        // 块应以句末标点或空格收尾，别把词切两半
        for (i, p) in pieces.iter().enumerate() {
            if i == pieces.len() - 1 { break; }
            let last = p.chars().last().unwrap();
            assert!(
                matches!(last, '.' | ' ' | '。' | '!' | '?' | ';' | '\n'),
                "块 {:?} 结尾 {:?} 不是自然边界",
                p, last
            );
        }
        assert_eq!(pieces.concat(), t);
    }

    #[test]
    fn 短段不切() {
        assert_eq!(split_long_text("short text", 1000).len(), 1);
    }

    #[test]
    fn 切分不会死循环() {
        // max_chars 极小时也要能收敛
        let t = "abcdefghij".repeat(50);
        let pieces = split_long_text(&t, 1);
        assert!(!pieces.is_empty());
        assert_eq!(pieces.concat(), t);
    }

    #[test]
    fn 超长段的分批用子块id且带序号() {
        let its = items(&[("t0", &"x".repeat(5000))]);
        let b = plan_batches(&its);
        // 全部块的 id 都该是 t0.N 形式
        for batch in &b {
            for it in batch {
                assert!(it.id.starts_with("t0."), "子块 id 形如 t0.0，实际 {}", it.id);
            }
        }
        // 每块都不超过上限
        for batch in &b {
            for it in batch {
                assert!(it.text.chars().count() <= BATCH_MAX_CHARS);
            }
        }
    }

    #[test]
    fn 切分后拼回能还原整段id() {
        // 前端按 base id（点号前）拼子块，所以每个子块必须能追溯到原 id
        let its = items(&[("t7", &"y".repeat(3000))]);
        let b = plan_batches(&its);
        let bases: std::collections::HashSet<String> = b
            .iter()
            .flatten()
            .map(|i| i.id.split('.').next().unwrap().to_string())
            .collect();
        assert_eq!(bases.len(), 1);
        assert!(bases.contains("t7"));
    }

}
