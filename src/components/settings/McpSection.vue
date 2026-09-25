<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { errText, invokeOk } from '../../utils/invoke'
import { useToastStore } from '../../stores/toast'

// ---- MCP 工具层（阶段 1.3）：server 配置 + 工具列表 + 手动调用 + 审计 ----
interface McpServer {
  id: number
  name: string
  command: string
  args: string[]
  enabled: boolean
  allowedTools: string[]
  toolCount: number
}
interface McpTool {
  name: string
  serverName: string
  description: string
  inputSchema: Record<string, unknown>
}
interface AuditEntry {
  id: number
  serverName: string
  toolName: string
  arguments: string
  resultSummary: string
  success: boolean
  createdAt: string
}

const mcpServers = ref<McpServer[]>([])
const mcpTools = ref<McpTool[]>([])
const mcpAudit = ref<AuditEntry[]>([])
const mcpLoading = ref(false)
const mcpError = ref('')
// 新增表单
const mcpNewName = ref('')
const mcpNewCommand = ref('')
const mcpNewArgs = ref('')
const mcpNewAllowed = ref('')
// 手动调用
const mcpCallTool = ref('')
const mcpCallArgs = ref('{}')
const mcpCallResult = ref('')
const mcpCalling = ref(false)

async function loadMcp() {
  mcpLoading.value = true
  mcpError.value = ''
  try {
    mcpServers.value = await invokeOk<McpServer[]>('mcp_list_servers')
    mcpTools.value = await invokeOk<McpTool[]>('mcp_list_tools')
    mcpAudit.value = await invokeOk<AuditEntry[]>('mcp_get_audit_log', { limit: 30 })
  } catch (e) {
    mcpError.value = errText(e)
  } finally {
    mcpLoading.value = false
  }
}

async function addMcpServer() {
  mcpError.value = ''
  const name = mcpNewName.value.trim()
  const command = mcpNewCommand.value.trim()
  if (!name || !command) {
    mcpError.value = '名称与命令不能为空'
    return
  }
  let args: string[] = []
  let allowed: string[] = []
  try {
    args = mcpNewArgs.value.trim() ? JSON.parse(mcpNewArgs.value) : []
    allowed = mcpNewAllowed.value.trim() ? JSON.parse(mcpNewAllowed.value) : []
  } catch {
    mcpError.value = '参数/白名单必须是合法 JSON 数组'
    return
  }
  try {
    await invokeOk('mcp_add_server', { name, command, args, allowedTools: allowed })
    mcpNewName.value = ''
    mcpNewCommand.value = ''
    mcpNewArgs.value = ''
    mcpNewAllowed.value = ''
    useToastStore().success('MCP server 已添加')
    await loadMcp()
  } catch (e) {
    mcpError.value = errText(e)
  }
}

async function removeMcpServer(id: number) {
  try {
    await invokeOk('mcp_remove_server', { id })
    useToastStore().success('已删除')
    await loadMcp()
  } catch (e) {
    mcpError.value = errText(e)
  }
}

async function toggleMcpServer(s: McpServer) {
  try {
    await invokeOk('mcp_set_server_enabled', { id: s.id, enabled: !s.enabled })
    s.enabled = !s.enabled
    await loadMcp()
  } catch (e) {
    mcpError.value = errText(e)
  }
}

async function callMcpTool() {
  if (!mcpCallTool.value) return
  mcpCalling.value = true
  mcpCallResult.value = ''
  mcpError.value = ''
  try {
    const args = JSON.parse(mcpCallArgs.value || '{}')
    mcpCallResult.value = await invokeOk<string>('mcp_call_tool', {
      name: mcpCallTool.value,
      arguments: args,
    })
  } catch (e) {
    mcpCallResult.value = `❌ ${errText(e)}`
  } finally {
    mcpCalling.value = false
  }
}

// 弹窗显示时隐藏所有标签 webview（child 盖在 Vue UI 上），关闭时恢复

onMounted(loadMcp)
</script>

<template>
  <div class="section-stack">
            <div class="card">
              <div class="card-title"><AppIcon name="plug" :size="14" /> MCP 工具</div>
              <div class="field-hint">
                配置 MCP server（本地进程，stdio 传输）后，AI 助手对话会自动携带这些工具（function
                calling），可让 AI 读写文件、查日历等。每次工具调用都会写入审计日志。
              </div>
              <div v-if="mcpError" class="ext-error">{{ mcpError }}</div>

              <div class="mcp-add-row">
                <input v-model="mcpNewName" type="text" placeholder="名称（如 filesystem）" spellcheck="false" />
                <input v-model="mcpNewCommand" type="text" placeholder="命令（如 npx）" spellcheck="false" />
                <input v-model="mcpNewArgs" type="text" placeholder='参数 JSON（如 &quot;-y&quot;,&quot;@modelcontextprotocol/server-filesystem&quot;,&quot;/tmp&quot;）' spellcheck="false" />
                <input v-model="mcpNewAllowed" type="text" placeholder='工具白名单 JSON（空=全部，如 &quot;read_file&quot;）' spellcheck="false" />
                <button class="mini-btn primary" @click="addMcpServer" :disabled="mcpLoading">添加</button>
              </div>

              <div v-if="mcpServers.length === 0 && !mcpLoading" class="field-hint">
                还没有 MCP server。示例：命令 <code>npx</code>，参数
                <code>["-y","@modelcontextprotocol/server-filesystem","/tmp"]</code>
              </div>
              <div v-for="s in mcpServers" :key="s.id" class="mcp-server-row">
                <button
                  class="mini-btn"
                  :class="s.enabled ? 'primary' : ''"
                  @click="toggleMcpServer(s)"
                  :title="s.enabled ? '点击停用' : '点击启用'"
                >
                  {{ s.enabled ? '✅ 启用' : '⏸ 停用' }}
                </button>
                <span class="mcp-server-name">{{ s.name }}</span>
                <span class="mcp-server-command">{{ s.command }} {{ s.args.join(' ') }}</span>
                <span class="mcp-server-count">{{ s.toolCount }} 个工具</span>
                <button class="mini-btn danger" @click="removeMcpServer(s.id)">删除</button>
              </div>
            </div>

            <div class="card">
              <div class="card-title"><AppIcon name="puzzle" :size="14" /> 可用工具（{{ mcpTools.length }}）</div>
              <div v-if="mcpTools.length === 0" class="field-hint">暂无可用工具（需启用且能连通的 server）</div>
              <div v-for="t in mcpTools" :key="t.name" class="mcp-tool-row">
                <span class="mcp-tool-name">{{ t.name }}</span>
                <span class="mcp-tool-desc">{{ t.description || '(无描述)' }}</span>
              </div>
            </div>

            <div class="card">
              <div class="card-title"><AppIcon name="pointer" :size="14" /> 手动调用测试</div>
              <div class="mcp-add-row">
                <select v-model="mcpCallTool">
                  <option value="">选择工具…</option>
                  <option v-for="t in mcpTools" :key="t.name" :value="t.name">{{ t.name }}</option>
                </select>
                <input v-model="mcpCallArgs" type="text" placeholder='参数 JSON（如 {"path":"/tmp/a.txt"}）' spellcheck="false" />
                <button class="mini-btn primary" @click="callMcpTool" :disabled="mcpCalling || !mcpCallTool">
                  {{ mcpCalling ? '调用中…' : '调用' }}
                </button>
              </div>
              <pre v-if="mcpCallResult" class="mcp-result">{{ mcpCallResult }}</pre>
            </div>

            <div class="card">
              <div class="card-title"><AppIcon name="list" :size="14" /> 审计日志（最近 {{ mcpAudit.length }} 条）</div>
              <div v-if="mcpAudit.length === 0" class="field-hint">暂无工具调用记录</div>
              <div v-for="a in mcpAudit" :key="a.id" class="mcp-audit-row" :class="{ fail: !a.success }">
                <span class="mcp-audit-time">{{ a.createdAt }}</span>
                <span class="mcp-audit-tool">{{ a.serverName }} · {{ a.toolName }}</span>
                <span class="mcp-audit-args">{{ a.arguments }}</span>
                <span class="mcp-audit-result">{{ a.resultSummary || (a.success ? '(成功)' : '(失败)') }}</span>
              </div>
            </div>
  </div>
</template>

<style scoped>
.settings-scroll {
  flex: 1;
  overflow-y: auto;
  padding: 14px 16px;
}
.settings-scroll::-webkit-scrollbar {
  width: 6px;
}
.settings-scroll::-webkit-scrollbar-thumb {
  background: var(--bg-active);
  border-radius: 3px;
}
.card {
  margin-bottom: 12px;
}
.api-actions .selfcheck-btn,
.card-title {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 14px;
  color: var(--text-primary);
  font-weight: 600;
  margin-bottom: 12px;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--border-subtle);
}
.card-title .app-icon {
  color: var(--accent);
}
.field-hint {
  font-size: 12px;
  line-height: 1.6;
  color: var(--text-secondary);
  background: var(--bg-primary);
  border: 1px solid var(--border-subtle);
  border-radius: 8px;
  padding: 8px 10px;
  margin-bottom: 10px;
}
.field-hint code {
  color: var(--accent);
  font-family: var(--font-mono, monospace);
  font-size: 11px;
}
.ext-error {
  color: var(--danger);
  font-size: 12px;
  margin-bottom: 6px;
}
.mcp-add-row {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-bottom: 10px;
}
.mcp-add-row input,
.mcp-add-row select {
  flex: 1 1 180px;
  min-width: 120px;
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: 6px;
  padding: 6px 8px;
  color: var(--text-primary);
  font-size: 12px;
  outline: none;
}
.mcp-add-row input:focus,
.mcp-add-row select:focus {
  border-color: var(--accent);
}
.mcp-server-row {
  display: flex;
  align-items: center;
  gap: 8px;
  background: var(--bg-primary);
  border: 1px solid var(--border-subtle);
  border-radius: 8px;
  padding: 6px 10px;
  margin-bottom: 6px;
  font-size: 12px;
}
.mcp-server-name {
  font-weight: 600;
  white-space: nowrap;
}
.mcp-server-command {
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
}
.mcp-server-count {
  color: var(--text-secondary);
  white-space: nowrap;
}
.mcp-tool-row {
  display: flex;
  gap: 8px;
  padding: 4px 0;
  font-size: 12px;
  border-bottom: 1px dashed var(--border-subtle);
}
.mcp-tool-name {
  font-family: var(--font-mono, monospace);
  color: var(--accent);
  white-space: nowrap;
}
.mcp-tool-desc {
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.mcp-result {
  margin-top: 6px;
  background: var(--bg-primary);
  border: 1px solid var(--border-subtle);
  border-radius: 6px;
  padding: 8px;
  font-size: 12px;
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 160px;
  overflow: auto;
}
.mcp-audit-row {
  display: flex;
  gap: 8px;
  padding: 4px 0;
  font-size: 12px;
  border-bottom: 1px dashed var(--border-subtle);
  align-items: baseline;
}
.mcp-audit-row.fail {
  color: var(--danger);
}
.mcp-audit-time {
  color: var(--text-secondary);
  white-space: nowrap;
}
.mcp-audit-tool {
  font-weight: 600;
  white-space: nowrap;
}
.mcp-audit-args {
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
}
.mcp-audit-result {
  max-width: 40%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.mini-btn {
  background: var(--bg-active);
  color: var(--text-primary);
  border: 1px solid var(--border-color);
  border-radius: 7px;
  padding: 5px 12px;
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.15s;
  white-space: nowrap;
}
.mini-btn:hover {
  background: var(--accent-soft);
  color: var(--accent);
  border-color: var(--accent);
}
.mini-btn:disabled {
  opacity: 0.45;
  cursor: default;
}
.mini-btn.primary {
  background: var(--accent);
  color: #fff;
  border-color: var(--accent);
}
.mini-btn.primary:hover {
  background: var(--accent-2);
  color: #fff;
  border-color: var(--accent-2);
}
.mini-btn.danger {
  color: var(--danger);
  background: var(--danger-soft);
  border-color: transparent;
}
.mini-btn.danger:hover {
  background: var(--danger);
  color: #fff;
  border-color: var(--danger);
}
</style>
