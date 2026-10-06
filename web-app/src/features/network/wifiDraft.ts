// Pure Wi-Fi band draft logic (PLAN2 R03, R09, R11): no React, unit-tested in tools/test-wifi-view.cjs.
//
// Vocabulary
//   observed  the editable subset of the last successful /api/wifi/status read, as a BandDraft
//   base      the latest observation the draft was started from (always follows the device)
//   draft     what the form shows
//   dirty     draft differs from base; a pristine draft follows the device, a dirty one never moves
//
// The patch sent to /api/wifi/settings contains ONLY the fields the user actually changed, so an
// unrelated save can never reset (for example) TX power.

import { t } from '../../i18n'
import type { WifiAll, WifiBand } from '../../types'

export type BandSuffix = '2g' | '5g'
export type WifiPatch = Readonly<Record<string, string | number>>

/** The agent substitutes this for a key it knows exists but did not return. */
export const MASKED_PASSWORD = '••••••••'

export interface BandDraft {
  ssid: string
  password: string
  /** 'auto' or a channel number. */
  channel: string
  /** UCI htmode, e.g. 'EHT80'. */
  htmode: string
  /** Free text from the TX power input. '' means "keep current" (omit from the request). */
  txpower: string
  hidden: boolean
}

export function normalizeConfiguredChannel(channel?: string): string {
  const raw = (channel ?? '').trim().toLowerCase()
  return !raw || raw === '0' || raw === 'auto' ? 'auto' : raw
}

export function formatBandwidthMode(mode?: string): string | undefined {
  if (!mode) return undefined
  const m = /^(EHT|HE|VHT|HT)(\d+)$/.exec(mode)
  return m ? `${m[2]} MHz (${m[1]})` : mode
}

export function draftFromBand(band: WifiBand): BandDraft {
  return {
    ssid: band.ssid ?? '',
    password: band.password ?? '',
    channel: normalizeConfiguredChannel(band.configuredChannel),
    htmode: band.configuredBandwidth ?? '',
    // Unknown stays unknown: never invent 100 or a "default".
    txpower: band.txpowerPercent != null ? String(band.txpowerPercent) : '',
    hidden: band.hidden,
  }
}

// ── TX power (R09) ────────────────────────────────────────────────────────────

export type TxPowerResult = { ok: true; value: number | null } | { ok: false; error: string }

export const TX_POWER_ERROR = t('Enter a whole number from 1 to 100.')

/** '' is valid and means "keep current" (value null). The firmware accepts integers 1–100 only. */
export function parseTxPower(text: string): TxPowerResult {
  const trimmed = text.trim()
  if (trimmed === '') return { ok: true, value: null }
  if (!/^\d{1,3}$/.test(trimmed)) return { ok: false, error: TX_POWER_ERROR }
  const n = Number(trimmed)
  if (n < 1 || n > 100) return { ok: false, error: TX_POWER_ERROR }
  return { ok: true, value: n }
}

/** Canonical text for comparing two TX inputs ('025' and '25' are the same setting). */
function txKey(text: string): string {
  const r = parseTxPower(text)
  return r.ok ? (r.value == null ? '' : String(r.value)) : `invalid:${text.trim()}`
}

// ── Validation (mirrors agent/src/wifi.rs::wifi_value) ───────────────────────

export interface DraftErrors {
  ssid?: string
  password?: string
  txpower?: string
}

export const SSID_FORBIDDEN = /['";$`\\|<>&]/
// eslint-disable-next-line no-control-regex
const CONTROL = /[\u0000-\u001f\u007f]/
const byteLength = (s: string) => new TextEncoder().encode(s).length

export function validateDraft(draft: BandDraft, base: BandDraft, security?: string): DraftErrors {
  const errors: DraftErrors = {}
  if (draft.ssid !== base.ssid) {
    const len = byteLength(draft.ssid)
    if (len < 1 || len > 32) errors.ssid = t('The network name must be 1–32 bytes.')
    else if (SSID_FORBIDDEN.test(draft.ssid) || CONTROL.test(draft.ssid))
      errors.ssid = t('The network name cannot contain quotes, ; $ ` \\ | < > & or control characters.')
  }
  if (draft.password !== base.password && security !== 'none') {
    const len = byteLength(draft.password)
    const hex64 = len === 64 && /^[0-9a-fA-F]{64}$/.test(draft.password)
    if (CONTROL.test(draft.password)) errors.password = t('The password cannot contain control characters.')
    else if (!((len >= 8 && len <= 63) || hex64)) errors.password = t('The password must be 8–63 characters.')
  }
  const tx = parseTxPower(draft.txpower)
  if (!tx.ok) errors.txpower = tx.error
  return errors
}

export const hasErrors = (e: DraftErrors) => Object.keys(e).length > 0

// ── Patch ─────────────────────────────────────────────────────────────────────

/** Only the intentionally changed fields. Password is sent only when its text changed. */
export function buildBandPatch(suffix: BandSuffix, draft: BandDraft, base: BandDraft): WifiPatch {
  const patch: Record<string, string | number> = {}
  if (draft.ssid !== base.ssid) patch[`ssid_${suffix}`] = draft.ssid
  if (draft.hidden !== base.hidden) patch[`hidden_${suffix}`] = draft.hidden ? '1' : '0'
  if (draft.password !== base.password) patch[`key_${suffix}`] = draft.password
  if (draft.channel && draft.channel !== base.channel) patch[`channel_${suffix}`] = draft.channel
  if (draft.htmode && draft.htmode !== base.htmode) patch[`htmode_${suffix}`] = draft.htmode
  const tx = parseTxPower(draft.txpower)
  if (tx.ok && tx.value != null && String(tx.value) !== txKey(base.txpower)) patch[`txpower_${suffix}`] = tx.value
  return Object.freeze(patch)
}

/** Copy SSID, security, hidden state and (when readable) the password from one band to the other. */
export function buildSyncPatch(
  source: WifiBand,
  targetSuffix: BandSuffix,
): { patch: WifiPatch; includePassword: boolean } | { error: string } {
  if (!source.ssid) return { error: t('source SSID is empty') }
  const patch: Record<string, string | number> = {
    [`ssid_${targetSuffix}`]: source.ssid,
    [`hidden_${targetSuffix}`]: source.hidden ? '1' : '0',
  }
  if (source.security) patch[`encryption_${targetSuffix}`] = source.security
  const includePassword = Boolean(source.password && source.password !== MASKED_PASSWORD)
  if (includePassword) patch[`key_${targetSuffix}`] = source.password as string
  return { patch: Object.freeze(patch), includePassword }
}

// ── Draft state (R03) ─────────────────────────────────────────────────────────

export interface DraftState {
  editing: boolean
  draft: BandDraft
  base: BandDraft
  /** The device changed this band's settings while the draft was dirty. */
  conflict: boolean
}

export function sameDraft(a: BandDraft, b: BandDraft): boolean {
  return (
    a.ssid === b.ssid &&
    a.password === b.password &&
    a.channel === b.channel &&
    a.htmode === b.htmode &&
    a.hidden === b.hidden &&
    txKey(a.txpower) === txKey(b.txpower)
  )
}

export const isDirty = (s: DraftState) => !sameDraft(s.draft, s.base)

export function initDraft(observed: BandDraft): DraftState {
  return { editing: false, draft: observed, base: observed, conflict: false }
}

/**
 * Fold a new observation into the state. An observation equal to the current base returns the SAME
 * state object (a re-read, or a sibling band refresh, is not a change). A pristine draft follows
 * the device; a dirty draft is kept and flagged as conflicting when the device really changed.
 */
export function reconcileDraft(state: DraftState, observed: BandDraft): DraftState {
  if (sameDraft(state.base, observed)) return state
  if (!isDirty(state)) return { ...state, draft: observed, base: observed, conflict: false }
  return { ...state, base: observed, conflict: !sameDraft(state.draft, observed) }
}

export const startEditing = (s: DraftState): DraftState => (s.editing ? s : { ...s, editing: true })
export const editDraft = (s: DraftState, patch: Partial<BandDraft>): DraftState => {
  const draft = { ...s.draft, ...patch }
  const next = { ...s, draft }
  // Editing back to the device's values clears an earlier conflict flag.
  return sameDraft(draft, s.base) ? { ...next, conflict: false } : next
}
/** Cancel and "Reload from device" both restore the latest observation. */
export const cancelDraft = (s: DraftState): DraftState => ({ editing: false, draft: s.base, base: s.base, conflict: false })
export const reloadDraft = (s: DraftState): DraftState => ({ ...s, draft: s.base, conflict: false })

// ── Read-back verification (R11) ──────────────────────────────────────────────

export type Verification =
  | { kind: 'verified'; checked: number }
  | { kind: 'mismatch'; fields: string[] }
  | { kind: 'unchecked' }

function observedValue(key: string, wifi: WifiAll): string | undefined {
  if (key === 'wifi_onoff') return wifi.master_supported ? (wifi.master_enabled ? '1' : '0') : undefined
  if (key === 'radio2_disabled') return wifi.band_2g.enabled ? '0' : '1'
  if (key === 'radio5_disabled') return wifi.band_5g.enabled ? '0' : '1'
  if (key.startsWith('guest_')) {
    const g = wifi.guest
    if (!g) return undefined
    switch (key) {
      case 'guest_ssid':
        return g.ssid
      case 'guest_encryption':
        return g.security
      case 'guest_hidden':
        return g.hidden ? '1' : '0'
      case 'guest_disabled_2g':
        return g.enabled_2g ? '0' : '1'
      case 'guest_disabled_5g':
        return g.enabled_5g ? '0' : '1'
      case 'guest_active_time':
        return g.active_minutes != null ? String(g.active_minutes) : undefined
      default:
        return undefined // passwords cannot be compared reliably
    }
  }
  const m = /^([a-z]+)_(2g|5g)$/.exec(key)
  if (!m) return undefined
  const band = m[2] === '2g' ? wifi.band_2g : wifi.band_5g
  switch (m[1]) {
    case 'ssid':
      return band.ssid
    case 'hidden':
      return band.hidden ? '1' : '0'
    case 'channel':
      return normalizeConfiguredChannel(band.configuredChannel)
    case 'htmode':
      return band.configuredBandwidth
    case 'txpower':
      return band.txpowerPercent != null ? String(band.txpowerPercent) : undefined
    case 'encryption':
      return band.security
    default:
      return undefined // passwords cannot be compared reliably
  }
}

/** Compare what was requested with a fresh read. Passwords and unknown values are not compared. */
export function verifyApplied(patch: WifiPatch, wifi: WifiAll): Verification {
  let checked = 0
  const fields: string[] = []
  for (const [key, want] of Object.entries(patch)) {
    const seen = observedValue(key, wifi)
    if (seen === undefined) continue
    checked++
    const expected = key.startsWith('channel_') ? normalizeConfiguredChannel(String(want)) : String(want)
    if (seen !== expected) fields.push(fieldLabel(key))
  }
  if (fields.length > 0) return { kind: 'mismatch', fields }
  return checked > 0 ? { kind: 'verified', checked } : { kind: 'unchecked' }
}

export function fieldLabel(key: string): string {
  if (key === 'wifi_onoff') return t('Master Wi-Fi switch')
  if (key.startsWith('radio')) return t('Radio')
  if (key.startsWith('guest_')) {
    return (
      {
        guest_ssid: t('Guest SSID'),
        guest_key: t('Guest password'),
        guest_encryption: t('Guest security'),
        guest_hidden: t('Guest hidden SSID'),
        guest_disabled_2g: t('Guest network (2.4 GHz)'),
        guest_disabled_5g: t('Guest network (5 GHz)'),
        guest_active_time: t('Guest time limit'),
      }[key] ?? key
    )
  }
  const prefix = key.replace(/_(2g|5g)$/, '')
  return (
    {
      ssid: 'SSID',
      key: t('Password'),
      hidden: t('Hidden SSID'),
      channel: t('Channel'),
      htmode: t('Width'),
      txpower: t('TX power'),
      encryption: t('Security'),
    }[prefix] ?? prefix
  )
}

/** A failed request with no HTTP reply (timeout, dropped link): the device may or may not have applied it. */
export function isUncertainFailure(error: unknown): boolean {
  return error instanceof Error && (error as { status?: number }).status == null
}

// ── Helpers for confirmation copy ─────────────────────────────────────────────

/** The settings the patch will change, never revealing a password. */
export function describePatch(patch: WifiPatch, band: WifiBand): { label: string; value: string }[] {
  const order = ['ssid', 'encryption', 'key', 'hidden', 'channel', 'htmode', 'txpower']
  const prior: Record<string, string | undefined> = {
    ssid: band.ssid,
    encryption: band.security,
    hidden: band.hidden ? '1' : '0',
    channel: normalizeConfiguredChannel(band.configuredChannel),
    htmode: band.configuredBandwidth,
    txpower: band.txpowerPercent != null ? String(band.txpowerPercent) : undefined,
  }
  const show = (prefix: string, v: string | undefined): string => {
    if (v == null || v === '') return t('unknown')
    if (prefix === 'hidden') return v === '1' ? t('Yes') : t('No')
    if (prefix === 'channel') return v === 'auto' ? t('Auto') : v
    if (prefix === 'htmode') return formatBandwidthMode(v) ?? v
    if (prefix === 'txpower') return `${v}%`
    return v
  }
  const rows: { prefix: string; label: string; value: string }[] = []
  for (const [key, want] of Object.entries(patch)) {
    const prefix = key.replace(/_(2g|5g)$/, '')
    if (!order.includes(prefix)) continue
    const value =
      prefix === 'key' ? t('Changed (not shown)') : `${show(prefix, prior[prefix])} → ${show(prefix, String(want))}`
    rows.push({ prefix, label: fieldLabel(key), value })
  }
  return rows.sort((a, b) => order.indexOf(a.prefix) - order.indexOf(b.prefix)).map(({ label, value }) => ({ label, value }))
}
