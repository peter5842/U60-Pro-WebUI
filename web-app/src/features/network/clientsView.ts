// Pure presentation helpers for the Clients tab (PLAN2 R07, U07).

import { t } from '../../i18n'
import type { Client } from '../../types'

export interface GroupedClients {
  wifi: Client[]
  usb: Client[]
  ethernet: Client[]
  other: Client[]
}

/** Every client lands in exactly one group, so the group counts always add up to the total. */
export function groupClients(clients: Client[]): GroupedClients {
  const out: GroupedClients = { wifi: [], usb: [], ethernet: [], other: [] }
  for (const c of clients) {
    if (c.medium === 'wifi') out.wifi.push(c)
    else if (c.medium === 'usb-c') out.usb.push(c)
    else if (c.medium === 'ethernet') out.ethernet.push(c)
    else out.other.push(c)
  }
  return out
}

/** Link speed as a number, or undefined when the device did not report one. */
export function formatLinkMbps(value?: number): string | undefined {
  if (value == null || !Number.isFinite(value) || value <= 0) return undefined
  return `${Math.round(value)} Mbps`
}

export function formatBitrate(mbps?: number): string | undefined {
  if (mbps == null || !Number.isFinite(mbps) || mbps <= 0) return undefined
  return mbps >= 1000 ? `${mbps / 1000} Gbit/s` : `${mbps} Mbit/s`
}

export function formatWifiLink(client: Client): string | undefined {
  const parts: string[] = []
  if (client.tx_bitrate_mbps != null) parts.push(`TX ${client.tx_bitrate_mbps.toFixed(0)}`)
  if (client.rx_bitrate_mbps != null) parts.push(`RX ${client.rx_bitrate_mbps.toFixed(0)}`)
  return parts.length > 0 ? `${parts.join(' / ')} Mbps` : undefined
}

export interface ClientField {
  label: string
  value: string | undefined
  mono?: boolean
}

/**
 * The fields of the stacked mobile row. The desktop tables show the same set, so nothing the
 * table has is dropped on a phone.
 */
export function clientFields(group: 'wifi' | 'usb' | 'ethernet' | 'other', c: Client): ClientField[] {
  const fields: ClientField[] = [{ label: 'IP', value: c.ip, mono: true }]
  if (group === 'wifi') {
    fields.push(
      { label: t('Signal'), value: c.signal_dbm != null ? `${c.signal_dbm} dBm` : undefined, mono: true },
      { label: t('Link'), value: formatWifiLink(c), mono: true },
    )
  } else if (group === 'usb') {
    fields.push({ label: t('Interface'), value: c.interface, mono: true })
  } else if (group === 'ethernet') {
    fields.push({ label: t('Speed'), value: formatLinkMbps(c.wired_link_mbps), mono: true })
  }
  fields.push({ label: 'MAC', value: c.mac, mono: true })
  return fields
}

/** The name to show: one set by the user first, then the DHCP hostname. */
export function displayName(c: Pick<Client, 'name' | 'hostname'>): string | undefined {
  return c.name || c.hostname || undefined
}

/** Mirrors the agent's rule (agent/src/clients.rs `valid_name`). Returns an error message or undefined. */
export function validateClientName(name: string): string | undefined {
  if (name.length === 0) return t('Enter a name')
  if ([...name].length > 32) return t('At most 32 characters')
  if (name.trim() !== name) return t('No spaces at the start or end')
  if (/[\p{Cc}'";$`\\|<>&]/u.test(name)) return t('Quotes and shell symbols ( \' " ; $ ` \\ | < > & ) are not allowed')
  return undefined
}
