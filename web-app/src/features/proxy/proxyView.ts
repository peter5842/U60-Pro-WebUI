// Pure view helpers for the Proxy group (tested in tools/test-proxy-view.cjs).

import type { ChipTone } from '../../ui/primitives'
import type { ProxyMode, ProxyNode, ProxyPreset, ProxyStatus, ProxyUsage } from '../../types'

export const MODE_OPTIONS: { value: ProxyMode; label: string }[] = [
  { value: 'rule', label: 'Rule' },
  { value: 'global', label: 'Global' },
  { value: 'direct', label: 'Direct' },
]

export const MODE_HELP: Record<ProxyMode, string> = {
  rule: 'Traffic follows the routing preset (Settings → Routing).',
  global: 'All proxied traffic goes through the selected node.',
  direct: 'mihomo stays up but nothing is proxied.',
}

export const PRESET_OPTIONS: { value: ProxyPreset; label: string; help: string }[] = [
  {
    value: 'bypass_cn',
    label: 'Bypass mainland China',
    help: 'Mainland China sites and LAN addresses go direct; everything else uses the proxy.',
  },
  {
    value: 'gfw',
    label: 'GFW list only',
    help: 'Only sites on the GFW list use the proxy; everything else goes direct.',
  },
  {
    value: 'proxy_all',
    label: 'Proxy everything',
    help: 'Everything except LAN addresses uses the proxy.',
  },
]

export const INTERVAL_OPTIONS: { value: number; label: string }[] = [
  { value: 0, label: 'Manual only' },
  { value: 6, label: 'Every 6 hours' },
  { value: 12, label: 'Every 12 hours' },
  { value: 24, label: 'Daily' },
  { value: 72, label: 'Every 3 days' },
  { value: 168, label: 'Weekly' },
]

export function intervalLabel(hours: number): string {
  return INTERVAL_OPTIONS.find((o) => o.value === hours)?.label ?? `Every ${hours} h`
}

export function serviceState(s: ProxyStatus | null | undefined): { label: string; tone: ChipTone } {
  if (!s) return { label: 'Unknown', tone: 'default' }
  if (!s.installed) return { label: 'Not installed', tone: 'danger' }
  if (s.running) return { label: 'Running', tone: 'ok' }
  if (s.enabled) return { label: s.last_error ? 'Failing' : 'Starting', tone: s.last_error ? 'danger' : 'warn' }
  return { label: 'Stopped', tone: 'default' }
}

/** The node traffic actually leaves through, e.g. "AUTO → HK 01". */
export function currentRoute(s: ProxyStatus | null | undefined): string | undefined {
  if (!s?.group_choice) return undefined
  return s.group_choice === 'AUTO' && s.auto_choice ? `AUTO → ${s.auto_choice}` : s.group_choice
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
  return delay === 0 ? 'timeout' : `${delay} ms`
}

export type NodeSort = 'name' | 'delay'

/** Filter by a case-insensitive substring, then sort. Untested and timed-out nodes sort last by delay. */
export function visibleNodes(nodes: ProxyNode[], query: string, sort: NodeSort): ProxyNode[] {
  const q = query.trim().toLowerCase()
  const filtered = q
    ? nodes.filter((n) => n.name.toLowerCase().includes(q) || n.subscription.toLowerCase().includes(q))
    : nodes.slice()
  const rank = (n: ProxyNode) => (n.delay ? n.delay : Number.POSITIVE_INFINITY)
  return filtered.sort((a, b) =>
    sort === 'delay' ? rank(a) - rank(b) || a.name.localeCompare(b.name) : a.name.localeCompare(b.name),
  )
}

/** Relative time for an RFC 3339 timestamp, e.g. "5 min ago". */
export function ago(iso: string | undefined, now = Date.now()): string | undefined {
  if (!iso) return undefined
  const t = Date.parse(iso)
  if (!Number.isFinite(t)) return undefined
  const secs = Math.max(0, Math.round((now - t) / 1000))
  if (secs < 60) return 'just now'
  if (secs < 3600) return `${Math.floor(secs / 60)} min ago`
  if (secs < DAY) return `${Math.floor(secs / 3600)} h ago`
  return `${Math.floor(secs / DAY)} d ago`
}

/** Local validation mirroring the agent; returns an error message or undefined. */
export function validateSubscription(name: string, url: string): { name?: string; url?: string } {
  const errors: { name?: string; url?: string } = {}
  const n = name.trim()
  if (!n) errors.name = 'Enter a name'
  else if ([...n].length > 32) errors.name = 'At most 32 characters'
  const u = url.trim()
  if (!u) errors.url = 'Paste the subscription link'
  else if (!/^https?:\/\/[^\s/?#@]+/i.test(u)) errors.url = 'Must be an http:// or https:// link'
  else if (/\s/.test(u)) errors.url = 'The link must not contain spaces'
  return errors
}

export function validatePort(text: string): { ok: true; port: number } | { ok: false; error: string } {
  if (!/^\d+$/.test(text.trim())) return { ok: false, error: 'Enter a port number' }
  const port = Number(text)
  if (port < 1024 || port > 65535) return { ok: false, error: '1024 to 65535' }
  if ([2222, 8080, 9090, 9097].includes(port)) return { ok: false, error: 'Reserved by the router' }
  return { ok: true, port }
}
