// Guest Wi-Fi editor: draft, validation and the request (tested in tools/test-guest-wifi.cjs).
// Mirrors agent/src/wifi.rs: content fields go to both guest bands; on/off and the time limit go
// through the stock call. The stock UI refuses an open guest network without a time limit.

import { t } from '../../i18n'
import type { GuestWifi } from '../../types'
import { SSID_FORBIDDEN, type WifiPatch } from './wifiDraft'

/** Securities the agent and the stock guest page both accept. */
export const GUEST_SECURITY = ['none', 'psk2+ccmp', 'sae-mixed', 'sae'] as const
/** Minutes; 0 = no limit (the stock UI's options). */
export const GUEST_TIMES = [0, 120, 240, 480, 720] as const

export interface GuestDraft {
  enabled: boolean
  ssid: string
  security: string
  /** '' = keep the current password. */
  password: string
  hidden: boolean
  minutes: number
}

export function draftFromGuest(g: GuestWifi): GuestDraft {
  return {
    enabled: g.enabled_2g || g.enabled_5g,
    ssid: g.ssid ?? '',
    security: g.security ?? 'none',
    password: '',
    hidden: g.hidden,
    minutes: g.active_minutes ?? 240,
  }
}

export function securityLabel(s: string): string {
  return (
    { none: t('Open (no password)'), 'psk2+ccmp': 'WPA2', 'sae-mixed': 'WPA2/WPA3', sae: 'WPA3' }[s] ?? s
  )
}

export function timeLimitLabel(minutes: number): string {
  return minutes === 0 ? t('No limit') : t('{n} h', { n: minutes / 60 })
}

/** "1 h 05 min left" style countdown text. */
export function leftLabel(secs: number): string {
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  return h > 0 ? t('{h} h {m} min left', { h, m: String(m).padStart(2, '0') }) : t('{m} min left', { m: Math.max(1, m) })
}

const byteLength = (s: string) => new TextEncoder().encode(s).length
// eslint-disable-next-line no-control-regex
const CONTROL = /[\u0000-\u001f\u007f]/

export interface GuestErrors {
  ssid?: string
  password?: string
  minutes?: string
}

export function validateGuest(d: GuestDraft, g: GuestWifi): GuestErrors {
  const e: GuestErrors = {}
  const len = byteLength(d.ssid)
  if (len < 1 || len > 32) e.ssid = t('The network name must be 1–32 bytes.')
  else if (SSID_FORBIDDEN.test(d.ssid) || CONTROL.test(d.ssid))
    e.ssid = t('The network name cannot contain quotes, ; $ ` \\ | < > & or control characters.')
  if (d.security !== 'none') {
    const needsNew = d.password !== '' || !g.has_key || (g.security ?? 'none') === 'none'
    const pl = byteLength(d.password)
    if (needsNew && CONTROL.test(d.password)) e.password = t('The password cannot contain control characters.')
    else if (needsNew && !(pl >= 8 && pl <= 63)) e.password = t('The password must be 8–63 characters.')
  }
  if (d.enabled && d.security === 'none' && d.minutes === 0)
    e.minutes = t('An open guest network needs a time limit. Set a password or choose a limit.')
  return e
}

/** Only what changed. Turning the network on or off covers both bands. */
export function buildGuestPatch(d: GuestDraft, g: GuestWifi): WifiPatch {
  const base = draftFromGuest(g)
  const p: Record<string, string | number> = {}
  if (d.ssid !== base.ssid) p.guest_ssid = d.ssid
  if (d.security !== base.security) p.guest_encryption = d.security
  if (d.password !== '' && d.security !== 'none') p.guest_key = d.password
  if (d.hidden !== base.hidden) p.guest_hidden = d.hidden ? '1' : '0'
  if (d.enabled !== base.enabled || (d.enabled && g.enabled_2g !== g.enabled_5g)) {
    p.guest_disabled_2g = d.enabled ? '0' : '1'
    p.guest_disabled_5g = d.enabled ? '0' : '1'
  }
  // Re-sending the limit when turning the network on restarts the firmware's timer.
  if (d.minutes !== base.minutes || (d.enabled && !base.enabled)) p.guest_active_time = String(d.minutes)
  return Object.freeze(p)
}
