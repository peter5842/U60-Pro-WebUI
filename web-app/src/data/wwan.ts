// Mappers for /api/modem/data and /api/data-usage/limit (agent/src/wwan.rs).

import type { DataLimit, MobileDataState } from '../types'
import { boolLike, intInRange, nonEmptyStr, nonNegativeInt } from './validate'

export function mapMobileData(d: Record<string, unknown>): MobileDataState {
  return {
    connected: boolLike(d.connected) ?? false,
    connect_status: nonEmptyStr(d.connect_status),
    auto_connect: boolLike(d.auto_connect),
    roaming_allowed: boolLike(d.roaming_allowed),
    ipv4: nonEmptyStr(d.ipv4),
    ipv6: nonEmptyStr(d.ipv6),
  }
}

export function mapDataLimit(d: Record<string, unknown>): DataLimit {
  return {
    enabled: boolLike(d.enabled) ?? false,
    kind: d.kind === 'time' ? 'time' : 'data',
    limit_bytes: nonNegativeInt(d.limit_bytes),
    alert_percent: intInRange(d.alert_percent, 1, 100),
  }
}
