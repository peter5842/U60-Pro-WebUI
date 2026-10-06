// The ONE signal-quality policy (PLAN2 R01). Thresholds, units and labels live
// in SIGNAL_POLICY; the classifiers, the legend and the legacy helpers in
// format.ts are all derived from it. Do not repeat numeric thresholds in JSX.
//
// These are product heuristics for a link-quality indication, not a 3GPP
// standard and not a throughput guarantee. Boundaries are inclusive at the
// lower edge of each level (RSRP -90 is "good", not "fair").

import { t } from '../i18n'

export type SignalMetric = 'rsrp' | 'rsrq' | 'sinr' | 'rssi'
export type SignalLevel = 'excellent' | 'good' | 'fair' | 'poor' | 'unknown'
export type SignalTone = 'ok' | 'warn' | 'danger' | 'neutral'

export interface SignalClassification {
  level: SignalLevel
  label: string
  tone: SignalTone
}

export interface SignalLegendRow {
  level: Exclude<SignalLevel, 'unknown'>
  label: string
  tone: SignalTone
  /** Boundary text without unit, e.g. "≥ −80", "−90 to −80", "< −100". */
  range: string
  /** Same with the unit appended, e.g. "≥ −80 dBm". */
  rangeWithUnit: string
}

type RatedLevel = Exclude<SignalLevel, 'unknown'>

interface MetricPolicy {
  unit: string
  /** Rated metrics list levels best to worst with their inclusive lower bound
   *  (null = no lower bound). RSSI has no rating and is always neutral. */
  levels: { level: RatedLevel; min: number | null }[] | null
}

export const LEVEL_LABEL: Record<SignalLevel, string> = {
  excellent: t('Excellent'),
  good: t('Good'),
  fair: t('Fair'),
  poor: t('Poor'),
  unknown: t('Unavailable'),
}

export const LEVEL_TONE: Record<SignalLevel, SignalTone> = {
  excellent: 'ok',
  good: 'ok',
  fair: 'warn',
  poor: 'danger',
  unknown: 'neutral',
}

export const SIGNAL_POLICY: Record<SignalMetric, MetricPolicy> = {
  rsrp: {
    unit: 'dBm',
    levels: [
      { level: 'excellent', min: -80 },
      { level: 'good', min: -90 },
      { level: 'fair', min: -100 },
      { level: 'poor', min: null },
    ],
  },
  rsrq: {
    unit: 'dB',
    levels: [
      { level: 'excellent', min: -10 },
      { level: 'good', min: -15 },
      { level: 'fair', min: -20 },
      { level: 'poor', min: null },
    ],
  },
  sinr: {
    unit: 'dB',
    levels: [
      { level: 'excellent', min: 20 },
      { level: 'good', min: 10 },
      { level: 'fair', min: 0 },
      { level: 'poor', min: null },
    ],
  },
  // RSSI includes signal, interference and noise and depends on bandwidth, so
  // it is displayed but never rated.
  rssi: { unit: 'dBm', levels: null },
}

export const UNAVAILABLE_LABEL = LEVEL_LABEL.unknown
export const NOT_RATED_LABEL = t('Not rated')

export function signalUnit(metric: SignalMetric): string {
  return SIGNAL_POLICY[metric].unit
}

/** Rating for a measurement. Non-finite / missing values are unknown (neutral). */
export function classifySignal(metric: SignalMetric, value: unknown): SignalClassification {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return { level: 'unknown', label: UNAVAILABLE_LABEL, tone: 'neutral' }
  }
  const levels = SIGNAL_POLICY[metric].levels
  if (!levels) return { level: 'unknown', label: NOT_RATED_LABEL, tone: 'neutral' }
  for (const { level, min } of levels) {
    if (min === null || value >= min) {
      return { level, label: LEVEL_LABEL[level], tone: LEVEL_TONE[level] }
    }
  }
  return { level: 'unknown', label: UNAVAILABLE_LABEL, tone: 'neutral' }
}

/** U+2212 so negative bounds read as typeset minus signs in the legend. */
function fmt(n: number): string {
  return n < 0 ? `−${Math.abs(n)}` : String(n)
}

/** Legend rows generated from the policy table; empty for unrated metrics (RSSI). */
export function signalLegend(metric: SignalMetric): SignalLegendRow[] {
  const { unit, levels } = SIGNAL_POLICY[metric]
  if (!levels) return []
  return levels.map(({ level, min }, i) => {
    const upper = i > 0 ? levels[i - 1].min : null
    let range: string
    if (upper === null && min !== null) range = `≥ ${fmt(min)}`
    else if (min === null && upper !== null) range = `< ${fmt(upper)}`
    else if (min !== null && upper !== null) range = t('{min} to {max}', { min: fmt(min), max: fmt(upper) })
    else range = ''
    return {
      level: level,
      label: LEVEL_LABEL[level],
      tone: LEVEL_TONE[level],
      range,
      rangeWithUnit: `${range} ${unit}`,
    }
  })
}

/** Tailwind text colour class for a tone (design tokens: ok / warn / danger / ink3). */
export function toneTextClass(tone: SignalTone): string {
  switch (tone) {
    case 'ok':
      return 'text-ok'
    case 'warn':
      return 'text-warn'
    case 'danger':
      return 'text-danger'
    default:
      return 'text-ink3'
  }
}

/** Tailwind background class for dots / bars. */
export function toneBgClass(tone: SignalTone): string {
  switch (tone) {
    case 'ok':
      return 'bg-ok'
    case 'warn':
      return 'bg-warn'
    case 'danger':
      return 'bg-danger'
    default:
      return 'bg-ink3'
  }
}
