// Settings backup: section names, file naming and reading a chosen file (tested in tools/test-backup.cjs).

import { t } from '../../i18n'

export const BACKUP_FORMAT = 'u60-pro-webui-backup'

export const SECTION_LABELS: Record<string, string> = {
  proxy: t('Proxy subscriptions and settings'),
  sms_forward: t('SMS forwarding'),
  client_names: t('Device names'),
  sleep: t('Device sleep'),
  reboot_schedule: t('Scheduled reboot'),
  data_limit: t('Monthly limit'),
  firewall: t('UPnP, DMZ and remote access'),
  dhcp_bindings: t('Fixed IP addresses'),
  port_rules: t('Port forwarding'),
  blocklist: t('Wi-Fi block list'),
  watchdog: t('Connection watchdog'),
}

export function backupFileName(created: string | undefined, now = new Date()): string {
  const m = created ? /^(\d{4})-(\d{2})-(\d{2})/.exec(created) : null
  const stamp = m ? `${m[1]}${m[2]}${m[3]}` : `${now.getFullYear()}${String(now.getMonth() + 1).padStart(2, '0')}${String(now.getDate()).padStart(2, '0')}`
  return `u60-pro-settings-${stamp}.json`
}

export type ParsedBackup =
  | { ok: true; doc: Record<string, unknown>; sections: string[]; created?: string; firmware?: string }
  | { ok: false; error: string }

/** Validate a chosen file's text before anything is sent. */
export function parseBackup(text: string): ParsedBackup {
  let doc: unknown
  try {
    doc = JSON.parse(text)
  } catch {
    return { ok: false, error: t('The file is not valid JSON.') }
  }
  if (!doc || typeof doc !== 'object' || (doc as Record<string, unknown>).format !== BACKUP_FORMAT) {
    return { ok: false, error: t('This is not a settings backup from this dashboard.') }
  }
  const d = doc as Record<string, unknown>
  const raw = d.sections && typeof d.sections === 'object' ? (d.sections as Record<string, unknown>) : {}
  const sections = Object.keys(SECTION_LABELS).filter((k) => raw[k] !== undefined && raw[k] !== null)
  if (sections.length === 0) return { ok: false, error: t('The backup contains no settings.') }
  return {
    ok: true,
    doc: d,
    sections,
    created: typeof d.created === 'string' ? d.created : undefined,
    firmware: typeof d.firmware === 'string' ? d.firmware : undefined,
  }
}
