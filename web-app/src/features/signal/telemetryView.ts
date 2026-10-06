// Presentation logic shared by Home and the Signal overview (PLAN2 R01/R04/R05/R06).
// Pure (no React) so it is covered by tools/test-telemetry-view.cjs.
//
// Rules this module encodes:
//  - Ratings come only from data/signalQuality; no thresholds live here or in JSX.
//  - Unknown values are neutral and "Unavailable"; RSSI is shown but never rated.
//  - The serving readout uses signal.primary only, never a raw field.
//  - "Reported" carriers are everything the firmware lists; "active" is a subset.

import { classifySignal, signalUnit, toneTextClass } from '../../data/signalQuality'
import type { SignalMetric, SignalTone } from '../../data/signalQuality'
import { usageTotal } from '../../data/usage'
import { modemMode, parseBandwidthMHz } from '../../format'
import { t } from '../../i18n'
import type { CarrierComponent, DataUsage, SignalInfo, UsagePeriod } from '../../types'

// ── Metric help ───────────────────────────────────────────────────────────────

export const METRIC_HELP: Record<SignalMetric, string> = {
  rsrp: t('Reference Signal Received Power: power of a single LTE/NR reference signal. Primary indicator of signal strength.'),
  rsrq: t('Reference Signal Received Quality: signal quality accounting for noise and interference from neighbouring cells.'),
  sinr: t('Signal to Interference plus Noise Ratio: how far the signal is above the noise floor. A key factor in achievable throughput.'),
  rssi: t('Received Signal Strength Indicator: total wideband received power including signal, noise and interference. Shown without a rating.'),
}

export const METRIC_LABEL: Record<SignalMetric, string> = { rsrp: 'RSRP', rsrq: 'RSRQ', sinr: 'SINR', rssi: 'RSSI' }

export const RATING_NOTE = t(
  'Ratings are approximate link indicators, not a standard or a speed guarantee. RSSI is shown without a rating.',
)

// ── One measurement ───────────────────────────────────────────────────────────

export interface MetricView {
  metric: SignalMetric
  /** Raw reading as text (precision preserved), or null when unavailable. */
  text: string | null
  unit: string
  /** Quality word for rated metrics ("Excellent"), "Unavailable" when unknown, null for a known RSSI. */
  word: string | null
  level: string
  tone: SignalTone
  /** Tailwind text colour: tone colour for rated metrics, neutral otherwise. */
  className: string
  /** Full accessible description, e.g. "RSRP -54 dBm, Excellent". */
  description: string
}

export function metricView(metric: SignalMetric, value: unknown): MetricView {
  const c = classifySignal(metric, value)
  const known = typeof value === 'number' && Number.isFinite(value)
  const unit = signalUnit(metric)
  const name = METRIC_LABEL[metric]
  if (!known) {
    return {
      metric,
      text: null,
      unit,
      word: t('Unavailable'),
      level: 'unknown',
      tone: 'neutral',
      className: 'text-ink3',
      description: t('{name} unavailable', { name }),
    }
  }
  const text = String(value)
  if (metric === 'rssi') {
    return {
      metric,
      text,
      unit,
      word: null,
      level: 'unrated',
      tone: 'neutral',
      className: 'text-ink2',
      description: t('{name} {value} {unit}, not rated', { name, value: text, unit }),
    }
  }
  return {
    metric,
    text,
    unit,
    word: c.label,
    level: c.level,
    tone: c.tone,
    className: toneTextClass(c.tone),
    description: t('{name} {value} {unit}, {rating}', { name, value: text, unit, rating: c.label }),
  }
}

// ── Serving cell (R06) ────────────────────────────────────────────────────────

export interface ServingView {
  available: boolean
  carrier?: CarrierComponent
  /** "5G SA", "5G NR", "LTE anchor (NSA)" or "LTE"; null when nothing is serving. */
  rat: string | null
  /** "5G SA · n78"; null when unavailable. */
  label: string | null
  band?: string
  pci?: number
}

/** The primary readout, derived only from `signal.primary` (mapper-validated). */
export function servingView(signal: SignalInfo | null | undefined): ServingView {
  const primary = signal?.primary
  if (!primary) return { available: false, rat: null, label: null }
  const mode = modemMode(signal?.type)
  const rat =
    primary.rat === 'nr' ? (mode === 'SA' ? '5G SA' : '5G NR') : mode === 'ENDC' ? t('LTE anchor (NSA)') : 'LTE'
  const band = primary.carrier.band || undefined
  return {
    available: true,
    carrier: primary.carrier,
    rat,
    label: band ? `${rat} · ${band}` : rat,
    band,
    pci: primary.carrier.pci,
  }
}

/** Signal bars: undefined = not reported (never shown as 0); a genuine 0 stays 0. */
export function barsText(bars: number | undefined | null): string | null {
  return typeof bars === 'number' && Number.isFinite(bars) ? t('{n}/5 bars', { n: bars }) : null
}

// ── Carrier counts and bandwidth (R06) ────────────────────────────────────────

export interface CarrierCounts {
  /** Every carrier the firmware listed, including configured-but-idle SCCs. */
  reported: number
  active: number
  idle: number
  /** Carriers whose activity state the firmware did not give. */
  unknown: number
}

export function carrierCounts(carriers: CarrierComponent[]): CarrierCounts {
  let active = 0
  let idle = 0
  for (const c of carriers) {
    if (c.active === true) active++
    else if (c.active === false) idle++
  }
  return { reported: carriers.length, active, idle, unknown: carriers.length - active - idle }
}

export interface BandwidthSummary {
  /** Sum over every reported carrier (idle SCCs included). */
  reportedMHz: number
  /** Sum over carriers the firmware marks active. */
  activeMHz: number
  hasIdle: boolean
}

export function bandwidthSummary(carriers: CarrierComponent[]): BandwidthSummary {
  let reportedMHz = 0
  let activeMHz = 0
  let hasIdle = false
  for (const c of carriers) {
    const mhz = parseBandwidthMHz(c.bandwidth)
    reportedMHz += mhz
    if (c.active === true) activeMHz += mhz
    if (c.active === false) hasIdle = true
  }
  return { reportedMHz, activeMHz, hasIdle }
}

// ── Usage (R04/R05, Home) ─────────────────────────────────────────────────────

export interface UsageRow {
  label: string
  rx: number | null
  tx: number | null
  total: number | null
}

/** Home rows: the cycle counters are the device's month/cycle counters, not a calendar month. */
export function homeUsageRows(usage: DataUsage): UsageRow[] {
  const row = (label: string, p: UsagePeriod): UsageRow => ({ label, rx: p.rx_bytes, tx: p.tx_bytes, total: usageTotal(p) })
  return [row(t('Today'), usage.day), row(t('Current cycle'), usage.cycle ?? usage.month), row(t('Total'), usage.total)]
}
