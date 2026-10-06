// Port rules, DMZ and static DHCP: validation mirroring agent/src/netsvc.rs (tested in tools/test-ports.cjs).

import { t } from '../../i18n'
import type { PortRule } from '../../types'

const toInt = (ip: string): number | undefined => {
  const m = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(ip.trim())
  if (!m) return undefined
  const o = m.slice(1).map(Number)
  if (o.some((n) => n > 255)) return undefined
  return ((o[0] << 24) | (o[1] << 16) | (o[2] << 8) | o[3]) >>> 0
}

/** A LAN device address: in the router's subnet, not the router, network or broadcast address. */
export function validateLanHost(ip: string, lanIp?: string, netmask?: string): string | undefined {
  const n = toInt(ip)
  if (n === undefined) return t('Enter an IPv4 address such as 192.168.0.20')
  const lan = lanIp ? toInt(lanIp) : undefined
  const mask = netmask ? toInt(netmask) : undefined
  if (lan === undefined || mask === undefined) return undefined
  if (((n & mask) >>> 0) !== ((lan & mask) >>> 0)) return t('Not in the router’s network ({lan})', { lan: `${lanIp}/${netmask}` })
  if (n === lan) return t('That is the router’s own address')
  const host = (n & ~mask) >>> 0
  const first = n >>> 24
  const last = n & 255
  if (host === 0 || host === (~mask >>> 0) || first < 1 || first > 223 || last === 0 || last === 255) return t('This address cannot be used for a device')
  return undefined
}

export function parsePort(text: string, max = 65535): number | undefined {
  if (!/^\d{1,5}$/.test(text.trim())) return undefined
  const n = Number(text)
  return n >= 1 && n <= max ? n : undefined
}

export function validateComment(c: string): string | undefined {
  if (c.length < 1 || c.length > 32) return t('1–32 characters')
  if (!/^[0-9a-zA-Z!#()+\-./%=?@^_{|}~]+$/.test(c)) return t('Letters, digits and !#()+-./%=?@^_{|}~ only (no spaces)')
  return undefined
}

export function protoLabel(p: PortRule['proto']): string {
  return p === 'both' ? 'TCP+UDP' : p.toUpperCase()
}

/** "8000-8010 → 192.168.0.20" or "2222 → 192.168.0.20:22". */
export function ruleSummary(r: PortRule): string {
  if (r.kind === 'mapping') return `${r.external_start} → ${r.ip}:${r.internal ?? r.external_start}`
  const ports = r.external_start === r.external_end ? `${r.external_start}` : `${r.external_start}-${r.external_end}`
  return `${ports} → ${r.ip}`
}

/** Mapping ports the firmware keeps for itself. */
export const isReservedPort = (p: number) => p >= 32000 && p <= 32007
