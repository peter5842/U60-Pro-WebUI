import { useState } from 'react'
import { api } from '../../data/api'
import { useResource } from '../../data/poll'
import { t } from '../../i18n'
import type { Client, DhcpBindings } from '../../types'
import { Button, Field, Input, Select, Toggle } from '../../ui/controls'
import { confirm, toast, toastError } from '../../ui/feedback'
import { Card, InlineStatus, Skeleton } from '../../ui/primitives'
import { displayName } from './clientsView'
import { validateLanHost } from './portsView'

const MAC_RE = /^([0-9A-Fa-f]{2}[:-]){5}[0-9A-Fa-f]{2}$/

export default function StaticDhcpCard() {
  const res = useResource<DhcpBindings>('router:dhcp-bindings', api.dhcpBindings)
  const clients = useResource<Client[]>('network:clients', api.clients)
  const [busy, setBusy] = useState<string | null>(null)
  const [draft, setDraft] = useState<{ mac: string; ip: string } | null>(null)
  const [errors, setErrors] = useState<{ mac?: string; ip?: string }>({})
  const [pendingReboot, setPendingReboot] = useState(false)
  const d = res.data

  async function run(key: string, fn: () => Promise<DhcpBindings>, done: string, fail: string) {
    setBusy(key)
    try {
      res.mutate(await fn())
      toast(done)
      return true
    } catch (e) {
      toastError(e, fail)
      return false
    } finally {
      setBusy(null)
    }
  }

  async function add() {
    if (!draft || !d) return
    const next = {
      mac: MAC_RE.test(draft.mac.trim()) ? undefined : t('Enter a MAC address such as AA:BB:CC:DD:EE:FF'),
      ip: validateLanHost(draft.ip, d.lan_ip, d.netmask),
    }
    setErrors(next)
    if (next.mac || next.ip) return
    if (await run('add', () => api.dhcpBindingAdd(draft.mac.trim(), draft.ip.trim()), t('Fixed address added'), t('Failed to add the fixed address'))) {
      setDraft(null)
      setPendingReboot(true)
    }
  }

  async function remove(id: string, label: string) {
    const ok = await confirm({ title: t('Remove the fixed address for {name}?', { name: label }), confirmLabel: t('Remove'), danger: true })
    if (ok && (await run(`del:${id}`, () => api.dhcpBindingDelete(id), t('Fixed address removed'), t('Failed to remove the fixed address')))) setPendingReboot(true)
  }

  if (res.status === 'loading') return <Skeleton className="h-32" />
  if (!d) {
    return (
      <InlineStatus kind="error" action={{ label: t('Retry'), onClick: res.refresh, loading: res.refreshing }}>
        {t('Fixed addresses could not be read.')}
      </InlineStatus>
    )
  }
  const names = new Map((clients.data ?? []).map((c) => [c.mac.toUpperCase(), displayName(c)]))
  const unbound = (clients.data ?? []).filter((c) => !d.bindings.some((b) => b.mac.toUpperCase() === c.mac.toUpperCase()))

  return (
    <Card title={t('Fixed IP addresses (static DHCP)')}>
      <div className="space-y-3">
        <div className="flex items-start justify-between gap-4">
          <p className="text-meta text-ink2">{t('A device always gets the same address from the router. Useful for port forwarding and DMZ.')}</p>
          <Toggle
            checked={d.enabled}
            disabled={busy !== null}
            label={t('Fixed IP addresses (static DHCP)')}
            onChange={(v) =>
              void run('switch', () => api.dhcpBindingsSwitch(v), v ? t('Turned on') : t('Turned off'), t('Failed to save the setting')).then((ok) => ok && setPendingReboot(true))
            }
          />
        </div>
        {pendingReboot && (
          <InlineStatus kind="info" live={false}>
            {t('The router applies fixed addresses after a restart (System → Settings → Reboot).')}
          </InlineStatus>
        )}
        {d.bindings.length > 0 && (
          <ul className="divide-y divide-line/6">
            {d.bindings.map((b) => {
              const label = names.get(b.mac.toUpperCase()) ?? b.name ?? b.mac
              return (
                <li key={b.id} className="flex items-center justify-between gap-2 py-2">
                  <div className="min-w-0">
                    <p className="truncate text-body font-medium text-ink">{label}</p>
                    <p className="tnum font-mono text-meta text-ink2">
                      {b.ip} · {b.mac}
                    </p>
                  </div>
                  <Button size="sm" variant="ghost" onClick={() => void remove(b.id, label)} loading={busy === `del:${b.id}`} disabled={busy !== null}>
                    {t('Remove')}
                  </Button>
                </li>
              )
            })}
          </ul>
        )}
        {draft ? (
          <div className="space-y-2.5 rounded-ctl border border-line/8 p-3">
            {unbound.length > 0 && (
              <Field label={t('Pick a connected device')}>
                <Select
                  value=""
                  onChange={(e) => {
                    const c = unbound.find((x) => x.mac === e.target.value)
                    if (c) setDraft({ mac: c.mac, ip: c.ip ?? draft.ip })
                  }}
                >
                  <option value="">—</option>
                  {unbound.map((c) => (
                    <option key={c.mac} value={c.mac}>
                      {displayName(c) ?? c.mac} {c.ip ? `(${c.ip})` : ''}
                    </option>
                  ))}
                </Select>
              </Field>
            )}
            <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
              <Field label="MAC" error={errors.mac}>
                <Input value={draft.mac} placeholder="AA:BB:CC:DD:EE:FF" onChange={(e) => setDraft({ ...draft, mac: e.target.value })} />
              </Field>
              <Field label={t('IP address')} error={errors.ip}>
                <Input value={draft.ip} placeholder="192.168.0.20" onChange={(e) => setDraft({ ...draft, ip: e.target.value })} />
              </Field>
            </div>
            <div className="flex gap-2">
              <Button variant="primary" onClick={() => void add()} loading={busy === 'add'} disabled={busy !== null}>
                {t('Add')}
              </Button>
              <Button variant="ghost" onClick={() => setDraft(null)} disabled={busy !== null}>
                {t('Cancel')}
              </Button>
            </div>
          </div>
        ) : (
          <Button variant="outline" onClick={() => setDraft({ mac: '', ip: '' })} disabled={busy !== null || d.bindings.length >= d.max}>
            {t('Add fixed address')}
          </Button>
        )}
      </div>
    </Card>
  )
}
