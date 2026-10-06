// Pure helpers for the monthly limit card (tested in tools/test-data-limit.cjs).

import { t } from '../../i18n'

export const GIB = 1024 ** 3

/** "300" or "1.5" (GB, binary like the stock UI) → bytes. */
export function parseLimitGb(text: string): { ok: true; bytes: number } | { ok: false; error: string } {
  const s = text.trim()
  if (!/^\d+(\.\d{1,2})?$/.test(s)) return { ok: false, error: t('Enter a number of GB, e.g. 100') }
  const gb = Number(s)
  if (gb <= 0 || gb > 1_048_576) return { ok: false, error: t('Between 0.01 and 1048576 GB') }
  return { ok: true, bytes: Math.round(gb * GIB) }
}

export function parseAlertPercent(text: string): { ok: true; percent: number } | { ok: false; error: string } {
  const s = text.trim()
  if (!/^\d+$/.test(s)) return { ok: false, error: t('Enter a whole percentage') }
  const p = Number(s)
  if (p < 1 || p > 99) return { ok: false, error: t('1 to 99') }
  return { ok: true, percent: p }
}

export function bytesToGbText(bytes: number | undefined): string {
  if (!bytes) return ''
  const gb = bytes / GIB
  return Number.isInteger(gb) ? String(gb) : gb.toFixed(2).replace(/0+$/, '').replace(/\.$/, '')
}

/** Used share of the limit, or undefined when either side is unknown. */
export function limitPct(used: number | null | undefined, limit: number | undefined): number | undefined {
  if (used == null || !limit) return undefined
  return Math.round((used / limit) * 1000) / 10
}
