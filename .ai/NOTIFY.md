# NOTIFY —— 交付即叫一声（回执哨兵 · 主动叫醒通道）

> 2026-10-19 立。高哥钦定：**协作 AI 交活儿的那一刻，必须主动把协调器（二狗）叫醒**。
> 起因：二狗不是常驻进程，没人给他发消息他就不会自己醒；此前交付全靠高哥开口才发现，
> 出现过"交付了但没人核、没人报"的漏检。

## 一句话

**交付动作的最后一步：跑一次 `notify-ergou`。不叫 = 等于没交。**

## 用法

在**本项目目录下**跑（两个 AI 的工作目录就是这里）：

```bash
./.ai/notify.sh <交付方> <卡号> <一句话摘要> [回执路径] [提交号]
```

真实例子：

```bash
./.ai/notify.sh atomcode F-102 "构建新包+真机闭环，回执已出" .ai/outbox/atomcode/20261019-xxxx-F102回执.md a1b2c3d
```

> **为什么用 `./.ai/notify.sh` 而不是 `notify-ergou`？**
> 真身装在 `/home/gao/bin/notify-ergou`，落在 **copilot 沙箱的信任目录之外**，
> copilot 的 bash 工具会被路径校验拦下（2026-10-19 实测原文：
> `Permission denied because no interactive user response was available`）。
> `.ai/notify.sh` 是项目内的薄包装，命令与路径都在信任目录里，两个 AI 都能直接跑。
> 真身仍可直接用（人/二狗在任意目录下跑 `notify-ergou` 都行）。

选项：

| 选项 | 作用 |
|---|---|
| `--dry-run` | 只演算不投递，把要发的话打出来（**首次用先干跑一遍**） |
| `--session=<sid>` | 指定目标会话（默认 = 高哥钉钉私聊那条主会话，即二狗本人） |

退出码：`0` 成功或幂等跳过 · `2` 参数错 · `3` 投递失败（已落待送队列）· `1` 其他错。

## 它干了什么

1. 从桥接 `config.json` 取认证、从 `sessions.json` 取**主会话 ID**（高哥私聊那条）
2. 幂等检查：`sha1(交付方|卡号|回执路径|提交号)` 已叫过就静默跳过（**同一份回执只叫一次**）
3. 主会话忙（二狗在跑长任务）→ 每 30 秒退避重试，最多 8 次（4 分钟），不硬塞
4. 投递一条固定格式的唤醒消息 → 二狗当场醒
5. 记账：`~/.local/state/ergou-notify/notified.log`；失败落 `undelivered.jsonl`

## 红线（防自激，务必遵守）

**只由"手"（交付方）调用。二狗自己写完 MESSAGES / WORKBOARD / 回执后绝不许调它。**

理由：2026-10-17 那套 `wakeup-daemon / copilot-inject / poll-daemon` 自动注入族，
就是因为"任何文件变动 → 注入 → 二狗写文件 → 再注入"自己把自己转晕，被整族退役。
本通道是**显式一次性调用**（交付方在交活儿那一刻调一次），链条在源头就断，不会自激。

## 兜底

- 交付方**忘了叫** / 调用失败：条目落在 `~/.local/state/ergou-notify/undelivered.jsonl`
- 二狗**每次被唤醒、每次答高哥话之前**，先扫一眼这个文件，补捡漏掉的
- `duty-tick.sh` 只读看门狗若发现 outbox 有新回执却没有对应 notify 记录，只**记一条待核提示**，
  不做自动注入（守住"只报不动"）

## 排障

| 症状 | 查什么 |
|---|---|
| `投递失败 HTTP 401` | 桥接 `config.json` 的 `opencode.password` 是否变了（本通道直接读它） |
| `找不到高哥私聊会话` | 桥接 `sessions.json` 里有没有 `dm:` 键；桥接会用 `/new` 重建映射 |
| 一直"主会话在忙" | 二狗在跑长任务；等下一轮，或让它落到待送队列 |
| 想确认通道活着 | `notify-ergou selftest SELF-CHECK "通道自检" --dry-run` |
