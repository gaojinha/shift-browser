import { invoke } from '@tauri-apps/api/core'
import { invokeOk } from '../utils/invoke'
import { useSettingsStore } from '../stores/settings'
import { useToastStore } from '../stores/toast'

/** 统一的设置写入入口（R2-1d 拆分抽出，供各设置分区 SFC 共用） */
export function useSettingsSet() {
  const settingsStore = useSettingsStore()

  async function onSet(key: string, value: string) {
    await settingsStore.set(key, value)
    // 书签栏显隐切换需同步通知 Rust 侧重算 top_offset
    if (key === 'show_bookmarks_bar') {
      try {
        await invokeOk('set_bookmarks_bar_visible', { visible: value === 'true' })
      } catch (e) {
        console.error('[settings] 书签栏显隐同步失败:', e)
        useToastStore().error('书签栏切换失败')
      }
    }
    // 垂直标签切换：通知 Rust 重算内容区几何（top 与 left 都变）
    if (key === 'vertical_tabs') {
      try {
        await invokeOk('set_vertical_tabs', { active: value === 'true' })
      } catch (e) {
        console.error('[settings] 标签栏方向同步失败:', e)
        useToastStore().error('标签栏方向切换失败')
      }
    }
    // 网络层拦截规则 / 自定义请求头 / GPC 开关：落盘到扩展文件（阶段3.2；热更新，约 2 秒生效）
    if (key === 'net_block_rules' || key === 'net_headers' || key === 'gpc_header') {
      try {
        await invoke('sync_ext_rules')
      } catch (e) {
        console.error('[settings] 写扩展规则/请求头文件失败:', e)
        useToastStore().error('保存网络层配置失败')
      }
    }
    // 启动页相关设置：保存后重新生成新标签页 HTML
    if (key === 'show_bg' || key === 'font_size' || key === 'search_engine') {
      try {
        await invokeOk('refresh_newtab')
      } catch (e) {
        console.error('[settings] 启动页刷新失败:', e)
      }
    }
  }

  return { onSet }
}
