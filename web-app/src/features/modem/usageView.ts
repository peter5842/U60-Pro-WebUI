// Presentation logic for the Data tab's billing-cycle block (PLAN2 R05, U03).
//
// Pure functions: no React, no Date. Everything shown comes from the device's own
// observations (`reset_enabled`, `reset_day`, `cycle_start`, `next_reset`); nothing is
// estimated from the browser clock.

import { isValidCalendarDate, formatCalendarDate } from '../../data/dates'
import { t } from '../../i18n'
import type { CalendarDate, DataUsage } from '../../types'

export type ResetState = 'enabled' | 'disabled' | 'unknown'

/** `null` enablement is unknown, never "disabled". */
export function resetState(u: Pick<DataUsage, 'reset_enabled'> | null | undefined): ResetState {
  if (!u) return 'unknown'
  return u.reset_enabled === true ? 'enabled' : u.reset_enabled === false ? 'disabled' : 'unknown'
}

const dateText = (d: CalendarDate | null | undefined): string | null =>
  d && isValidCalendarDate(d.year, d.month, d.day) ? formatCalendarDate(d) : null

export interface CycleView {
  state: ResetState
  /** "Enabled" / "Disabled" / "Unknown". */
  stateLabel: string
  /** Headline when automatic reset is not known to be on; null when enabled. */
  headline: string | null
  /** Reset day as text, or null when the device did not report a valid one. */
  resetDay: string | null
  /** Device-supplied cycle start, or null when missing/invalid. */
  cycleStart: string | null
  /** Whether a "Next reset" row may be shown at all (only when reset is known to be enabled). */
  showNextReset: boolean
  /** Device-supplied next reset date; null when missing/invalid (shown as unavailable). */
  nextReset: string | null
  /** Explanatory line under the counters. */
  note: string
}

export function cycleView(u: DataUsage): CycleView {
  const state = resetState(u)
  return {
    state,
    stateLabel: state === 'enabled' ? t('Enabled') : state === 'disabled' ? t('Disabled') : t('Unknown'),
    headline:
      state === 'disabled'
        ? t('Automatic reset disabled')
        : state === 'unknown'
          ? t('Automatic reset status unknown')
          : null,
    resetDay: u.reset_day != null ? String(u.reset_day) : null,
    cycleStart: dateText(u.cycle_start),
    showNextReset: state === 'enabled',
    nextReset: state === 'enabled' ? dateText(u.next_reset) : null,
    note:
      state === 'enabled'
        ? t('Counters are maintained by the router and reset automatically on the reset day.')
        : state === 'disabled'
          ? t('Automatic reset is disabled, so no reset is scheduled.')
          : t('The router did not report whether counters reset automatically.'),
  }
}

export type ResetDayParse = { ok: true; day: number } | { ok: false; error: string }

/** Whole number 1–31 only: no blanks, fractions, signs or exponents. */
export function parseResetDay(text: string): ResetDayParse {
  const trimmed = text.trim()
  if (!/^\d{1,2}$/.test(trimmed)) return { ok: false, error: t('Enter a whole number from 1 to 31.') }
  const day = Number(trimmed)
  if (day < 1 || day > 31) return { ok: false, error: t('Enter a whole number from 1 to 31.') }
  return { ok: true, day }
}

export interface ResetDayCopy {
  /** True when saving also switches automatic reset on. */
  turnsOn: boolean
  hint: string
  button: string
}

/** Saving a reset day also enables automatic reset on the agent; say so before it happens. */
export function resetDayCopy(state: ResetState): ResetDayCopy {
  if (state === 'enabled') {
    return { turnsOn: false, hint: t('Day of the month, 1 to 31.'), button: t('Save') }
  }
  return {
    turnsOn: true,
    hint:
      state === 'disabled'
        ? t('Day of the month, 1 to 31. Saving also turns automatic reset on.')
        : t('Day of the month, 1 to 31. Automatic reset status is unknown; saving also turns automatic reset on.'),
    button: t('Save and turn on automatic reset'),
  }
}
