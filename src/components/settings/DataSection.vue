<script setup lang="ts">
// 设置弹窗 · 数据与同步分区（IA 重组：历史/清理/UA/证书 移至 PrivacyDataCard）
// 密码库主密码 / 上云 E2E 加密同步 / 启动锁屏
import { onMounted } from 'vue'
import { useSettingsStore } from '../../stores/settings'
import { useSettingsSet } from '../../composables/useSettingsSet'
import { useVault } from '../../composables/useVault'
import { useSync } from '../../composables/useSync'

const settingsStore = useSettingsStore()
const { onSet } = useSettingsSet()

/** 密码库主密码（阶段 2 · 数据主权底线） */
const { vaultStatus, vaultPassword, vaultBusy, vaultIdleMinutes, loadVaultStatus, saveVaultIdleLock, setVaultPassword, unlockVault, lockVault, removeVaultPassword } = useVault()
const { syncStatus, syncUrl, syncUser, syncPass, syncBusy, loadSyncStatus, saveSyncConfig, pushSync, pullSync, clearSync } = useSync()

onMounted(() => {
  loadVaultStatus()
  loadSyncStatus()
})
</script>

<template>
  <div class="section-stack">
    <div class="card">
      <div class="card-title"><AppIcon name="lock" :size="14" /> 数据与同步</div>
      <div class="kw-block">
        <div class="kw-hint">
          <b>密码库主密码</b>：设置后密码库启动即<strong>锁定</strong>，须输入主密码解锁
          ——同机拿到密钥文件也无法解密（数据主权底线）。
          <span v-if="vaultStatus">
            当前：<b>{{
              vaultStatus.protected
                ? vaultStatus.unlocked
                  ? '已设主密码 · 已解锁'
                  : '已设主密码 · 已锁定'
                : '未设主密码（仅密钥文件保护）'
            }}</b>
          </span>
        </div>
        <div class="kw-add">
          <input
            v-model="vaultPassword"
            type="password"
            :placeholder="
              vaultStatus?.protected ? '输入主密码' : '设置主密码（至少 8 位）'
            "
            spellcheck="false"
          />
          <button
            v-if="!vaultStatus?.protected"
            class="mini-btn primary"
            :disabled="vaultBusy"
            @click="setVaultPassword"
          >
            设置主密码
          </button>
          <template v-else>
            <button
              v-if="!vaultStatus.unlocked"
              class="mini-btn primary"
              :disabled="vaultBusy"
              @click="unlockVault"
            >
              解锁
            </button>
            <button v-else class="mini-btn" @click="lockVault">锁定</button>
            <button
              class="mini-btn danger"
              :disabled="vaultBusy"
              @click="removeVaultPassword"
            >
              移除主密码
            </button>
          </template>
        </div>
        <div v-if="vaultStatus?.protected" class="kw-add" style="margin-top: 8px">
          <span class="kw-hint" style="margin-right: 8px">
            空闲自动锁定（无操作 N 分钟后清内存主密码，0 = 关闭）：
          </span>
          <input
            v-model.number="vaultIdleMinutes"
            type="number"
            min="0"
            max="1440"
            style="width: 80px"
          />
          <span style="margin: 0 8px">分钟</span>
          <button class="mini-btn primary" @click="saveVaultIdleLock">保存</button>
        </div>
      </div>
      <div class="kw-block">
        <div class="kw-hint">
          <b>上云 E2E 加密同步（服务器零知识）</b>：把<strong>密码库 + 书签 + 会话 + 历史</strong>
          加密推送到自建 Nextcloud（WebDAV）。同步包用<strong>主密码</strong>整包加密，服务器只存密文，
          没主密码谁也解不开。须先设主密码并解锁。
          <span v-if="syncStatus">
            当前：<b>{{
              !syncStatus.vault_protected
                ? '未设主密码（禁止上云）'
                : !syncStatus.unlocked
                  ? '已锁定（请先解锁）'
                  : syncStatus.configured
                    ? '就绪 · ' + (syncStatus.host || '')
                    : '待配置 WebDAV'
            }}</b>
          </span>
        </div>
        <div class="kw-add sync-config">
          <input
            v-model="syncUrl"
            placeholder="WebDAV 地址，如 https://host/remote.php/dav/files/user"
            spellcheck="false"
          />
          <input
            v-model="syncUser"
            placeholder="用户名"
            spellcheck="false"
          />
          <input
            v-model="syncPass"
            type="password"
            placeholder="密码"
            spellcheck="false"
          />
          <button
            class="mini-btn"
            :disabled="syncBusy || !syncStatus?.vault_protected || !syncStatus?.unlocked"
            @click="saveSyncConfig"
          >
            保存配置
          </button>
        </div>
        <div class="btn-row">
          <button
            class="mini-btn primary"
            :disabled="syncBusy || !syncStatus?.ready"
            @click="pushSync"
          >
            ⬆ 推送到云
          </button>
          <button
            class="mini-btn"
            :disabled="syncBusy || !syncStatus?.ready"
            @click="pullSync"
          >
            ⬇ 从云合并
          </button>
          <button
            v-if="syncStatus?.configured"
            class="mini-btn danger"
            :disabled="syncBusy"
            @click="clearSync"
          >
            清除配置
          </button>
          <span v-if="syncStatus?.last_push_at" class="sync-meta">
            上次推送：{{ syncStatus.last_push_at }}
          </span>
          <span v-if="syncStatus?.last_pull_at" class="sync-meta">
            上次拉取：{{ syncStatus.last_pull_at }}
          </span>
        </div>
      </div>
      <div class="field">
        <label>启动锁屏（每次启动输入账号密码解锁）</label>
        <label class="switch">
          <input
            type="checkbox"
            :checked="settingsStore.get('lock_enabled') === 'true'"
            @change="onSet('lock_enabled', ($event.target as HTMLInputElement).checked ? 'true' : 'false')"
          />
          <span class="slider"></span>
        </label>
      </div>
    </div>
  </div>
</template>

<style scoped>
.settings-scroll { flex: 1; overflow-y: auto; padding: 14px 16px; }.settings-scroll::-webkit-scrollbar { width: 6px; }.settings-scroll::-webkit-scrollbar-thumb { background: var(--bg-active); border-radius: 3px; }.card { margin-bottom: 12px; }.card-title { display: flex; align-items: center; gap: 7px; font-size: 14px; color: var(--text-primary); font-weight: 600; margin-bottom: 12px; padding-bottom: 8px; border-bottom: 1px solid var(--border-subtle); }.card-title .app-icon { color: var(--accent); }.field { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 10px; }.field-desc { flex: 1; font-size: 12px; color: var(--text-secondary); }.field label { font-size: 13px; color: var(--text-secondary); flex-shrink: 0; }.switch { position: relative; display: inline-block; width: 36px; height: 20px; flex-shrink: 0; }.switch input { opacity: 0; width: 0; height: 0; }.slider { position: absolute; cursor: pointer; inset: 0; background: var(--bg-active); border-radius: 10px; transition: 0.2s; }.slider::before { content: ''; position: absolute; width: 14px; height: 14px; left: 3px; top: 3px; background: var(--text-muted); border-radius: 50%; transition: 0.2s; }.switch input:checked + .slider { background: var(--accent); }.switch input:checked + .slider::before { transform: translateX(16px); background: #fff; }.kw-block { margin: 4px 0 12px 0; padding: 10px 12px; background: var(--bg-primary); border: 1px solid var(--border-subtle); border-radius: 8px; }.kw-hint { font-size: 11px; color: var(--text-muted); margin-bottom: 8px; line-height: 1.4; }.kw-add { display: flex; gap: 8px; }.kw-add input { flex: 1; border: 1px solid var(--border-color); background: var(--bg-primary); color: var(--text-primary); border-radius: 7px; padding: 6px 10px; font-size: 13px; outline: none; -webkit-appearance: none; appearance: none; }.kw-add input:focus { border-color: var(--accent); }.btn-row { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; margin-top: 8px; }.sync-config { flex-wrap: wrap; margin-top: 6px; }.sync-config input { min-width: 180px; }.sync-meta { font-size: 11px; color: var(--text-muted); }.mini-btn { background: var(--bg-active); color: var(--text-primary); border: 1px solid var(--border-color); border-radius: 7px; padding: 5px 12px; font-size: 12px; font-weight: 500; cursor: pointer; transition: all 0.15s; white-space: nowrap; }.mini-btn:hover { background: var(--accent-soft); color: var(--accent); border-color: var(--accent); }.mini-btn:disabled { opacity: 0.45; cursor: default; }.mini-btn.primary { background: var(--accent); color: #fff; border-color: var(--accent); }.mini-btn.primary:hover { background: var(--accent-2); color: #fff; border-color: var(--accent-2); }.mini-btn.danger { color: #ff8a8a; background: rgba(229, 57, 53, 0.16); border-color: rgba(229, 57, 53, 0.45); }.mini-btn.danger:hover { background: rgba(229, 57, 53, 0.34); color: #ffb3b3; border-color: rgba(229, 57, 53, 0.6); }
</style>
