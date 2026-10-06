// Formatting helpers. Signal-quality thresholds live in ./data/signalQuality.

import { LEVEL_LABEL, LEVEL_TONE, classifySignal, toneBgClass, toneTextClass } from './data/signalQuality'
import type { SignalLevel } from './data/signalQuality'
import { t } from './i18n'

/** Unknown (null/undefined/non-finite/negative) renders as an em dash; a real 0 is "0 B". */
export function formatBytes(bytes: number | null | undefined): string {
  if (bytes == null || !Number.isFinite(bytes) || bytes < 0) return '\u2014'
  if (bytes >= 1e12) return `${(bytes / 1e12).toFixed(1)} TB`
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`
  if (bytes >= 1e3) return `${(bytes / 1e3).toFixed(0)} KB`
  return `${bytes} B`
}

export function formatSpeed(bps: number): string {
  const mbps = (bps * 8) / 1_000_000
  if (mbps >= 1) return `${mbps.toFixed(1)} Mbps`
  const kbps = (bps * 8) / 1000
  return `${kbps.toFixed(0)} Kbps`
}

export function parseBandwidthMHz(bandwidth?: string): number {
  if (!bandwidth || bandwidth === '\u2014') return 0
  const match = bandwidth.match(/\d+(?:\.\d+)?/)
  return match ? parseFloat(match[0]) : 0
}

export function sumBandwidthMHz(carriers: { bandwidth?: string }[]): number {
  return carriers.reduce((sum, c) => sum + parseBandwidthMHz(c.bandwidth), 0)
}

export function formatBandwidthMHz(mhz: number): string {
  if (mhz <= 0) return '\u2014'
  return `${Number.isInteger(mhz) ? mhz.toFixed(0) : mhz.toFixed(1)} MHz`
}

export function formatUptime(secs?: number | null): string {
  if (!secs) return '\u2014'
  const d = Math.floor(secs / 86400)
  const h = Math.floor((secs % 86400) / 3600)
  const m = Math.floor((secs % 3600) / 60)
  return [d && t('{n}d', { n: d }), (d || h) && t('{n}h', { n: h }), t('{n}m', { n: m })].filter(Boolean).join(' ')
}

/** Usage-counter time: unknown is an em dash, a measured 0 is "0m". */
export function formatCounterTime(secs?: number | null): string {
  if (secs == null || !Number.isFinite(secs) || secs < 0) return '\u2014'
  return secs === 0 ? t('{n}m', { n: 0 }) : formatUptime(secs)
}

export function formatDuration(secs: number): string {
  if (!Number.isFinite(secs) || secs <= 0) return t('{n}s', { n: 0 })
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const s = Math.floor(secs % 60)
  if (h > 0) return `${t('{n}h', { n: h })} ${t('{n}m', { n: m })}`
  if (m > 0) return `${t('{n}m', { n: m })} ${t('{n}s', { n: s })}`
  return t('{n}s', { n: s })
}

// ── Signal quality ────────────────────────────────────────────────────────────
// Thresholds, labels and tones live in ./data/signalQuality (one policy table);
// these helpers only adapt it to the class-name shapes the UI already uses.

export type Quality = SignalLevel

export function rsrpQuality(rsrp?: number | null): Quality {
  return classifySignal('rsrp', rsrp).level
}

export function rsrqQuality(rsrq?: number | null): Quality {
  return classifySignal('rsrq', rsrq).level
}

export function sinrQuality(sinr?: number | null): Quality {
  return classifySignal('sinr', sinr).level
}

export function qualityLabel(q: Quality): string {
  return LEVEL_LABEL[q]
}

/** Tailwind text color class for a quality level. */
export function qualityText(q: Quality): string {
  return toneTextClass(LEVEL_TONE[q])
}

/** Tailwind bg class for status dots / bars. */
export function qualityBg(q: Quality): string {
  return toneBgClass(LEVEL_TONE[q])
}

export function rsrpColorClass(rsrp?: number | null): string {
  return toneTextClass(classifySignal('rsrp', rsrp).tone)
}

export function rsrqColorClass(v?: number | null): string {
  return toneTextClass(classifySignal('rsrq', v).tone)
}

export function sinrColorClass(v?: number | null): string {
  return toneTextClass(classifySignal('sinr', v).tone)
}

export function tempColorClass(c?: number): string {
  if (c == null) return 'text-ink3'
  if (c > 80) return 'text-danger'
  if (c > 60) return 'text-warn'
  return 'text-ok'
}

export function modemMode(type?: string): string {
  const raw = (type ?? '').toUpperCase()
  if (!raw) return '\u2014'
  if (raw.includes('ENDC') || raw.includes('NSA')) return 'ENDC'
  if (raw.includes('SA')) return 'SA'
  if (raw.includes('LTE') || raw === '4G') return 'LTE'
  if (raw.includes('NR') || raw.includes('5G')) return '5G'
  return raw
}
