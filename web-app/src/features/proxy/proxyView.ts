// Pure view helpers for the Proxy group (tested in tools/test-proxy-view.cjs).

import { t } from '../../i18n'
import type { ChipTone } from '../../ui/primitives'
import type { ProxyGroup, ProxyMode, ProxyNode, ProxyPreset, ProxyStatus, ProxyUsage } from '../../types'

export const MODE_OPTIONS: { value: ProxyMode; label: string }[] = [
  { value: 'rule', label: t('Rule') },
  { value: 'global', label: t('Global') },
  { value: 'direct', label: t('Direct') },
]

export const MODE_HELP: Record<ProxyMode, string> = {
  rule: t('Traffic follows the routing preset (Settings → Routing).'),
  global: t('All proxied traffic goes through the selected node.'),
  direct: t('mihomo stays up but nothing is proxied.'),
}

export const PRESET_OPTIONS: { value: ProxyPreset; label: string; help: string }[] = [
  {
    value: 'bypass_cn',
    label: t('Bypass mainland China'),
    help: t('Mainland China sites and LAN addresses go direct; everything else uses the proxy.'),
  },
  {
    value: 'gfw',
    label: t('GFW list only'),
    help: t('Only sites on the GFW list use the proxy; everything else goes direct.'),
  },
  {
    value: 'proxy_all',
    label: t('Proxy everything'),
    help: t('Everything except LAN addresses uses the proxy.'),
  },
]

export const INTERVAL_OPTIONS: { value: number; label: string }[] = [
  { value: 0, label: t('Manual only') },
  { value: 6, label: t('Every 6 hours') },
  { value: 12, label: t('Every 12 hours') },
  { value: 24, label: t('Daily') },
  { value: 72, label: t('Every 3 days') },
  { value: 168, label: t('Weekly') },
]

export function intervalLabel(hours: number): string {
  return INTERVAL_OPTIONS.find((o) => o.value === hours)?.label ?? t('Every {n} h', { n: hours })
}

export function serviceState(s: ProxyStatus | null | undefined): { label: string; tone: ChipTone } {
  if (!s) return { label: t('Unknown'), tone: 'default' }
  if (!s.installed) return { label: t('Not installed'), tone: 'danger' }
  if (s.running) return { label: t('Running'), tone: 'ok' }
  if (s.enabled) return { label: s.last_error ? t('Failing') : t('Starting'), tone: s.last_error ? 'danger' : 'warn' }
  return { label: t('Stopped'), tone: 'default' }
}

/** Where proxied traffic leaves, e.g. "节点选择 → 自动选择 → HK 01". */
export function currentRoute(s: ProxyStatus | null | undefined): string | undefined {
  return s?.route?.join(' → ')
}

const GROUP_TYPE_LABELS: Record<string, string> = {
  Selector: t('Manual'),
  URLTest: t('Fastest'),
  Fallback: t('Fallback'),
  LoadBalance: t('Load balance'),
  Relay: t('Relay'),
}

export function groupTypeLabel(type: string | undefined): string {
  return (type && GROUP_TYPE_LABELS[type]) ?? type ?? ''
}

/** Only select groups accept a manual choice. */
export function isSelectable(group: ProxyGroup): boolean {
  return group.type === 'Selector'
}

/** Used share of the plan (0-100+), when the provider reports a total. */
export function usagePct(u: ProxyUsage | undefined): number | undefined {
  if (!u?.total) return undefined
  const used = (u.upload ?? 0) + (u.download ?? 0)
  return Math.round((used / u.total) * 1000) / 10
}

export function usedBytes(u: ProxyUsage | undefined): number | undefined {
  if (!u || (u.upload === undefined && u.download === undefined)) return undefined
  return (u.upload ?? 0) + (u.download ?? 0)
}

const DAY = 86_400

/** Expiry as a date plus days left; undefined when the provider sets none. */
export function expiry(
  u: ProxyUsage | undefined,
  nowSecs = Date.now() / 1000,
): { date: string; daysLeft: number; tone: ChipTone } | undefined {
  if (!u?.expire) return undefined
  const daysLeft = Math.floor((u.expire - nowSecs) / DAY)
  const d = new Date(u.expire * 1000)
  const date = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
  const tone: ChipTone = daysLeft < 0 ? 'danger' : daysLeft <= 7 ? 'warn' : 'default'
  return { date, daysLeft, tone }
}

export function usageTone(pct: number | undefined): string {
  if (pct === undefined) return 'bg-accent'
  return pct >= 95 ? 'bg-danger' : pct >= 80 ? 'bg-warn' : 'bg-accent'
}

export function delayTone(delay: number | undefined): ChipTone {
  if (delay === undefined) return 'default'
  if (delay === 0) return 'danger'
  return delay < 200 ? 'ok' : delay < 500 ? 'warn' : 'danger'
}

export function delayLabel(delay: number | undefined): string {
  if (delay === undefined) return '—'
  return delay === 0 ? t('timeout') : `${delay} ms`
}

export type NodeSort = 'config' | 'delay'

/**
 * A group's members after filtering (case-insensitive substring of the name or
 * subscription) and sorting. `config` keeps the provider's order; `delay` puts
 * the fastest first and untested/timed-out members last.
 */
export function visibleMembers(
  members: string[],
  nodes: Map<string, ProxyNode>,
  query: string,
  sort: NodeSort,
): string[] {
  const q = query.trim().toLowerCase()
  const filtered = q
    ? members.filter((m) => m.toLowerCase().includes(q) || nodes.get(m)?.subscription?.toLowerCase().includes(q))
    : members.slice()
  if (sort === 'config') return filtered
  const rank = (m: string) => nodes.get(m)?.delay || Number.POSITIVE_INFINITY
  return filtered
    .map((m, i) => ({ m, i }))
    .sort((a, b) => rank(a.m) - rank(b.m) || a.i - b.i)
    .map((x) => x.m)
}

/** Relative time for an RFC 3339 timestamp, e.g. "5 min ago". */
export function ago(iso: string | undefined, now = Date.now()): string | undefined {
  if (!iso) return undefined
  const time = Date.parse(iso)
  if (!Number.isFinite(time)) return undefined
  const secs = Math.max(0, Math.round((now - time) / 1000))
  if (secs < 60) return t('just now')
  if (secs < 3600) return t('{n} min ago', { n: Math.floor(secs / 60) })
  if (secs < DAY) return t('{n} h ago', { n: Math.floor(secs / 3600) })
  return t('{n} d ago', { n: Math.floor(secs / DAY) })
}

/** Local validation mirroring the agent; returns an error message or undefined. */
export function validateSubscription(name: string, url: string): { name?: string; url?: string } {
  const errors: { name?: string; url?: string } = {}
  const n = name.trim()
  if (!n) errors.name = t('Enter a name')
  else if ([...n].length > 32) errors.name = t('At most 32 characters')
  const u = url.trim()
  if (!u) errors.url = t('Paste the subscription link')
  else if (!/^https?:\/\/[^\s/?#@]+/i.test(u)) errors.url = t('Must be an http:// or https:// link')
  else if (/\s/.test(u)) errors.url = t('The link must not contain spaces')
  return errors
}

export function validatePort(text: string): { ok: true; port: number } | { ok: false; error: string } {
  if (!/^\d+$/.test(text.trim())) return { ok: false, error: t('Enter a port number') }
  const port = Number(text)
  if (port < 1024 || port > 65535) return { ok: false, error: t('1024 to 65535') }
  if ([2222, 8080, 9090, 9097].includes(port)) return { ok: false, error: t('Reserved by the router') }
  return { ok: true, port }
}
