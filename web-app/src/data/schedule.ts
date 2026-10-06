// Mappers for /api/device/sleep and /api/device/reboot-schedule (agent/src/schedule.rs).

import type { RebootSchedule, SleepSetting } from '../types'
import { boolLike, intInRange } from './validate'

export function mapSleep(d: Record<string, unknown>): SleepSetting {
  const minutes = intInRange(d.minutes, -1, 1440)
  if (minutes === undefined || minutes === 0) throw new Error('unexpected sleep time')
  const options = Array.isArray(d.options) ? d.options.filter((o): o is number => intInRange(o, -1, 1440) !== undefined && o !== 0) : []
  return { minutes, options }
}

/** Every field is needed to edit the schedule, so a partial answer is an error, not defaults. */
export function mapRebootSchedule(d: Record<string, unknown>): RebootSchedule {
  const enabled = boolLike(d.enabled)
  const mode = d.mode === 'weekly' || d.mode === 'interval' ? d.mode : undefined
  const weekday = intInRange(d.weekday, 0, 6)
  const interval_days = intInRange(d.interval_days, 1, 30)
  const hour = intInRange(d.hour, 0, 23)
  const minute = intInRange(d.minute, 0, 59)
  const window_hours = intInRange(d.window_hours, 0, 6)
  if (
    enabled === undefined ||
    mode === undefined ||
    weekday === undefined ||
    interval_days === undefined ||
    hour === undefined ||
    minute === undefined ||
    window_hours === undefined
  ) {
    throw new Error('unexpected reboot schedule')
  }
  return { enabled, mode, weekday, interval_days, hour, minute, window_hours }
}
