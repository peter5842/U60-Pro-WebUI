// Mappers for the router network services (agent/src/netsvc.rs).

import type { ClockStatus, DhcpBindings, FirewallServices, PortRule, PortRules, WatchdogSettings } from '../types'
import { boolLike, finiteNumber, intInRange, nonEmptyStr } from './validate'

const rows = (v: unknown): Record<string, unknown>[] =>
  Array.isArray(v) ? v.filter((r): r is Record<string, unknown> => !!r && typeof r === 'object') : []

export function mapWatchdog(d: Record<string, unknown>): WatchdogSettings {
  return {
    enabled: boolLike(d.enabled) ?? false,
    host: nonEmptyStr(d.host),
    interval_minutes: intInRange(d.interval_minutes, 1, 100000),
    failures: intInRange(d.failures, 1, 1000),
  }
}

export function mapFirewall(d: Record<string, unknown>): FirewallServices {
  return {
    upnp: boolLike(d.upnp) ?? false,
    dmz_enabled: boolLike(d.dmz_enabled) ?? false,
    dmz_ip: nonEmptyStr(d.dmz_ip),
    remote_web_access: boolLike(d.remote_web_access) ?? false,
    wan_ping: boolLike(d.wan_ping) ?? false,
  }
}

function mapPortRule(r: Record<string, unknown>): PortRule | null {
  const id = nonEmptyStr(r.id)
  const kind = r.kind === 'forward' || r.kind === 'mapping' ? r.kind : undefined
  const start = intInRange(r.external_start, 1, 65535)
  const end = intInRange(r.external_end, 1, 65535)
  const ip = nonEmptyStr(r.ip)
  if (!id || !kind || start === undefined || end === undefined || !ip) return null
  return {
    id,
    kind,
    ip,
    external_start: start,
    external_end: end,
    internal: intInRange(r.internal, 1, 65535),
    proto: r.proto === 'tcp' || r.proto === 'udp' ? r.proto : 'both',
    comment: nonEmptyStr(r.comment) ?? '',
  }
}

export function mapPortRules(d: Record<string, unknown>): PortRules {
  return {
    forward_enabled: boolLike(d.forward_enabled) ?? false,
    mapping_enabled: boolLike(d.mapping_enabled) ?? false,
    max_per_kind: intInRange(d.max_per_kind, 1, 1000) ?? 20,
    rules: rows(d.rules).flatMap((r) => mapPortRule(r) ?? []),
  }
}

export function mapDhcpBindings(d: Record<string, unknown>): DhcpBindings {
  return {
    enabled: boolLike(d.enabled) ?? false,
    max: intInRange(d.max, 1, 1000) ?? 10,
    lan_ip: nonEmptyStr(d.lan_ip),
    netmask: nonEmptyStr(d.netmask),
    bindings: rows(d.bindings).flatMap((b) => {
      const id = nonEmptyStr(b.id)
      const mac = nonEmptyStr(b.mac)
      const ip = nonEmptyStr(b.ip)
      return id && mac && ip ? [{ id, mac, ip, name: nonEmptyStr(b.name) }] : []
    }),
  }
}

export function mapClock(d: Record<string, unknown>): ClockStatus {
  return {
    local_time: nonEmptyStr(d.local_time),
    utc_offset_hours: finiteNumber(d.utc_offset_hours),
    mode: nonEmptyStr(d.mode),
    source: nonEmptyStr(d.source),
    sntp_synced: boolLike(d.sntp_synced) ?? false,
    servers: Array.isArray(d.servers) ? d.servers.filter((s): s is string => typeof s === 'string' && s !== '') : [],
  }
}
