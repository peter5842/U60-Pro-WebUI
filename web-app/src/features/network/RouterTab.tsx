import { useId, useState } from 'react'
import { api } from '../../data/api'
import { confirmLan } from '../../data/client'
import { useResource, type PollResult } from '../../data/poll'
import { t } from '../../i18n'
import type { DnsConfig, LanConfig } from '../../types'
import { Button, Field, Input, Toggle } from '../../ui/controls'
import { confirm, toast, toastError } from '../../ui/feedback'
import { Card, InlineStatus, Loading, Skeleton } from '../../ui/primitives'

/**
 * Baseline state of a settings read (R07). While there is no trustworthy baseline the form shows
 * empty disabled fields (never plausible defaults), the failure and a Retry; a stale baseline stays
 * editable but is labelled.
 */
function BaselineStatus({ what, resource }: { what: 'dns' | 'lan'; resource: Pick<PollResult<unknown>, 'status' | 'error' | 'refresh' | 'refreshing'> }) {
  const error = resource.error ?? ''
  if (resource.status === 'error') {
    return (
      <InlineStatus
        kind="error"
        className="mb-3"
        action={{ label: t('Retry'), onClick: resource.refresh, loading: resource.refreshing }}
      >
        {what === 'dns'
          ? t('Could not read the DNS settings: {error}. Editing is disabled until the current settings load.', { error })
          : t('Could not read the LAN settings: {error}. Editing is disabled until the current settings load.', { error })}
      </InlineStatus>
    )
  }
  if (resource.status === 'stale') {
    return (
      <InlineStatus
        kind="stale"
        className="mb-3"
        action={{ label: t('Retry'), onClick: resource.refresh, loading: resource.refreshing }}
      >
        {what === 'dns'
          ? t('Showing the last DNS settings that loaded. The latest refresh failed: {error}', { error })
          : t('Showing the last LAN settings that loaded. The latest refresh failed: {error}', { error })}
      </InlineStatus>
    )
  }
  return null
}

const DNS_PRESETS: { label: string; v: DnsConfig }[] = [
  {
    label: 'Cloudflare',
    v: { primary: '1.1.1.1', secondary: '1.0.0.1', ipv6_primary: '2606:4700:4700::1111', ipv6_secondary: '2606:4700:4700::1001' },
  },
  {
    label: 'Google',
    v: { primary: '8.8.8.8', secondary: '8.8.4.4', ipv6_primary: '2001:4860:4860::8888', ipv6_secondary: '2001:4860:4860::8844' },
  },
  {
    label: 'Quad9',
    v: { primary: '9.9.9.9', secondary: '149.112.112.112', ipv6_primary: '2620:fe::fe', ipv6_secondary: '2620:fe::9' },
  },
]

function DnsSection() {
  const resource = useResource('router:dns', api.dnsGet)
  // `draft` is null while the form is pristine (it then mirrors the device); an edit copies the baseline.
  const [draft, setDraft] = useState<DnsConfig | null>(null)
  const [busy, setBusy] = useState(false)
  const [saveNote, setSaveNote] = useState('')
  const known = resource.data != null
  const dns: DnsConfig = draft ?? resource.data ?? { primary: '', secondary: '' }
  const edit = (patch: Partial<DnsConfig>) => setDraft({ ...dns, ...patch })

  async function save() {
    if (!known || busy) return
    const frozen = { ...dns }
    setBusy(true)
    setSaveNote('')
    try {
      await api.dnsSet({
        dns_mode: 'manual',
        prefer_dns_manual: frozen.primary,
        standby_dns_manual: frozen.secondary,
        ...(frozen.ipv6_primary ? { ipv6_wan_prefer_dns_manual: frozen.ipv6_primary } : {}),
        ...(frozen.ipv6_secondary ? { ipv6_wan_standby_dns_manual: frozen.ipv6_secondary } : {}),
      })
      toast(t('DNS settings saved'))
      try {
        resource.mutate(await api.dnsGet())
        setDraft(null)
      } catch {
        // Accepted, but the read-back failed: keep showing what was submitted and say it is unverified.
        setSaveNote(t('DNS settings were accepted, but reading them back failed. The values shown are what you submitted and are unverified.'))
      }
    } catch (e) {
      toastError(e, t('Failed to save DNS'))
    } finally {
      setBusy(false)
    }
  }

  if (!resource.data && resource.status === 'loading') {
    return (
      <Loading label={t('Loading DNS settings')}>
        <Skeleton className="h-40" />
      </Loading>
    )
  }

  return (
    <Card title={t('DNS servers')}>
      <BaselineStatus what="dns" resource={resource} />
      {saveNote && (
        <InlineStatus kind="warn" className="mb-3">
          {saveNote}
        </InlineStatus>
      )}
      <div className="grid grid-cols-1 gap-2.5 lg:grid-cols-2">
        <Field label={t('Primary DNS (IPv4)')}>
          <Input disabled={!known} value={dns.primary} onChange={(e) => edit({ primary: e.target.value })} placeholder="1.1.1.1" inputMode="numeric" />
        </Field>
        <Field label={t('Secondary DNS (IPv4)')}>
          <Input disabled={!known} value={dns.secondary} onChange={(e) => edit({ secondary: e.target.value })} placeholder="1.0.0.1" inputMode="numeric" />
        </Field>
        <Field label={t('Primary DNS (IPv6)')}>
          <Input disabled={!known} value={dns.ipv6_primary ?? ''} onChange={(e) => edit({ ipv6_primary: e.target.value })} placeholder="2606:4700:4700::1111" />
        </Field>
        <Field label={t('Secondary DNS (IPv6)')}>
          <Input disabled={!known} value={dns.ipv6_secondary ?? ''} onChange={(e) => edit({ ipv6_secondary: e.target.value })} placeholder="2001:4860:4860::8888" />
        </Field>
      </div>
      <div className="mt-3.5 flex flex-wrap items-center gap-2">
        <Button variant="primary" onClick={save} loading={busy} disabled={!known}>
          {t('Apply DNS')}
        </Button>
        <div className="flex flex-wrap gap-1.5" role="group" aria-label={t('DNS presets')}>
          {DNS_PRESETS.map((p) => (
            <Button key={p.label} variant="ghost" size="sm" disabled={!known} aria-label={t('Fill {name} DNS servers', { name: p.label })} onClick={() => setDraft(p.v)}>
              {p.label}
            </Button>
          ))}
        </div>
      </div>
    </Card>
  )
}

function LanSection() {
  const resource = useResource('router:lan', api.lanGet)
  const [draft, setDraft] = useState<LanConfig | null>(null)
  const [busy, setBusy] = useState(false)
  const [transition, setTransition] = useState('')
  const dhcpId = useId()
  const known = resource.data != null
  const lan: LanConfig = draft ?? resource.data ?? { ipaddr: '', netmask: '', dhcp_enabled: false, dhcp_start: '', dhcp_end: '', lease_seconds: 0 }
  const edit = (patch: Partial<LanConfig>) => setDraft({ ...lan, ...patch })

  async function save() {
    if (!known || busy) return
    const frozen = { ...lan }
    const before = resource.data
    const moves = before != null && (frozen.ipaddr !== before.ipaddr || frozen.netmask !== before.netmask)
    setBusy(true)
    const ok = await confirm({
      kind: 'connection',
      title: moves ? t('Move the router to {ip}?', { ip: frozen.ipaddr }) : t('Apply LAN settings?'),
      body: t('The router restarts its LAN and DHCP service to apply this.'),
      details: [
        { label: t('Router address'), value: before && before.ipaddr !== frozen.ipaddr ? `${before.ipaddr} → ${frozen.ipaddr}` : frozen.ipaddr },
        { label: t('Netmask'), value: frozen.netmask },
        { label: 'DHCP', value: frozen.dhcp_enabled ? `${frozen.dhcp_start} – ${frozen.dhcp_end}` : t('Off') },
      ],
      consequence: moves
        ? t('Every device on Wi-Fi and USB-C briefly loses its LAN connection, including this dashboard, which moves to the new address. Mobile data is not changed.')
        : t('Connected devices may briefly lose their LAN connection and renew their addresses. Mobile data is not changed.'),
      recovery: t('If the new address cannot be confirmed within about two minutes, the previous LAN settings return automatically.'),
    })
    if (!ok) {
      setBusy(false)
      return
    }
    try {
      const result = await api.lanSet({
        ipaddr: frozen.ipaddr,
        netmask: frozen.netmask,
        dhcp_enabled: frozen.dhcp_enabled,
        dhcp_start: frozen.dhcp_start,
        dhcp_end: frozen.dhcp_end,
        lease_seconds: frozen.lease_seconds,
      })
      if (result.changed) {
        if (result.reconnect_ip !== frozen.ipaddr || typeof result.confirmation_token !== 'string') {
          throw new Error(t('Invalid LAN transition response; previous settings will be restored automatically'))
        }
        setTransition(t('Reconnecting to {ip}. Rejoin Wi-Fi if needed. Previous settings return automatically if confirmation fails.', { ip: frozen.ipaddr }))
        const deadline = Date.now() + 90_000
        let confirmed = false
        while (Date.now() < deadline) {
          await new Promise(resolve => setTimeout(resolve, 2000))
          try {
            await confirmLan(frozen.ipaddr, result.confirmation_token)
            confirmed = true
            break
          } catch { /* The address may still be changing; retry within the recovery window. */ }
        }
        if (!confirmed) {
          throw new Error(t('Could not confirm the new address. Wait up to two minutes from Apply for the previous LAN settings to return, then reconnect.'))
        }
        setTransition(t('LAN settings confirmed. Opening the dashboard at its new address…'))
        if (window.location.hostname !== frozen.ipaddr) {
          const next = new URL(window.location.href)
          next.hostname = frozen.ipaddr
          // Session storage belongs to the old origin; sign in again at the new address.
          window.location.assign(next.toString())
        }
      }
      toast(t('LAN settings saved and confirmed'))
      try {
        resource.mutate(await api.lanGet())
        setDraft(null)
      } catch {
        setTransition(t('LAN settings were accepted, but reading them back failed. The values shown are what you submitted and are unverified.'))
      }
    } catch (e) {
      setTransition(e instanceof Error ? e.message : t('LAN change failed'))
      toastError(e, t('Failed to save LAN settings'))
    } finally {
      setBusy(false)
    }
  }

  if (!resource.data && resource.status === 'loading') {
    return (
      <Loading label={t('Loading LAN settings')}>
        <Skeleton className="h-56" />
      </Loading>
    )
  }

  return (
    <Card title={t('LAN / DHCP')}>
      <BaselineStatus what="lan" resource={resource} />
      <p className="mb-3 text-xs text-ink3" role="status">{transition || t('Changes must reconnect and confirm within two minutes; otherwise the previous settings are restored.')}</p>
      <div className="grid grid-cols-1 gap-2.5 lg:grid-cols-2">
        <Field label={t('LAN IP')}>
          <Input disabled={!known} value={lan.ipaddr} onChange={(e) => edit({ ipaddr: e.target.value })} inputMode="numeric" />
        </Field>
        <Field label={t('Netmask')}>
          <Input disabled={!known} value={lan.netmask} onChange={(e) => edit({ netmask: e.target.value })} inputMode="numeric" />
        </Field>
        <Field label={t('DHCP start')}>
          <Input disabled={!known || !lan.dhcp_enabled} value={lan.dhcp_start} onChange={(e) => edit({ dhcp_start: e.target.value })} inputMode="numeric" />
        </Field>
        <Field label={t('DHCP end')}>
          <Input disabled={!known || !lan.dhcp_enabled} value={lan.dhcp_end} onChange={(e) => edit({ dhcp_end: e.target.value })} inputMode="numeric" />
        </Field>
        <Field label={t('Lease time (hours)')} hint={t('The firmware stores this value in seconds.')}>
          <Input
            type="number"
            min={1}
            max={168}
            disabled={!known || !lan.dhcp_enabled}
            value={known ? lan.lease_seconds / 3600 : ''}
            onChange={(e) => edit({ lease_seconds: Math.round(Number(e.target.value) * 3600) })}
            inputMode="numeric"
          />
        </Field>
      </div>
      <div className="mt-3 flex items-center justify-between gap-3 rounded-ctl bg-surface2/60 px-3 py-2.5">
        <div className="min-w-0">
          <p id={dhcpId} className="text-body font-semibold text-ink">{t('DHCP server')}</p>
          <p className="text-caption text-ink3">{t('Assign addresses to LAN and Wi-Fi clients')}</p>
        </div>
        <Toggle checked={lan.dhcp_enabled} disabled={!known} onChange={(dhcp_enabled) => edit({ dhcp_enabled })} labelledBy={dhcpId} />
      </div>
      <div className="mt-3.5">
        <Button variant="primary" onClick={save} loading={busy} disabled={!known}>
          {t('Apply LAN')}
        </Button>
      </div>
    </Card>
  )
}

export default function RouterTab() {
  return (
    <div className="space-y-3">
      <LanSection />
      <DnsSection />
    </div>
  )
}
