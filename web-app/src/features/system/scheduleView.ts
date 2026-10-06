// Pure helpers for the sleep and scheduled-reboot card (tested in tools/test-schedule.cjs).

import { t } from '../../i18n'
import type { RebootSchedule } from '../../types'

export function sleepLabel(minutes: number): string {
  if (minutes < 0) return t('Never')
  if (minutes >= 60 && minutes % 60 === 0) return t('{n} h', { n: minutes / 60 })
  return t('{n} min', { n: minutes })
}

export function weekdayName(day: number): string {
  return [t('Sunday'), t('Monday'), t('Tuesday'), t('Wednesday'), t('Thursday'), t('Friday'), t('Saturday')][day] ?? '?'
}

const pad = (n: number) => String(n).padStart(2, '0')

export function formatTime(hour: number, minute: number): string {
  return `${pad(hour)}:${pad(minute)}`
}

/** "HH:MM" from a time input → hour and minute. */
export function parseTime(text: string): { hour: number; minute: number } | undefined {
  const m = /^(\d{1,2}):(\d{2})$/.exec(text.trim())
  if (!m) return undefined
  const hour = Number(m[1])
  const minute = Number(m[2])
  return hour <= 23 && minute <= 59 ? { hour, minute } : undefined
}

/** One sentence describing when the device reboots. */
export function scheduleSummary(s: RebootSchedule): string {
  if (!s.enabled) return t('Not scheduled')
  const start = formatTime(s.hour, s.minute)
  const when =
    s.window_hours > 0
      ? t('between {start} and {end}', { start, end: formatTime((s.hour + s.window_hours) % 24, s.minute) })
      : t('at {time}', { time: start })
  return s.mode === 'weekly'
    ? t('Every {day} {when}', { day: weekdayName(s.weekday), when })
    : s.interval_days === 1
      ? t('Every day {when}', { when })
      : t('Every {n} days after boot {when}', { n: s.interval_days, when })
}

/**
 * The fields that differ between the saved schedule and a draft. Weekly and interval keep separate
 * time slots on the device, so a mode change sends the whole time as shown.
 */
export function scheduleChanges(saved: RebootSchedule, draft: RebootSchedule): Partial<RebootSchedule> {
  const out: Partial<RebootSchedule> = {}
  const slot: (keyof RebootSchedule)[] = ['hour', 'minute', 'window_hours']
  for (const k of Object.keys(draft) as (keyof RebootSchedule)[]) {
    if (draft[k] !== saved[k] || (draft.mode !== saved.mode && slot.includes(k))) {
      ;(out as Record<string, unknown>)[k] = draft[k]
    }
  }
  return out
}

/** Mirrors agent/src/netsvc.rs watchdog checks. */
export function validateWatchdog(host: string, interval: string, failures: string): { host?: string; interval?: string; failures?: string } {
  const e: { host?: string; interval?: string; failures?: string } = {}
  const h = host.trim()
  const ipv4 = /^(\d{1,3}\.){3}\d{1,3}$/.test(h) && h.split('.').every((o) => Number(o) <= 255)
  const domain = !/^[\d.]+$/.test(h) && /^(?=.{1,253}$)([a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?\.)+[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?$/.test(h)
  if (!ipv4 && !domain) e.host = t('Enter an IPv4 address or a domain name')
  const i = /^\d+$/.test(interval.trim()) ? Number(interval) : NaN
  if (!(i >= 2 && i <= 1440)) e.interval = t('2 to 1440')
  const f = /^\d+$/.test(failures.trim()) ? Number(failures) : NaN
  if (!(f >= 1 && f <= 20)) e.failures = t('1 to 20')
  return e
}
