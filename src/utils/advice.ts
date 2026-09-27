// 页面数据一句话助手（纯函数，可单测）：权限用途、证书研判、自检处置
// 调用方：PermissionPrompt / CertWarning / SelfCheckSection（组件只负责展示）

/** 权限用途+建议：用户看不懂英文 kind，必须翻成人话 */
export function adviceFor(kind: string): { use: string; advice: string } {
  const map: Record<string, { use: string; advice: string }> = {
    media: {
      use: '网站能打开你的摄像头、麦克风，看到听到你这边。',
      advice: '建议拒绝；视频会议等确实需要时再允许，用完在设置里删掉。',
    },
    geolocation: {
      use: '网站能知道你的大概位置（城市级）。',
      advice: '建议拒绝；地图导航确实需要时再允许。',
    },
    notification: {
      use: '网站能给你推送消息通知。',
      advice: '按需允许；不认识的站建议拒绝，免得广告轰炸。',
    },
    'pointer-lock': {
      use: '网站能锁定鼠标指针（常见于网页游戏、全屏应用）。',
      advice: '玩游戏时允许，平时拒绝。',
    },
    'device-info': {
      use: '网站能读取设备信息，可能被用来追踪识别你。',
      advice: '建议拒绝。',
    },
  }
  return (
    map[kind] ?? { use: '网站请求了一项不常见的权限。', advice: '不认识就拒绝。' }
  )
}

/** 证书错误一句话研判：能不能进、怎么进最安全 */
export function verdictFor(reason: string): string {
  if (reason.includes('过期')) return '证书过期了：内容可能被偷看，建议返回；自己的站且刚续费可继续。'
  if (reason.includes('未知') || reason.includes('自签'))
    return '自签或内网证书常见，公网大站出现就是异常，建议返回。'
  if (reason.includes('不匹配')) return '证书和主机名对不上，极可能是钓鱼或劫持，建议返回。'
  if (reason.includes('吊销')) return '证书已被签发者作废，不要继续。'
  if (reason.includes('尚未生效')) return '证书还没到生效时间，先检查系统时间对不对。'
  if (reason.includes('不安全')) return '加密算法强度不够，建议返回。'
  return '原因不明，谨慎起见建议返回。'
}

/** 自检红项一句话处置：修法只说真实存在的入口 */
export function fixFor(name: string): string {
  if (name.includes('内容过滤器')) return '去「设置 → 隐私防护」开关一次广告拦截，等编译完成再查。'
  if (name.includes('密码主密钥')) return '检查数据目录权限，或恢复主密钥备份；缺失则已存密码解不开。'
  if (name.includes('数据库') || name.includes('数据表'))
    return '先确认磁盘没满、数据目录可写，再重启浏览器重查。'
  if (name.includes('数据目录')) return '确认主目录存在可写；snap/容器环境看挂载路径。'
  if (name.includes('主窗口') || name.includes('标签管理器')) return '重启浏览器；仍红则看日志报上来。'
  if (name.includes('隐私') || name.includes('广告拦截')) return '去「设置 → 隐私防护」核对开关档位。'
  if (name.includes('扩展')) return '去扩展管理看哪家没启用。'
  return '重启重查一次；仍红把这行原文报上来。'
}
