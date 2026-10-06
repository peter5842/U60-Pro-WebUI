import { useState } from 'react'
import { api } from '../../data/api'
import { usePoll, useResource } from '../../data/poll'
import { formatBytes, formatSpeed } from '../../format'
import { t } from '../../i18n'
import type { Blocklist, Client, ClientTrafficReport } from '../../types'
import { Button, Field, Input } from '../../ui/controls'
import { confirm, toast, toastError } from '../../ui/feedback'
import { Card, Chip, InlineStatus, Skeleton } from '../../ui/primitives'
import { displayName, validateClientName } from './clientsView'

const mediumLabel = (c: Client) =>
  c.medium === 'wifi' ? (c.wifi_band ?? 'Wi-Fi') : c.medium === 'usb-c' ? 'USB-C' : c.medium === 'ethernet' ? t('Ethernet') : t('Other')

export default function DeviceControls({ clients, onChanged }: { clients: Client[]; onChanged: () => void }) {
  const blocklist = useResource<Blocklist>('network:blocklist', api.blocklist)
  const traffic = usePoll<ClientTrafficReport>('network:client-traffic', api.clientTraffic, 10_000)
  const [busy, setBusy] = useState<string | null>(null)
  const [editing, setEditing] = useState<{ mac: string; name: string; error?: string } | null>(null)

  async function rename() {
    if (!editing || busy) return
    const name = editing.name
    const error = validateClientName(name)
    if (error) {
      setEditing({ ...editing, error })
      return
    }
    setBusy(`name:${editing.mac}`)
    try {
      await api.clientNameSet(editing.mac, name)
      toast(t('Renamed to {name}', { name }))
      setEditing(null)
      onChanged()
      blocklist.refresh()
    } catch (e) {
      toastError(e, t('Failed to rename the device'))
    } finally {
      setBusy(null)
    }
  }

  async function disconnect(c: Client) {
    const label = displayName(c) ?? c.mac
    const ok = await confirm({
      title: t('Disconnect {name}?', { name: label }),
      body: t('The device is dropped from Wi-Fi now. It can rejoin straight away unless it is also blocked.'),
      kind: 'connection',
      confirmLabel: t('Disconnect'),
      details: [{ label: 'MAC', value: c.mac }],
    })
    if (!ok) return
    setBusy(`kick:${c.mac}`)
    try {
      await api.clientKick(c.mac)
      toast(t('{name} disconnected', { name: label }))
      onChanged()
    } catch (e) {
      toastError(e, t('Failed to disconnect the device'))
    } finally {
      setBusy(null)
    }
  }

  async function setBlocked(mac: string, label: string, blocked: boolean) {
    if (blocked) {
      const ok = await confirm({
        title: t('Block {name} from Wi-Fi?', { name: label }),
        body: t('The device is disconnected and cannot join this router’s Wi-Fi (main or guest) until it is unblocked.'),
        kind: 'connection',
        confirmLabel: t('Block'),
        details: [{ label: 'MAC', value: mac }],
        consequence: t('If this is the device you are using over Wi-Fi, you lose access to this dashboard over Wi-Fi.'),
        recovery: t('Unblock it here from a device on USB-C or another Wi-Fi device.'),
      })
      if (!ok) return
    }
    setBusy(`block:${mac}`)
    try {
      blocklist.mutate(await api.blocklistSet(mac, blocked))
      toast(blocked ? t('{name} blocked', { name: label }) : t('{name} unblocked', { name: label }))
      onChanged()
    } catch (e) {
      toastError(e, blocked ? t('Failed to block the device') : t('Failed to unblock the device'))
      blocklist.refresh()
    } finally {
      setBusy(null)
    }
  }

  const usage = new Map((traffic.data?.clients ?? []).map((c) => [c.mac.toUpperCase(), c]))

  async function resetTraffic() {
    const ok = await confirm({ title: t('Reset the traffic counters?'), body: t('Every device starts again from zero.'), confirmLabel: t('Reset'), danger: true })
    if (!ok) return
    try {
      traffic.mutate(await api.clientTrafficReset())
    } catch (e) {
      toastError(e, t('Failed to reset the counters'))
    }
  }

  const bl = blocklist.data
  const blockedSet = new Set(bl?.blocked.map((b) => b.mac.toUpperCase()) ?? [])
  const canBlock = bl?.available === true && bl.blocked.length < bl.max

  return (
    <Card title={t('Device controls')}>
      <div className="space-y-3">
        <ul className="divide-y divide-line/6">
          {clients.map((c) => {
            const label = displayName(c)
            const isEditing = editing?.mac === c.mac
            const wifi = c.medium === 'wifi'
            return (
              <li key={c.mac} className="py-2.5">
                {isEditing ? (
                  <div className="flex flex-wrap items-end gap-2">
                    <div className="min-w-48 flex-1">
                      <Field label={t('Name for {mac}', { mac: c.mac })} error={editing.error}>
                        <Input
                          value={editing.name}
                          autoFocus
                          maxLength={64}
                          onChange={(e) => setEditing({ mac: c.mac, name: e.target.value })}
                          onKeyDown={(e) => {
                            if (e.key === 'Enter') void rename()
                            if (e.key === 'Escape') setEditing(null)
                          }}
                        />
                      </Field>
                    </div>
                    <Button variant="primary" size="sm" onClick={() => void rename()} loading={busy === `name:${c.mac}`}>
                      {t('Save')}
                    </Button>
                    <Button variant="ghost" size="sm" onClick={() => setEditing(null)} disabled={busy !== null}>
                      {t('Cancel')}
                    </Button>
                  </div>
                ) : (
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div className="min-w-0">
                      <p className="truncate text-body font-medium text-ink">{label ?? c.mac}</p>
                      <p className="flex flex-wrap items-center gap-1.5 text-meta text-ink3">
                        <Chip>{mediumLabel(c)}</Chip>
                        <span className="tnum font-mono">{c.mac}</span>
                        {c.ip && <span className="tnum font-mono">{c.ip}</span>}
                      </p>
                      {usage.get(c.mac.toUpperCase()) && (
                        <p className="tnum font-mono text-meta text-ink2">
                          ↓ {formatBytes(usage.get(c.mac.toUpperCase())!.down_bytes)} · ↑ {formatBytes(usage.get(c.mac.toUpperCase())!.up_bytes)}
                          {usage.get(c.mac.toUpperCase())!.down_rate > 0 && ` · ${formatSpeed(usage.get(c.mac.toUpperCase())!.down_rate)}`}
                        </p>
                      )}
                    </div>
                    <div className="flex flex-wrap gap-1.5">
                      <Button size="sm" variant="ghost" onClick={() => setEditing({ mac: c.mac, name: label ?? '' })} disabled={busy !== null}>
                        {t('Rename')}
                      </Button>
                      {wifi && (
                        <Button size="sm" variant="outline" onClick={() => void disconnect(c)} loading={busy === `kick:${c.mac}`} disabled={busy !== null}>
                          {t('Disconnect')}
                        </Button>
                      )}
                      {wifi && !blockedSet.has(c.mac.toUpperCase()) && (
                        <Button
                          size="sm"
                          variant="danger"
                          onClick={() => void setBlocked(c.mac, label ?? c.mac, true)}
                          loading={busy === `block:${c.mac}`}
                          disabled={busy !== null || !canBlock}
                        >
                          {t('Block')}
                        </Button>
                      )}
                    </div>
                  </div>
                )}
              </li>
            )
          })}
        </ul>

        {traffic.data?.since && (
          <div className="flex flex-wrap items-center justify-between gap-2 text-meta text-ink3">
            <span>{t('Internet traffic counted since {time}', { time: traffic.data.since })}</span>
            <Button size="sm" variant="ghost" onClick={() => void resetTraffic()}>
              {t('Reset counters')}
            </Button>
          </div>
        )}

        <div className="border-t border-line/8 pt-3">
          <p className="text-body font-semibold text-ink">
            {bl ? t('Blocked from Wi-Fi ({n})', { n: bl.blocked.length }) : t('Blocked from Wi-Fi')}
          </p>
          {blocklist.status === 'loading' && <Skeleton className="mt-2 h-8" />}
          {blocklist.status === 'error' && !bl && (
            <InlineStatus kind="error" className="mt-2" action={{ label: t('Retry'), onClick: blocklist.refresh, loading: blocklist.refreshing }}>
              {t('The block list could not be read.')}
            </InlineStatus>
          )}
          {bl && !bl.available && (
            <InlineStatus kind="info" live={false} className="mt-2">
              {t('The Wi-Fi MAC filter is in allow-list mode (set in the stock web UI), so devices cannot be blocked here.')}
            </InlineStatus>
          )}
          {bl && bl.blocked.length === 0 && bl.available && (
            <p className="mt-1 text-meta text-ink3">{t('No device is blocked. Blocking only affects Wi-Fi; USB-C devices always connect.')}</p>
          )}
          {bl && bl.blocked.length > 0 && (
            <ul className="mt-1 divide-y divide-line/6">
              {bl.blocked.map((b) => (
                <li key={b.mac} className="flex items-center justify-between gap-2 py-2">
                  <span className="min-w-0">
                    <span className="block truncate text-body text-ink">{b.name ?? b.mac}</span>
                    {b.name && <span className="tnum font-mono text-meta text-ink3">{b.mac}</span>}
                  </span>
                  <Button size="sm" variant="outline" onClick={() => void setBlocked(b.mac, b.name ?? b.mac, false)} loading={busy === `block:${b.mac}`} disabled={busy !== null}>
                    {t('Unblock')}
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </Card>
  )
}
