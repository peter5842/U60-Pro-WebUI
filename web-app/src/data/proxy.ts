// Mappers for /api/proxy/* (agent/src/mihomo). Same boundary rules as the
// rest of the dashboard: malformed fields become undefined, never invented.

import type {
  ProxyGroup,
  ProxyGroups,
  ProxyMode,
  ProxyNode,
  ProxyPreset,
  ProxyStatus,
  ProxySubscription,
  ProxySubscriptions,
  ProxyTraffic,
  ProxyUsage,
} from '../types'
import { boolLike, isObj, nonEmptyStr, nonNegativeInt, str, strList } from './validate'

const MODES: ProxyMode[] = ['rule', 'global', 'direct']
const PRESETS: ProxyPreset[] = ['bypass_cn', 'gfw', 'proxy_all']

function oneOf<T extends string>(v: unknown, allowed: T[]): T | undefined {
  return typeof v === 'string' && (allowed as string[]).includes(v) ? (v as T) : undefined
}

function mapTraffic(v: unknown): ProxyTraffic | undefined {
  if (!isObj(v)) return undefined
  return {
    up_total: nonNegativeInt(v.up_total),
    down_total: nonNegativeInt(v.down_total),
    up_rate: nonNegativeInt(v.up_rate),
    down_rate: nonNegativeInt(v.down_rate),
    connections: nonNegativeInt(v.connections),
  }
}

export function mapProxyStatus(d: Record<string, unknown>): ProxyStatus {
  const route = isObj(d.route) ? strList(d.route.chain) : undefined
  const profile = isObj(d.profile) && nonEmptyStr(d.profile.id) && nonEmptyStr(d.profile.name)
    ? { id: d.profile.id as string, name: d.profile.name as string }
    : undefined
  return {
    installed: boolLike(d.installed) ?? false,
    version: nonEmptyStr(d.version),
    running: boolLike(d.running) ?? false,
    pid: nonNegativeInt(d.pid),
    uptime_secs: nonNegativeInt(d.uptime_secs),
    rss_bytes: nonNegativeInt(d.rss_bytes),
    enabled: boolLike(d.enabled) ?? false,
    mode: oneOf(d.mode, MODES),
    preset: oneOf(d.preset, PRESETS),
    tun: boolLike(d.tun) ?? false,
    tun_active: boolLike(d.tun_active) ?? false,
    mixed_port: nonNegativeInt(d.mixed_port),
    lan_ip: nonEmptyStr(d.lan_ip),
    proxy_address: nonEmptyStr(d.proxy_address),
    pac_url: nonEmptyStr(d.pac_url),
    subscriptions: nonNegativeInt(d.subscriptions) ?? 0,
    traffic: mapTraffic(d.traffic),
    route: route?.length ? route : undefined,
    profile,
    health: isObj(d.health)
      ? {
          ok: boolLike(d.health.ok),
          route_ok: boolLike(d.health.route_ok),
          checked_secs_ago: nonNegativeInt(d.health.checked_secs_ago),
          retested_secs_ago: nonNegativeInt(d.health.retested_secs_ago),
        }
      : undefined,
    restarts: nonNegativeInt(d.restarts) ?? 0,
    last_error: nonEmptyStr(d.last_error),
    notice: nonEmptyStr(d.notice),
  }
}

function mapUsage(v: unknown): ProxyUsage | undefined {
  if (!isObj(v)) return undefined
  const usage = {
    upload: nonNegativeInt(v.upload),
    download: nonNegativeInt(v.download),
    total: nonNegativeInt(v.total),
    expire: nonNegativeInt(v.expire),
  }
  return Object.values(usage).some((x) => x !== undefined) ? usage : undefined
}

function mapSubscription(v: unknown): ProxySubscription | null {
  if (!isObj(v)) return null
  const id = nonEmptyStr(v.id)
  const name = nonEmptyStr(v.name)
  if (!id || !name) return null
  return {
    id,
    name,
    url_masked: str(v.url_masked) ?? '',
    enabled: boolLike(v.enabled) ?? false,
    interval_hours: nonNegativeInt(v.interval_hours) ?? 0,
    use_config: boolLike(v.use_config) ?? false,
    full_config: boolLike(v.full_config),
    groups: nonNegativeInt(v.groups),
    node_count: nonNegativeInt(v.node_count),
    updated_at: nonEmptyStr(v.updated_at),
    usage: mapUsage(v.usage),
    error: nonEmptyStr(v.error),
  }
}

export function mapProxySubscriptions(d: Record<string, unknown>): ProxySubscriptions {
  const list = Array.isArray(d.subscriptions) ? d.subscriptions : []
  return {
    subscriptions: list.map(mapSubscription).filter((s): s is ProxySubscription => s !== null),
    running: boolLike(d.running) ?? false,
  }
}

function mapGroup(v: unknown): ProxyGroup | null {
  if (!isObj(v)) return null
  const name = nonEmptyStr(v.name)
  if (!name) return null
  return { name, type: nonEmptyStr(v.type), now: nonEmptyStr(v.now), all: strList(v.all) ?? [], hidden: boolLike(v.hidden) }
}

function mapNode(v: unknown): ProxyNode | null {
  if (!isObj(v)) return null
  const name = nonEmptyStr(v.name)
  if (!name) return null
  return {
    name,
    type: nonEmptyStr(v.type),
    udp: boolLike(v.udp),
    alive: boolLike(v.alive),
    delay: nonNegativeInt(v.delay),
    subscription_id: nonEmptyStr(v.subscription_id),
    subscription: nonEmptyStr(v.subscription),
  }
}

export function mapProxyGroups(d: Record<string, unknown>): ProxyGroups {
  return {
    running: boolLike(d.running) ?? false,
    groups: (Array.isArray(d.groups) ? d.groups : []).map(mapGroup).filter((g): g is ProxyGroup => g !== null),
    nodes: (Array.isArray(d.nodes) ? d.nodes : []).map(mapNode).filter((n): n is ProxyNode => n !== null),
  }
}

/** name → ms (0 = timed out). Non-numeric entries are dropped. */
export function mapProxyDelays(d: Record<string, unknown>): Record<string, number> {
  const out: Record<string, number> = {}
  if (isObj(d.delays)) {
    for (const [name, ms] of Object.entries(d.delays)) {
      const n = nonNegativeInt(ms)
      if (n !== undefined) out[name] = n
    }
  }
  return out
}
