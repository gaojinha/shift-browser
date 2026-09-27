//! `/api/model` 模型清单：解析 + 设置页下拉命令。

use serde::Serialize;
use serde_json::Value;

use crate::error::{AppError, CommandResult};
use crate::webqa::{base_url_pub, client_pub, resolve_credentials_pub};

/// 设置页模型下拉项。`model_ref` 是存进 `opencode_model` 设置项的完整值。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcModelInfo {
    /// 完整引用 `providerID/modelID`（设置项就存它）
    pub model_ref: String,
    pub provider_id: String,
    pub model_id: String,
    pub name: String,
    /// active / deprecated（来自 opencode ModelV2Info.status）
    pub status: String,
    pub enabled: bool,
    pub deprecated: bool,
}

/// 解析 `/api/model` 返回：仅保留 enabled 模型，active 在前、deprecated 在后，
/// 组内按名称排序。纯函数便于防漂移测试。
pub fn parse_models(v: &Value) -> Vec<OcModelInfo> {
    let mut out: Vec<OcModelInfo> = v
        .get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|m| {
                    let pid = m.get("providerID")?.as_str()?;
                    let mid = m.get("id")?.as_str()?;
                    // enabled 缺省视为 true；显式 false 才剔除
                    let enabled = m
                        .get("enabled")
                        .and_then(Value::as_bool)
                        .unwrap_or(true);
                    if !enabled {
                        return None;
                    }
                    let status = m
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("active")
                        .to_string();
                    Some(OcModelInfo {
                        model_ref: format!("{pid}/{mid}"),
                        provider_id: pid.to_string(),
                        model_id: mid.to_string(),
                        enabled,
                        name: m
                            .get("name")
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .unwrap_or(mid)
                            .to_string(),
                        deprecated: status == "deprecated",
                        status,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    // active 在前；同组按名称（稳定、好找）
    out.sort_by(|a, b| {
        a.deprecated
            .cmp(&b.deprecated)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

/// 列出本机 opencode 当前可用模型（设置页下拉用）。
///
/// 不传 location：opencode 以 serve 进程工作目录为默认位置（实测返回完整清单）。
#[tauri::command]
pub async fn opencode_list_models(
    db: tauri::State<'_, crate::db::Db>,
) -> CommandResult<Vec<OcModelInfo>> {
    let creds = resolve_credentials_pub(&db).await?;
    let http = client_pub(&creds)?;
    let resp = http
        .get(format!("{}/api/model", base_url_pub()))
        .send()
        .await
        .map_err(|e| AppError::net(format!("读取 opencode 模型列表失败：{e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::net(format!(
            "读取 opencode 模型列表失败：HTTP {}",
            resp.status()
        ))
        .into());
    }
    let v: Value = resp
        .json()
        .await
        .map_err(|e| AppError::net(format!("解析 opencode 模型列表失败：{e}")))?;
    Ok(parse_models(&v))
}
