// R11 confirmation content for connection-affecting radio changes. Pure builders: the caller freezes
// the reviewed values, builds the options from them, and submits exactly those values after Confirm.
// Nothing here reads live state or secrets.

import { describeBandLock } from '../../../data/bands'
import { lang, t } from '../../../i18n'
import type { BandLockState } from '../../../types'
import type { ConfirmOptions } from '../../../ui/feedback'

export type Rat = 'nr' | 'lte'

export const ratName = (rat: Rat) => (rat === 'nr' ? 'NR (5G SA)' : 'LTE')
export const bandName = (rat: Rat, band: number) => (rat === 'nr' ? `n${band}` : `B${band}`)
export const bandList = (rat: Rat, bands: number[]) => bands.map((b) => bandName(rat, b)).join(', ')

/** Plain-language rendering of an observed lock, using the capability band list. */
export function describeObserved(type: Rat, state: BandLockState, supported: number[]): string {
  const d = describeBandLock(state, supported)
  switch (d.kind) {
    case 'unknown':
      return t('Unknown (the modem did not report it)')
    case 'automatic':
      return t('Automatic (no band restriction)')
    case 'all':
      return t('All bands')
    case 'subset':
      return bandList(type, d.bands)
  }
}

/** Internet (WAN) can drop; the dashboard is served by the router and stays reachable over its LAN. */
const WAN_CONSEQUENCE = t(
  'The modem reconnects, so mobile Internet (WAN) may drop for a while. This dashboard is served by the router itself, so it stays reachable over USB or Wi-Fi LAN; only the Internet connection is interrupted.',
)

/** Joins whole sentences into one paragraph; Chinese text takes no separating space. */
const sentences = (...parts: string[]) => parts.join(lang() === 'zh' ? '' : ' ')

export function networkModeConfirm(next: { value: string; label: string }, current: { value?: string; label?: string }): ConfirmOptions {
  return {
    title: t('Change network mode?'),
    confirmLabel: t('Change mode'),
    kind: 'connection',
    details: [
      { label: t('Operation'), value: t('Change network mode') },
      { label: t('New mode'), value: next.label },
      { label: t('Current mode'), value: current.label ?? current.value ?? t('Unknown') },
    ],
    consequence: WAN_CONSEQUENCE,
    recovery: current.label
      ? t('To undo, select "{mode}" and apply again, or use "Reset bands to automatic" if a band lock is limiting service.', {
          mode: current.label,
        })
      : t('If service does not return, choose a broader mode and apply again, or use "Reset bands to automatic".'),
  }
}

export function bandLockConfirm(type: Rat, bands: number[]): ConfirmOptions {
  return {
    title: t('Lock {tech} bands?', { tech: type === 'nr' ? 'NR' : 'LTE' }),
    confirmLabel: t('Apply lock'),
    kind: 'connection',
    details: [
      { label: t('Operation'), value: t('Lock {rat} bands', { rat: ratName(type) }) },
      { label: t('Bands'), value: bandList(type, bands) },
    ],
    consequence: sentences(
      WAN_CONSEQUENCE,
      t('If none of the selected bands has coverage here, there will be no mobile service until the lock is removed.'),
    ),
    recovery: t('Use "Reset bands to automatic" on this page to return to automatic band selection.'),
  }
}

export function bandResetConfirm(): ConfirmOptions {
  return {
    title: t('Reset band locks to automatic?'),
    confirmLabel: t('Reset bands'),
    kind: 'connection',
    details: [
      { label: t('Operation'), value: t('Reset band locks') },
      { label: t('Affected'), value: t('LTE and NR (5G SA) band locks') },
    ],
    consequence: WAN_CONSEQUENCE,
    recovery: t('Lock bands again from the band cards below if you still want a restriction.'),
  }
}

export interface CellTuple {
  tech: Rat
  pci: string
  earfcn: string
  /** NR band number (digits only); not used for LTE. */
  band?: string
}

export function cellLockConfirm(cell: CellTuple): ConfirmOptions {
  const details = [
    { label: t('Operation'), value: t('Lock {rat} cell', { rat: ratName(cell.tech) }) },
    { label: 'PCI', value: cell.pci },
    { label: cell.tech === 'nr' ? 'NR-ARFCN' : 'EARFCN', value: cell.earfcn },
  ]
  if (cell.tech === 'nr' && cell.band) details.push({ label: t('Band'), value: `n${cell.band}` })
  return {
    title: t('Lock to this cell?'),
    confirmLabel: t('Lock cell'),
    kind: 'connection',
    details,
    consequence: sentences(
      WAN_CONSEQUENCE,
      t('If this cell cannot be used, there will be no mobile service until the lock is removed.'),
    ),
    recovery: t('Use "Reset cell locks" on this page to return to automatic cell selection.'),
  }
}

export function cellResetConfirm(): ConfirmOptions {
  return {
    title: t('Reset cell locks to automatic?'),
    confirmLabel: t('Reset cells'),
    kind: 'connection',
    details: [
      { label: t('Operation'), value: t('Reset cell locks') },
      { label: t('Affected'), value: t('LTE and NR cell locks') },
    ],
    consequence: WAN_CONSEQUENCE,
    recovery: t('Lock a cell again from the serving cells list or the cell lock forms if you still want one.'),
  }
}
