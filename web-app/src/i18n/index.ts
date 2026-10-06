// UI language. English source strings are the keys; the Chinese catalog maps
// them to translations and anything missing falls back to English.
//
//   t('Apply')                       → 应用
//   t('Using {name}', { name })      → 正在使用 {name}
//
// Only call t() with a string literal (tools/test-i18n.cjs checks that every
// literal has a translation with the same placeholders). The language is read
// once per page load; changing it reloads the page.

import { ZH } from './zh'

export type Lang = 'zh' | 'en'

const STORAGE_KEY = 'u60_lang'

function detect(): Lang {
  try {
    const stored = localStorage.getItem(STORAGE_KEY)
    if (stored === 'zh' || stored === 'en') return stored
  } catch {
    // storage unavailable (private mode, tests)
  }
  // Chinese is the dashboard default; outside a browser (unit tests) keep English.
  return typeof window === 'undefined' ? 'en' : 'zh'
}

let current: Lang = detect()

if (typeof document !== 'undefined') document.documentElement.lang = current === 'zh' ? 'zh-CN' : 'en'

export function lang(): Lang {
  return current
}

export function setLang(next: Lang) {
  if (next === current) return
  try {
    localStorage.setItem(STORAGE_KEY, next)
  } catch {
    // ignore: the choice then lasts for this page only
  }
  current = next
  window.location.reload()
}

export function t(text: string, vars?: Record<string, string | number>): string {
  const template = current === 'zh' ? (ZH[text] ?? text) : text
  if (!vars) return template
  return template.replace(/\{(\w+)\}/g, (match, key: string) => (key in vars ? String(vars[key]) : match))
}
