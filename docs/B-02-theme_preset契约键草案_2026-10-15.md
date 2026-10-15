# B-02 `theme_preset` 契约键草案（S-5 落地准备）

> 日期：2026-10-15 · 执行：atomcode · 性质：**契约键草案**（只出草案，不写实现；`contracts.rs` 与前端**不动**，落不落地待高哥拍板）
> 上游：`docs/B-02-配色库扩充草案_2026-10-15.md`（10 套预设色值）· `docs/B-02-配色对比度实测报告_2026-10-15.md`（对比度实测 + 65 处硬伤最小修正）
> 契约纪律依据：`contracts.rs` 是唯一来源（AGENTS.md 第三节）——新键必须进 `SETTINGS` 白名单，前端默认值与后端白名单都从它派生。

## 一、键定义（草案，未落地）

`contracts.rs` 建议新增一行（**草案示意，未写入**）：

```rust
SettingDef { key: "theme_preset", default: "starship-blue", kind: Kind::Str, sensitive: false,
  desc: "配色预设（starship-blue/midnight-blue/ultra-black/warm-gray/cool-gray/pink-white/celadon/moss-green/honey-orange/lavender；空或未知=starship-blue）" },
```

| 字段 | 取值 | 依据 |
|---|---|---|
| `key` | `theme_preset` | 与 `theme` / `theme_color` 命名一致，语义为“配色方案” |
| `kind` | `Kind::Str` | 与 `theme`（dark/light）、`search_engine`（bing/baidu/google）同型——字符串枚举取值集 |
| `default` | `starship-blue` | = 预设 1 = **现行默认**（其深浅 accent 恰等于 App.vue 现硬编码 `#5b8cff`/`#3a7afe`），存量用户零视觉变化 |
| `sensitive` | `false` | 非秘密、非掩码；不进 `SENSITIVE_KEYS` / `MASKED_KEYS` |

## 二、取值集合（10 预设 ID）

| ID | 名称 | 定位 | 深 accent | 浅 accent |
|---|---|---|---|---|
| `starship-blue` | 星舰蓝（默认） | 现行默认，深空导航感 | `#5b8cff` | `#3a7afe` |
| `midnight-blue` | 午夜蓝 | 更沉静的蓝黑，夜间长读 | `#4d7dff` | `#2f5fd8` |
| `ultra-black` | Ultra 黑 | 纯黑 OLED 向，对比拉满 | `#7aa2ff` | `#2f5fe0` |
| `warm-gray` | 暖灰 | 中性办公，不抢内容 | `#d98f4e` | `#b06f2e` |
| `cool-gray` | 冷灰 | 蓝灰极简，开发者向 | `#63b3ed` | `#2b6cb0` |
| `pink-white` | 粉白 | 柔和暖调，浅色主打 | `#ff7eb6` | `#e0448c` |
| `celadon` | 青瓷 | 清爽冷绿，阅读护眼 | `#3ddbb8` | `#0f9d81` |
| `moss-green` | 苔绿 | 自然沉稳，长时间写作 | `#68d391` | `#2f855a` |
| `honey-orange` | 蜜橙 | 高能量暖色，创作场景 | `#ffa94d` | `#e8720c` |
| `lavender` | 薰衣紫 | 与 AI 面板品牌色呼应 | `#a78bfa` | `#7c4fe0` |

> 上表为 B-02 草案**原值**；实测报告显示 65 处对比度硬伤，**落地前须先应用实测报告 §五 的最小修正**（尤其深色 accent 压深撑白字、浅色 accent 由 hover 紧约束定值），否则新预设继承既存的 a11y 缺口。

## 三、优先级与交互（三键模型）

`theme_preset`（配色方案）× `theme`（深/浅变体）× `theme_color`（自定义 accent 覆盖）：

- **accent 来源优先级**：`theme_color`（非空自定义）> `theme_preset`（预设 accent）> 硬编码默认。
  → 已设自定义 `theme_color` 的用户，其自定义 accent 继续优先，**保住既有自定义行为**。
- **`theme`（dark/light）选择预设的深/浅变体**；`theme_preset` 不改 `theme` 的值，只给当前主题模式提供色集。
- **未知值兜底**：`theme_preset` 为不认识的 ID（如改名后）→ 读侧**降级为 `starship-blue`**，不报错、不白屏（写侧另可加白名单校验）。

## 四、迁移与兼容口径

1. **向后兼容**：新键，存量用户无值 → 读回默认 `starship-blue` = 现行外观，**零变化**；无需数据迁移。
2. **不迁移旧数据**：`theme_color` 保持原语义（自定义 accent 覆盖），不与 `theme_preset` 互相转换。
3. **`reset_settings`（一键重置）**：`theme_preset` 是普通设置键（非敏感/凭据/内部键）→ 重置时**回到 `starship-blue`**（在重置范围内，符合 028 件二的重置口径）。
4. **`ts_export` 影响**：加进 `SETTINGS` 后重生成 `constants.ts`，前端设置 store 获得新键（即“新键进白名单”契约要求）；落地时须跑 `cargo test --lib ts_export` 重生成 + 同步前端默认值。
5. **敏感/掩码**：不在 `SENSITIVE_KEYS`（`master_key`）/ `MASKED_KEYS`（`ai_api_key`/`opencode_password`）之列，读侧明文可返回。

## 五、落地分期（建议，供拍板）

- **一期（最小，建议作为落地小包）**：`theme_preset` 只驱动 **accent 族**（`--accent`/`-hover`/`-press`/`-soft`/`-ring`/`--grad-accent`），**App.vue 主界面 + newtab 双端消费同一键**（消除“启动页一个色、主界面另一个色”的口径分裂）；bg/text 仍由现 `theme` token 集提供。
- **二期（全量）**：`theme_preset` 同时驱动 bg/text 色阶（`--bg-*`/`--text-*`）。
- **三期**：标签组/空间色板（groupPalette/spaceColors）联动（B-02 二期）。
- 每期可独立交付、独立回滚；**一期即能兑现 B-02 第一目标**（强调色从启动页提升到全局 accent 族）。

## 六、与对比度实测报告的关系

- 预设色值落地时**必须带上实测报告 §五 的修正**，否则 65 处硬伤（含现行默认主题既有的按钮白字/浅底链接/弱化文字缺口）原样带入。
- 一期落地小包建议范围：`theme_preset` 键（含单测）+ §五 修正 + App.vue/newtab 双端消费 + **axe 深浅双跑回归**（实测报告的局限 1：本次是公式精算，落地需真跑 axe 全站）。

## 七、待拍板点（呈高哥）

1. **是否落地 `theme_preset`、落哪一期**（建议一期：仅 accent 族 + 双端消费 + §五 修正 + axe 回归）。
2. **默认值**：`starship-blue`（=现行，推荐）还是空串（语义=“未选预设”）。
3. **优先级口径**：`theme_color` 自定义覆盖预设，是否符合预期（本草案判断：是，保住既有自定义行为）。
4. **`reset_settings` 是否重置 `theme_preset`**（本草案判断：是，回到默认预设）。
