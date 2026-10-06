// Mappers for carrier selection (agent/src/netselect.rs), SMS forwarding (sms_forward.rs)
// and per-device traffic (client_traffic.rs).

import type { CarrierNetwork, CarrierSelection, ClientTraffic, ClientTrafficReport, SmsForward, SmsForwardChannel } from '../types'
import { boolLike, nonEmptyStr, nonNegativeInt } from './validate'

const rows = (v: unknown): Record<string, unknown>[] =>
  Array.isArray(v) ? v.filter((r): r is Record<string, unknown> => !!r && typeof r === 'object') : []

const oneOf = <T extends string>(v: unknown, all: readonly T[], fallback: T): T => (all.includes(v as T) ? (v as T) : fallback)

export function mapCarrierSelection(d: Record<string, unknown>): CarrierSelection {
  const cur = (d.current && typeof d.current === 'object' ? d.current : {}) as Record<string, unknown>
  return {
    select_mode: d.select_mode === 'manual' ? 'manual' : 'auto',
    network_mode: nonEmptyStr(d.network_mode),
    current: { name: nonEmptyStr(cur.name), mcc: nonEmptyStr(String(cur.mcc ?? '')), mnc: nonEmptyStr(String(cur.mnc ?? '')) },
    scan: oneOf(d.scan, ['idle', 'scanning', 'done', 'failed'] as const, 'idle'),
    networks: rows(d.networks).flatMap((n): CarrierNetwork[] => {
      const mccmnc = nonEmptyStr(n.mccmnc)
      const rat = nonEmptyStr(n.rat)
      if (!mccmnc || !rat) return []
      return [
        {
          state: oneOf(n.state, ['available', 'current', 'forbidden', 'unknown'] as const, 'unknown'),
          name: nonEmptyStr(n.name) ?? mccmnc,
          mccmnc,
          rat,
          rat_label: nonEmptyStr(n.rat_label) ?? '?',
        },
      ]
    }),
    register: oneOf(d.register, ['idle', 'registering', 'success', 'failed'] as const, 'idle'),
  }
}

export const SMS_CHANNELS: readonly SmsForwardChannel[] = ['bark', 'serverchan', 'wecom', 'telegram', 'webhook']

export function mapSmsForward(d: Record<string, unknown>): SmsForward {
  return {
    enabled: boolLike(d.enabled) ?? false,
    channel: oneOf(d.channel, SMS_CHANNELS, 'bark'),
    configured: boolLike(d.configured) ?? false,
    target_hint: nonEmptyStr(d.target_hint),
    chat_id: nonEmptyStr(d.chat_id),
    via_proxy: boolLike(d.via_proxy) ?? false,
    forwarded: nonNegativeInt(d.forwarded) ?? 0,
    last_sent: nonEmptyStr(d.last_sent),
    last_error: nonEmptyStr(d.last_error),
  }
}

export function mapClientTraffic(d: Record<string, unknown>): ClientTrafficReport {
  return {
    since: nonEmptyStr(d.since),
    clients: rows(d.clients).flatMap((c): ClientTraffic[] => {
      const mac = nonEmptyStr(c.mac)
      if (!mac) return []
      return [
        {
          mac,
          ip: nonEmptyStr(c.ip),
          up_bytes: nonNegativeInt(c.up_bytes) ?? 0,
          down_bytes: nonNegativeInt(c.down_bytes) ?? 0,
          up_rate: nonNegativeInt(c.up_rate) ?? 0,
          down_rate: nonNegativeInt(c.down_rate) ?? 0,
        },
      ]
    }),
  }
}
