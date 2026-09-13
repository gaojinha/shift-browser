import pluginVue from 'eslint-plugin-vue'
import vueTsConfigs from '@vue/eslint-config-typescript'
import skipFormatting from '@vue/eslint-config-prettier/skip-formatting'

export default [
  {
    name: 'app/files-to-lint',
    files: ['**/*.{ts,mts,tsx,vue}'],
  },

  {
    name: 'app/files-to-ignore',
    ignores: ['**/dist/**', '**/dist-ssr/**', '**/coverage/**', 'src-tauri/**'],
  },

  // 注意：ESLint 9.39 的 @eslint/config-array 不支持数组里嵌套数组，
  // 必须用 ... 展开（否则报 "Unexpected array"）
  ...pluginVue.configs['flat/essential'],
  ...vueTsConfigs(),
  skipFormatting,
]