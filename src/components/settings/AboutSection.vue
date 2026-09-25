<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { openUrl } from '@tauri-apps/plugin-opener'
import { useToastStore } from '../../stores/toast'

/** 「关于」页信息（只读，打开即显示） */
interface AboutInfo {
  appName: string
  version: string
  webkitVersion: string
  osArch: string
  dataDir: string
  repoUrl: string
  profile: string
  iconDataUrl: string
}

const aboutInfo = ref<AboutInfo | null>(null)
async function loadAbout() {
  try {
    aboutInfo.value = await invoke<AboutInfo>('app_about_cmd')
  } catch (e) {
    console.error('[settings] 读取关于信息失败:', e)
  }
}
/** 打开项目主页（外部浏览器） */
async function openRepo() {
  if (!aboutInfo.value?.repoUrl) return
  try {
    await openUrl(aboutInfo.value.repoUrl)
  } catch (e) {
    console.error('[settings] 打开项目主页失败:', e)
    useToastStore().error('打开项目主页失败')
  }
}
/** 打开数据目录（文件管理器） */
async function openDataDir() {
  if (!aboutInfo.value?.dataDir) return
  try {
    await openUrl(aboutInfo.value.dataDir)
  } catch (e) {
    console.error('[settings] 打开数据目录失败:', e)
    useToastStore().error('打开数据目录失败')
  }
}
onMounted(() => {
  loadAbout()
})

// AI Key 输入框：不回显掩码（get_all_settings 返回前4后4掩码），
// 留空 = 保持原值；输入新值才保存。hasAiKey 判断是否已配置（掩码非空即已配置）
</script>

<template>
  <div class="section-stack">
            <div class="card about-hero">
              <div class="about-logo">
                <img
                  v-if="aboutInfo?.iconDataUrl"
                  :src="aboutInfo.iconDataUrl"
                  alt="Shift Browser"
                  class="about-logo-img"
                />
                <AppIcon v-else name="shield" :size="30" />
              </div>
              <div class="about-name">{{ aboutInfo?.appName || 'Shift Browser' }}</div>
              <div class="about-version">
                v{{ aboutInfo?.version || '—' }}
                <span class="about-badge">{{ aboutInfo?.profile === 'debug' ? '调试版' : '正式版' }}</span>
              </div>
              <div class="about-tagline">本地优先的隐私浏览器，数据主权在你手里。</div>
              <div class="about-env">
                <span class="about-env-item"><AppIcon name="monitor" :size="12" /> WebKitGTK {{ aboutInfo?.webkitVersion || '—' }}</span>
                <span class="about-env-item"><AppIcon name="gear" :size="12" /> {{ aboutInfo?.osArch || '—' }}</span>
              </div>
            </div>

            <div class="card">
              <div class="card-title"><AppIcon name="sparkles" :size="14" /> 核心能力</div>
              <div class="about-feature"><span class="about-feature-icon">🛡</span><div><b>隐私防护 2.0</b><div class="about-feature-desc">反指纹加噪 / 统一化、追踪参数清洗、GPC 隐私信号</div></div></div>
              <div class="about-feature"><span class="about-feature-icon">🔒</span><div><b>数据主权</b><div class="about-feature-desc">主密码加密密码库、E2E 上云同步（服务器零知识）</div></div></div>
              <div class="about-feature"><span class="about-feature-icon">🔌</span><div><b>开放 API + MCP 工具</b><div class="about-feature-desc">本机脚本与智能体可安全调用浏览器能力</div></div></div>
              <div class="about-feature"><span class="about-feature-icon">📰</span><div><b>RSS 订阅 · 阅读模式 · AI 助手</b><div class="about-feature-desc">信息聚合与智能辅助一站完成</div></div></div>
            </div>

            <div class="card">
              <div class="card-title"><AppIcon name="lock" :size="14" /> 数据与隐私承诺</div>
              <div class="about-feature"><span class="about-feature-icon">📍</span><div><b>本地优先</b><div class="about-feature-desc">历史、书签、密码库全部存在本机</div></div></div>
              <div class="about-feature"><span class="about-feature-icon">📡</span><div><b>零遥测</b><div class="about-feature-desc">默认不收集任何数据，外联审计可随时验证</div></div></div>
              <div class="about-feature"><span class="about-feature-icon">🔓</span><div><b>开源透明</b><div class="about-feature-desc">代码仓库公开，可审计</div></div></div>
            </div>

            <div class="card">
              <div class="card-title"><AppIcon name="info" :size="14" /> 项目与致谢</div>
              <div class="about-link-row">
                <span class="about-link-label">项目主页</span>
                <button class="mini-btn" @click="openRepo">打开仓库</button>
              </div>
              <div class="about-link-row">
                <span class="about-link-label">数据目录</span>
                <button class="mini-btn" @click="openDataDir">打开文件夹</button>
              </div>
              <div class="field-hint" style="margin-top: 8px">
                技术栈：Tauri 2 · Rust · WebKitGTK · Vue 3<br />
                致谢：Lucide 图标（ISC）、Nextcloud（WebDAV 同步）、W3C GPC 规范
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
</style>
