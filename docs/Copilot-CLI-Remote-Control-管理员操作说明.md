# Copilot CLI Remote Control 管理员操作说明

适用提示：

> `Remote sessions are not enabled. Contact your organization administrator to enable remote sessions.`

## 先明确这是什么功能

Copilot CLI 的 `/remote on` 是将**当前用户自己的本地 CLI 会话**同步到 GitHub，让同一 GitHub 账号从 GitHub.com 或 GitHub Mobile 查看进度、回答问题和控制该会话。它不是把另一位用户或其他 AI 邀请进当前 CLI 的多人通讯功能；远程访问只对启动该会话的同一 GitHub 账号开放。

CLI 提示里的 “organization administrator” 是通用提示。GitHub 官方要求修改策略的人具有 **organization owner**、**enterprise owner**，或适用的企业 AI controls 管理权限；仅有日常所说的管理员身份不一定拥有这些权限。即使你是组织管理员，企业策略也可能由上级企业锁定。

## 没有组织：是否需要先创建一个？

**不需要为了开启 Remote Control 而创建 GitHub 组织。**GitHub 的 Remote Control 前置条件将 `Store local sessions in the Cloud` 组织/企业策略要求限定在 Copilot 席位由组织提供的用户。个人账号自己拥有 Copilot 席位时，应先在本机交互式 CLI 用 `/remote on` 启用；本文后续的组织/企业策略步骤不适用于纯个人席位。官方说明没有在 Remote Control 页面逐项列出所有个人套餐的适用范围，所以这里不额外推断套餐资格。

如果你确定没有组织席位、但仍收到组织管理员提示，请按以下顺序核对：

1. GitHub **Settings → Billing & licensing → Licensing**（或旧版 **Billing and licensing → Plans and usage**）确认当前 Copilot 方案和席位来源；“没有自己创建组织”不一定表示席位不是由组织分配。
2. 确认本机 Copilot CLI 登录的 GitHub 账号与个人订阅账号相同。必要时在 CLI 执行 `/logout` 后重新 `/login`。
3. 检查交互会话是否以 `--no-remote` 启动；如 `~/.copilot/settings.json` 有 `remoteSessions: false`，移除或改为 `true`，再重新启动交互会话并运行 `/remote on`。本机设置不能覆盖组织/企业服务端策略。
4. 确认 CLI 为当前版本，并记录完整错误文本、CLI 版本、登录账号类型和 Copilot 席位来源；若仍失败，通过 GitHub Support 排查账号资格或服务端策略状态。

## 管理员操作：开启正确的策略

目标策略名称是 **Store local sessions in the Cloud**。要允许远程控制，应将适用层级设为 **View and control**，而非关闭、未配置或只读的 **View from cloud**。

本节仅适用于 Copilot 席位由组织/企业提供的情况。

### 先检查组织级策略

1. 登录 GitHub.com，切换到发放你 Copilot 席位的组织。
2. 打开 **Organization Settings → Copilot → Policies**。
3. 找到 **Store local sessions in the Cloud**。
4. 若该项可编辑，将它设为 **View and control** 并保存。

如果你有多个 Copilot 席位，应检查实际为当前账号提供 Copilot Business/Enterprise 席位的组织。

### 再检查企业级策略覆盖

如果组织级选项被锁定、显示由企业管理，或组织设为 `Let organizations decide` 仍未生效：

1. 打开对应企业的 **Enterprise settings → AI controls → Copilot → Policies**。
2. 检查 **Store local sessions in the Cloud**。
3. 若企业策略设为 **Let organizations decide**，回到前面的组织设置，由组织 owner 设为 **View and control**。
4. 若企业强制关闭或只允许查看，只有企业 owner/具备对应 AI controls 权限的人能修改企业策略；组织 owner 不能覆盖。

不同 GitHub 页面布局或套餐可能使菜单名称略有差异；以策略的完整名称为准。若找不到该策略或无法保存，不要用 CLI 配置尝试绕过服务端策略。

## 策略开放后：启动并验证

需要在**本地终端的交互式 Copilot CLI 会话**中操作；`copilot -p ...` 这类程序化/一次性会话不支持远程控制。会话所在电脑必须在线且有互联网连接。

1. 在交互式会话输入 `/remote` 查看当前状态。
2. 输入 `/remote on` 启用。也可以启动交互会话时运行 `copilot --remote`。
3. 成功时 CLI 会显示 GitHub 远程访问信息；在 GitHub.com 的 Copilot Agents/Recent agent sessions 或 GitHub Mobile 中，用**同一个 GitHub 账号**打开会话。
4. 如仍显示策略错误，检查账号登录身份、席位所属组织、企业策略，以及下列设备级限制。

个人级默认设置（通常为 `~/.copilot/settings.json`）中的 `"remoteSessions": true`，或 `copilot --remote`，只能表达本地启用意图，**不能覆盖组织/企业策略**。策略允许之后，可用 `/remote on` 逐会话启用；如确需默认对交互会话开启，再参考官方设置说明。

## 仍然失败时按顺序排查

1. **权限角色**：确认自己是 organization owner / enterprise owner，或拥有企业的 **Manage enterprise AI controls** 权限；“管理员”称呼本身不足以证明具备权限。
2. **席位和策略归属**：确认当前登录账号的 Copilot 席位来自哪个组织/企业，并检查对应的策略。企业策略优先；若由企业锁定，组织层面无法覆盖。
3. **策略模式**：必须允许 `View and control`。`View from cloud` 仅同步、只读，不足以控制会话。
4. **企业设备管理限制**：企业 managed settings 的 `remoteControl` 可对承载会话的设备额外禁用远程控制，或要求控制端通过指定组织的 SSO 授权。即使云端策略已开放，这项设备限制仍可能阻止本机托管会话；请检查企业设备管理配置。
5. **会话条件**：使用交互式 CLI；会话仍在本机运行；电脑在线；工作目录的 GitHub 仓库远程地址可解析（若 CLI 报仓库解析问题，先修复该环境）。
6. **本地覆盖**：确认 CLI 启动参数没有 `--no-remote`，并检查 `~/.copilot/settings.json` 没有显式关闭 `remoteSessions`。重新启动一个交互式 CLI 会话后再试。
7. **联系 GitHub Support**：若策略明确为 `View and control` 且条件均满足仍被拒绝，提供组织/企业标识、Copilot 套餐与席位来源、CLI 版本、发生时间、命令和脱敏后的错误文本。不要发送 token、凭据、完整远程会话 URL 或会话私密内容。

## 你最初的目标如果是“多个 AI 协同”

Remote Control 不是多 AI 通讯。打开这项策略后，你可以远程操控**你自己的** Copilot CLI 会话，但其他 AI 不会因此收到任务或加入会话。要协调不同 AI 产品，需要另行搭建双方都能访问的 MCP 协调服务、任务 API 或共享消息通道，并各自接入。也可以使用 Copilot 内置子代理分担分析/测试等工作，但它们不等于可由此策略连接的外部 AI。

## 官方文档

- [About remote control of GitHub Copilot CLI sessions](https://docs.github.com/en/copilot/concepts/agents/copilot-cli/about-remote-control)
- [Steering a GitHub Copilot CLI session from another device](https://docs.github.com/en/copilot/how-tos/copilot-cli/use-copilot-cli/steer-remotely)
- [Managing policies and features for GitHub Copilot in your organization](https://docs.github.com/en/copilot/how-tos/administer-copilot/manage-for-organization/manage-policies)
- [Managing policies and features for GitHub Copilot in your enterprise](https://docs.github.com/en/copilot/how-tos/administer-copilot/manage-for-enterprise/manage-enterprise-policies)
- [GitHub Copilot policies for enterprises and organizations](https://docs.github.com/en/copilot/concepts/enterprise/policies)
- [Enterprise managed settings](https://docs.github.com/en/copilot/reference/enterprise-administrators/enterprise-managed-settings)
