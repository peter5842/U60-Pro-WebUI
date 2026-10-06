// Confirmation copy for Wi-Fi changes that drop clients (PLAN2 R11). Pure: callers freeze the
// payload first, pass it here, and submit exactly that payload once if the dialog resolves true.
// Never contains a password; the browser's own connection is not guessed, only named as a possibility.

import { t } from '../../i18n'
import type { WifiBand } from '../../types'
import type { ConfirmOptions } from '../../ui/feedback'
import { describePatch, type WifiPatch } from './wifiDraft'

// Each consequence is one whole translated sentence group, so the Mobile-data note is repeated
// per message instead of being appended as a separate fragment.

export function masterOffConfirm(): ConfirmOptions {
  return {
    title: t('Turn off all Wi-Fi?'),
    kind: 'connection',
    confirmLabel: t('Turn off Wi-Fi'),
    details: [
      { label: t('Operation'), value: t('Turn Wi-Fi off') },
      { label: t('Radios'), value: t('2.4 GHz and 5 GHz') },
    ],
    consequence: t(
      'Every device connected over Wi-Fi, possibly this browser, will disconnect and this dashboard cannot be reached over Wi-Fi. Mobile data to the Internet is not changed.',
    ),
    recovery: t('Connect with USB-C or a cable, open this dashboard and turn Wi-Fi back on.'),
  }
}

export function radioOffConfirm(band: string, other: string): ConfirmOptions {
  return {
    title: t('Turn off {band} Wi-Fi?', { band }),
    kind: 'connection',
    confirmLabel: t('Turn off {band}', { band }),
    details: [
      { label: t('Operation'), value: t('Disable radio') },
      { label: t('Band'), value: band },
    ],
    consequence: t('Devices connected on {band}, possibly this browser, will disconnect. Mobile data to the Internet is not changed.', { band }),
    recovery: t('Reconnect to the {other} network or use USB-C, open this dashboard and turn {band} back on.', { other, band }),
  }
}

export function bandSaveConfirm(band: string, other: string, patch: WifiPatch, observed: WifiBand): ConfirmOptions {
  const renamed = Object.keys(patch).some((k) => k.startsWith('ssid_'))
  return {
    title: t('Apply {band} Wi-Fi changes?', { band }),
    kind: 'connection',
    confirmLabel: t('Apply changes'),
    details: [{ label: t('Band'), value: band }, ...describePatch(patch, observed)],
    consequence: renamed
      ? t(
          'Wi-Fi restarts to apply this. Devices connected on {band}, possibly this browser, will disconnect and must rejoin under the new name. Mobile data to the Internet is not changed.',
          { band },
        )
      : t('Wi-Fi restarts to apply this. Devices connected on {band}, possibly this browser, will disconnect. Mobile data to the Internet is not changed.', {
          band,
        }),
    recovery: renamed
      ? t('Reconnect to the new {band} network, or to {other} or USB-C, then reopen this dashboard. If a setting is wrong, change it back here.', {
          band,
          other,
        })
      : t('Reconnect to the {band} network, or to {other} or USB-C, then reopen this dashboard. If a setting is wrong, change it back here.', {
          band,
          other,
        }),
  }
}

export function syncConfirm(
  source: string,
  target: string,
  patch: WifiPatch,
  targetObserved: WifiBand,
  includePassword: boolean,
): ConfirmOptions {
  // The password row is added below (copied or not), so drop the generic "Changed (not shown)" one.
  const withoutPassword = Object.fromEntries(Object.entries(patch).filter(([key]) => !key.startsWith('key_')))
  return {
    title: t('Copy {source} settings to {target}?', { source, target }),
    kind: 'connection',
    confirmLabel: t('Copy settings'),
    details: [
      { label: t('Operation'), value: t('Copy {source} to {target}', { source, target }) },
      ...describePatch(withoutPassword, targetObserved),
      { label: t('Password'), value: includePassword ? t('Copied (not shown)') : t('Not copied') },
    ],
    consequence: t(
      'Wi-Fi restarts. Devices connected on {target}, possibly this browser, will disconnect and may need to rejoin. Mobile data to the Internet is not changed.',
      { target },
    ),
    recovery: t('Reconnect to {source} or USB-C, then reopen this dashboard.', { source }),
  }
}
