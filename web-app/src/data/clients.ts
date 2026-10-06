// Mapper for /api/network/blocklist (agent/src/clients.rs).

import type { Blocklist } from '../types'
import { boolLike, intInRange, nonEmptyStr } from './validate'

export function mapBlocklist(d: Record<string, unknown>): Blocklist {
  const rows = Array.isArray(d.blocked) ? d.blocked : []
  return {
    blocked: rows.flatMap((r) => {
      if (!r || typeof r !== 'object') return []
      const o = r as Record<string, unknown>
      const mac = nonEmptyStr(o.mac)
      return mac ? [{ mac, name: nonEmptyStr(o.name) }] : []
    }),
    max: intInRange(d.max, 1, 1024) ?? 32,
    available: boolLike(d.available) ?? false,
  }
}
