// Alerts derived from the home poll (no extra requests), and their dismissal
// episodes. Pure: no React, so tools/test-alerts.cjs can load it directly.
import { t } from '../i18n'
import type { HomeData } from '../types'

export type AlertLevel = 'warning' | 'error'

export interface Alert {
  /** Stable condition identity, e.g. `source:signal`, `battery:temperature`. Never carries a value. */
  id: string
  level: AlertLevel
  /** Stable wording for the condition at this level; announced to assistive technology. */
  title: string
  /** Current detail (ages, readings, firmware errors). Updates silently. */
  detail?: string
}

export interface AlertConditions {
  active: Alert[]
  /** Conditions the latest observation shows to be resolved. Unknown is neither active nor resolved. */
  resolved: Set<string>
}

const label = (source: string) => source.replaceAll('_', ' ')

/** Display names for the dashboard's freshness sources; unknown ones fall back to the raw name. */
const SOURCE_NAMES: Record<string, string> = {
  battery: t('Battery'),
  cpu: 'CPU',
  data_usage: t('Data usage'),
  signal: t('Signal'),
  speed: t('Throughput'),
  thermal: t('Temperature'),
  wan: 'WAN',
  wan6: 'WAN (IPv6)',
}

const sourceName = (source: string) =>
  SOURCE_NAMES[source] ?? `${label(source)[0].toUpperCase()}${label(source).slice(1)}`

export function deriveConditions(data: HomeData | null, error: string | null): AlertConditions {
  const active: Alert[] = []
  const resolved = new Set<string>()

  if (error) {
    active.push({
      id: 'dashboard:refresh', level: 'error',
      title: t('Dashboard refresh failed; displayed readings may be old'),
      detail: error,
    })
  } else if (data) {
    resolved.add('dashboard:refresh')
  }
  if (!data) return { active, resolved }

  for (const [source, freshness] of Object.entries(data.sources ?? {})) {
    const id = `source:${source}`
    if (!freshness.stale) {
      resolved.add(id)
      continue
    }
    const age = freshness.age_ms == null
      ? t('No successful reading')
      : t('Last reading {n}s ago', { n: Math.floor(freshness.age_ms / 1000) })
    active.push({
      id, level: 'warning',
      title: t('{source} unavailable', { source: sourceName(source) }),
      detail: `${age}${freshness.error ? `. ${freshness.error}` : ''}`,
    })
  }

  if (data.charge_control_error) {
    active.push({ id: 'charge-control', level: 'error', title: t('Charge control failed'), detail: data.charge_control_error })
  } else if (data.charge_control_error === null) {
    resolved.add('charge-control')
  }

  const { battery, thermal } = data
  const temp = battery?.temperature_c
  if (typeof temp === 'number') {
    if (temp >= 50) {
      active.push({ id: 'battery:temperature', level: 'error', title: t('Battery temperature critically high'), detail: `${temp.toFixed(0)}°C` })
    } else if (temp >= 45) {
      active.push({ id: 'battery:temperature', level: 'warning', title: t('Battery temperature high'), detail: `${temp.toFixed(0)}°C` })
    } else {
      resolved.add('battery:temperature')
    }
  }

  const percent = battery?.percent
  if (battery && typeof percent === 'number') {
    if (!battery.plugged && percent <= 5) {
      active.push({ id: 'battery:low', level: 'error', title: t('Battery critically low'), detail: `${percent}%` })
    } else if (!battery.plugged && percent <= 15) {
      active.push({ id: 'battery:low', level: 'warning', title: t('Battery low'), detail: `${percent}%` })
    } else {
      resolved.add('battery:low')
    }
  }

  const cpu = thermal?.cpu_temp_c
  if (typeof cpu === 'number') {
    if (cpu >= 90) {
      active.push({ id: 'cpu:temperature', level: 'error', title: t('CPU temperature critically high'), detail: `${cpu}°C` })
    } else if (cpu >= 75) {
      active.push({ id: 'cpu:temperature', level: 'warning', title: t('CPU temperature elevated'), detail: `${cpu}°C` })
    } else {
      resolved.add('cpu:temperature')
    }
  }

  return { active, resolved }
}

/** Condition id → the level it was dismissed at. */
export type Dismissals = ReadonlyMap<string, AlertLevel>

const rank = (level: AlertLevel) => (level === 'error' ? 2 : 1)

export function dismiss(dismissals: Dismissals, alert: Alert): Dismissals {
  return new Map(dismissals).set(alert.id, alert.level)
}

/**
 * End the episode of every dismissed condition that has been observed to
 * recover, so a later recurrence shows again. Returns the same object when
 * nothing changed. A condition that is merely unknown keeps its dismissal.
 */
export function endEpisodes(dismissals: Dismissals, resolved: ReadonlySet<string>): Dismissals {
  if (![...dismissals.keys()].some((id) => resolved.has(id))) return dismissals
  const next = new Map(dismissals)
  for (const id of resolved) next.delete(id)
  return next
}

/** Dismissed alerts stay hidden for their episode unless they escalate. */
export function isDismissed(dismissals: Dismissals, alert: Alert): boolean {
  const at = dismissals.get(alert.id)
  return at != null && rank(alert.level) <= rank(at)
}
