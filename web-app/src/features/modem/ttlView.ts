// TTL status interpretation (PLAN2 R07/U03). Unreadable fields are unknown, never "disabled".

import { t } from '../../i18n'
import type { TtlStatus } from '../../types'

export type TtlState = 'active' | 'inactive' | 'unknown'

export function ttlState(s: TtlStatus | null | undefined): TtlState {
  if (!s) return 'unknown'
  if (s.active === true || s.ipv6_active === true) return 'active'
  if (s.active === false && s.ipv6_active === false) return 'inactive'
  return 'unknown'
}

/** Which IP families are clamped, for the status chip. null when not active. */
export function ttlFamilies(s: TtlStatus | null | undefined): string | null {
  if (!s || ttlState(s) !== 'active') return null
  if (s.active && s.ipv6_active) return 'IPv4 + IPv6'
  return s.active ? t('IPv4 only') : t('IPv6 only')
}

export const DEFAULT_TTL = '65'

/** The value the input shows: the user's draft, else the observed TTL, else the usual suggestion. */
export function ttlInputValue(draft: string | null, s: TtlStatus | null | undefined): string {
  if (draft !== null) return draft
  const v = s?.ttl_value
  return v !== undefined && v > 0 ? String(v) : DEFAULT_TTL
}

export type TtlParse = { ok: true; ttl: number } | { ok: false; error: string }

export function parseTtl(text: string): TtlParse {
  const trimmed = text.trim()
  if (!/^\d{1,3}$/.test(trimmed)) return { ok: false, error: t('Enter a whole number from 1 to 255.') }
  const ttl = Number(trimmed)
  if (ttl < 1 || ttl > 255) return { ok: false, error: t('Enter a whole number from 1 to 255.') }
  return { ok: true, ttl }
}
