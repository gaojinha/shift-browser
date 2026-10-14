// English locale (skeleton · R4-5 framework 2026-10-14)
// Keys must mirror zh-CN 1:1 — enforced by i18n.test.ts key-set equality.
export default {
  progress: {
    preparing: 'Preparing…',
    done: 'Done · {s}s',
    running: '{s}s',
    runningEta: '{s}s · {eta}s left',
    title: 'Translating {info}{failsText} — click to collapse',
    fails: ' ({n} segments untranslated)',
  },
  update: {
    available: 'New version v{latest} available (current v{current}) — see the About page or the Releases page',
  },
  commandPalette: {
    title: 'Command Palette',
    placeholder: 'Type a command, URL, or ask AI…',
    searchLabel: 'Command Palette Search',
    listLabel: 'Command Suggestions',
    empty: 'No matching items',
  },
}
