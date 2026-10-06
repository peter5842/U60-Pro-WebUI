import { useState, type ReactNode } from 'react'
import { api } from '../../data/api'
import { useResource } from '../../data/poll'
import { t } from '../../i18n'
import type { Client, FirewallServices, PortRule, PortRules } from '../../types'
import { Button, Field, Input, Segmented, Select, Toggle } from '../../ui/controls'
import { confirm, toast, toastError } from '../../ui/feedback'
import { Card, Chip, Empty, InlineStatus, Skeleton } from '../../ui/primitives'
import { isReservedPort, parsePort, protoLabel, ruleSummary, validateComment, validateLanHost } from './portsView'

type Lan = { ip?: string; mask?: string }

export default function PortsTab() {
  const clients = useResource<Client[]>('network:clients', api.clients)
  // Same cache key as the Router tab, so the LAN is read once.
  const lanRes = useResource('router:lan', api.lanGet)
  const lan: Lan = { ip: lanRes.data?.ipaddr, mask: lanRes.data?.netmask }
  return (
    <div className="space-y-3">
      <PortRulesCard clients={clients.data ?? []} lan={lan} />
      <FirewallCard clients={clients.data ?? []} lan={lan} />
    </div>
  )
}

function ClientIps({ id, clients }: { id: string; clients: Client[] }) {
  return (
    <datalist id={id}>
      {clients
        .filter((c) => c.ip)
        .map((c) => (
          <option key={c.mac} value={c.ip}>
            {c.name || c.hostname || c.mac}
          </option>
        ))}
    </datalist>
  )
}

type Draft = { kind: PortRule['kind']; ip: string; start: string; end: string; internal: string; proto: PortRule['proto']; comment: string }
const EMPTY: Draft = { kind: 'forward', ip: '', start: '', end: '', internal: '', proto: 'both', comment: '' }

function validateDraft(d: Draft, lan: Lan) {
  const max = d.kind === 'mapping' ? 65000 : 65535
  const e: Partial<Record<keyof Draft, string>> = {}
  e.ip = validateLanHost(d.ip, lan.ip, lan.mask)
  const start = parsePort(d.start, max)
  if (start === undefined) e.start = t('1 to {max}', { max })
  else if (d.kind === 'mapping' && isReservedPort(start)) e.start = t('32000–32007 are reserved')
  if (d.kind === 'forward' && d.end !== '') {
    const end = parsePort(d.end, max)
    if (end === undefined) e.end = t('1 to {max}', { max })
    else if (start !== undefined && end < start) e.end = t('Must not be below the first port')
  }
  if (d.kind === 'mapping') {
    const internal = parsePort(d.internal, max)
    if (internal === undefined) e.internal = t('1 to {max}', { max })
    else if (isReservedPort(internal)) e.internal = t('32000–32007 are reserved')
  }
  e.comment = validateComment(d.comment)
  return Object.fromEntries(Object.entries(e).filter(([, v]) => v)) as Partial<Record<keyof Draft, string>>
}

function PortRulesCard({ clients, lan }: { clients: Client[]; lan: Lan }) {
  const res = useResource<PortRules>('router:port-rules', api.portRules)
  const [busy, setBusy] = useState<string | null>(null)
  const [draft, setDraft] = useState<Draft | null>(null)
  const [showErrors, setShowErrors] = useState(false)
  const data = res.data

  async function run(key: string, fn: () => Promise<PortRules>, done: string, fail: string) {
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
    if (!draft) return
    const errors = validateDraft(draft, lan)
    if (Object.keys(errors).length > 0) {
      setShowErrors(true)
      return
    }
    const body: Record<string, unknown> = { kind: draft.kind, ip: draft.ip.trim(), proto: draft.proto, comment: draft.comment, external_start: Number(draft.start) }
    if (draft.kind === 'forward' && draft.end !== '') body.external_end = Number(draft.end)
    if (draft.kind === 'mapping') body.internal = Number(draft.internal)
    const ok = await run('add', () => api.portRuleAdd(body), t('Rule added'), t('Failed to add the rule'))
    if (ok) {
      setDraft(null)
      setShowErrors(false)
    }
  }

  async function remove(r: PortRule) {
    const ok = await confirm({ title: t('Delete the rule "{name}"?', { name: r.comment }), body: ruleSummary(r), confirmLabel: t('Delete'), danger: true })
    if (ok) await run(`del:${r.id}`, () => api.portRuleDelete(r.kind, r.id), t('Rule deleted'), t('Failed to delete the rule'))
  }

  if (res.status === 'loading') return <Skeleton className="h-40" />
  if (!data) {
    return (
      <InlineStatus kind="error" action={{ label: t('Retry'), onClick: res.refresh, loading: res.refreshing }}>
        {t('Port rules could not be read.')}
      </InlineStatus>
    )
  }
  const errors = draft && showErrors ? validateDraft(draft, lan) : {}
  const set = (p: Partial<Draft>) => draft && setDraft({ ...draft, ...p })

  return (
    <Card title={t('Port forwarding')}>
      <div className="space-y-3">
        <p className="text-meta text-ink2">
          {t('Opens ports on the mobile connection to a device on the LAN. This only works when the carrier gives the router a public IPv4 address; most mobile plans share one address (CGNAT) and incoming connections never arrive.')}
        </p>
        <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
          {(
            [
              ['forward_enabled', t('Port forwarding on')],
              ['mapping_enabled', t('Port mapping on')],
            ] as const
          ).map(([key, label]) => (
            <div key={key} className="flex items-center justify-between gap-3 rounded-ctl bg-surface2/70 px-3 py-2">
              <span className="text-body text-ink">{label}</span>
              <Toggle
                checked={data[key]}
                disabled={busy !== null}
                label={label}
                onChange={(v) => void run(key, () => api.portRulesSwitch({ [key]: v }), v ? t('Turned on') : t('Turned off'), t('Failed to save the setting'))}
              />
            </div>
          ))}
        </div>

        {data.rules.length === 0 ? (
          <Empty title={t('No port rules')} body={t('Forward a port range to the same ports on a device, or map one external port to a different port.')} />
        ) : (
          <ul className="divide-y divide-line/6">
            {data.rules.map((r) => {
              const on = r.kind === 'forward' ? data.forward_enabled : data.mapping_enabled
              return (
                <li key={r.id} className="flex flex-wrap items-center justify-between gap-2 py-2">
                  <div className="min-w-0">
                    <p className="flex flex-wrap items-center gap-2 text-body font-medium text-ink">
                      {r.comment}
                      <Chip>{r.kind === 'forward' ? t('Forward') : t('Mapping')}</Chip>
                      <Chip>{protoLabel(r.proto)}</Chip>
                      {!on && <Chip tone="warn">{t('Switched off')}</Chip>}
                    </p>
                    <p className="tnum font-mono text-meta text-ink2">{ruleSummary(r)}</p>
                  </div>
                  <Button size="sm" variant="ghost" onClick={() => void remove(r)} loading={busy === `del:${r.id}`} disabled={busy !== null} aria-label={t('Delete {name}', { name: r.comment })}>
                    {t('Delete')}
                  </Button>
                </li>
              )
            })}
          </ul>
        )}

        {draft ? (
          <div className="space-y-3 rounded-ctl border border-line/8 p-3">
            <Segmented<PortRule['kind']>
              label={t('Rule type')}
              options={[
                { value: 'forward', label: t('Forward a port range') },
                { value: 'mapping', label: t('Map one port') },
              ]}
              value={draft.kind}
              onChange={(kind) => set({ kind })}
            />
            <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2 lg:grid-cols-3">
              <Field label={t('Device IP')} error={errors.ip}>
                <Input value={draft.ip} list="port-rule-ips" placeholder="192.168.0.20" onChange={(e) => set({ ip: e.target.value })} />
              </Field>
              <ClientIps id="port-rule-ips" clients={clients} />
              {draft.kind === 'forward' ? (
                <>
                  <Field label={t('First port')} error={errors.start}>
                    <Input inputMode="numeric" value={draft.start} onChange={(e) => set({ start: e.target.value })} />
                  </Field>
                  <Field label={t('Last port (optional)')} error={errors.end}>
                    <Input inputMode="numeric" value={draft.end} onChange={(e) => set({ end: e.target.value })} />
                  </Field>
                </>
              ) : (
                <>
                  <Field label={t('External port')} error={errors.start}>
                    <Input inputMode="numeric" value={draft.start} onChange={(e) => set({ start: e.target.value })} />
                  </Field>
                  <Field label={t('Device port')} error={errors.internal}>
                    <Input inputMode="numeric" value={draft.internal} onChange={(e) => set({ internal: e.target.value })} />
                  </Field>
                </>
              )}
              <Field label={t('Protocol')}>
                <Select value={draft.proto} onChange={(e) => set({ proto: e.target.value as PortRule['proto'] })}>
                  <option value="both">TCP+UDP</option>
                  <option value="tcp">TCP</option>
                  <option value="udp">UDP</option>
                </Select>
              </Field>
              <Field label={t('Name')} error={errors.comment}>
                <Input value={draft.comment} maxLength={32} placeholder="nas" onChange={(e) => set({ comment: e.target.value })} />
              </Field>
            </div>
            <div className="flex gap-2">
              <Button variant="primary" onClick={() => void add()} loading={busy === 'add'} disabled={busy !== null}>
                {t('Add rule')}
              </Button>
              <Button
                variant="ghost"
                onClick={() => {
                  setDraft(null)
                  setShowErrors(false)
                }}
                disabled={busy !== null}
              >
                {t('Cancel')}
              </Button>
            </div>
          </div>
        ) : (
          <Button variant="outline" onClick={() => setDraft(EMPTY)} disabled={busy !== null || data.rules.length >= data.max_per_kind * 2}>
            {t('Add rule')}
          </Button>
        )}
      </div>
    </Card>
  )
}

function FirewallCard({ clients, lan }: { clients: Client[]; lan: Lan }) {
  const res = useResource<FirewallServices>('router:firewall', api.firewall)
  const [busy, setBusy] = useState<string | null>(null)
  const [dmzDraft, setDmzDraft] = useState<string | null>(null)
  const [dmzError, setDmzError] = useState<string | undefined>()
  const f = res.data

  async function save(key: string, body: Partial<FirewallServices>, done: string) {
    setBusy(key)
    try {
      res.mutate(await api.firewallSet(body))
      toast(done)
      return true
    } catch (e) {
      toastError(e, t('Failed to save the setting'))
      return false
    } finally {
      setBusy(null)
    }
  }

  async function setRemote(on: boolean) {
    if (on) {
      const ok = await confirm({
        title: t('Allow management from the internet?'),
        body: t('The stock web interface becomes reachable from the mobile network side. Turn it off again when you are done.'),
        kind: 'danger',
        confirmLabel: t('Allow'),
      })
      if (!ok) return
    }
    await save('remote', { remote_web_access: on }, on ? t('Remote management on') : t('Remote management off'))
  }

  async function applyDmz(on: boolean) {
    if (on) {
      const ip = (dmzDraft ?? f?.dmz_ip ?? '').trim()
      const err = validateLanHost(ip, lan.ip, lan.mask)
      if (err) {
        setDmzError(err)
        return
      }
      const ok = await confirm({
        title: t('Expose {ip} as DMZ host?', { ip }),
        body: t('Every incoming connection that matches no other rule goes to this device, so it is exposed to the internet like an unprotected computer.'),
        kind: 'danger',
        confirmLabel: t('Enable DMZ'),
      })
      if (!ok) return
      if (await save('dmz', { dmz_enabled: true, dmz_ip: ip }, t('DMZ on'))) setDmzDraft(null)
    } else {
      await save('dmz', { dmz_enabled: false }, t('DMZ off'))
    }
  }

  if (res.status === 'loading') return <Skeleton className="h-40" />
  if (!f) {
    return (
      <InlineStatus kind="error" action={{ label: t('Retry'), onClick: res.refresh, loading: res.refreshing }}>
        {t('Firewall settings could not be read.')}
      </InlineStatus>
    )
  }

  const row = (label: string, hint: string, control: ReactNode) => (
    <div className="flex items-start justify-between gap-4 py-2.5">
      <div className="min-w-0">
        <p className="text-body font-semibold text-ink">{label}</p>
        <p className="text-meta text-ink2">{hint}</p>
      </div>
      {control}
    </div>
  )

  return (
    <Card title={t('UPnP, DMZ and remote access')}>
      <div className="divide-y divide-line/6">
        {row(
          'UPnP',
          t('Lets games and apps on the LAN open ports by themselves. Convenient, but any program on the LAN can use it.'),
          <Toggle checked={f.upnp} disabled={busy !== null} label="UPnP" onChange={(v) => void save('upnp', { upnp: v }, v ? t('UPnP on') : t('UPnP off'))} />,
        )}
        <div className="py-2.5">
          {row(
            'DMZ',
            t('Sends every unmatched incoming connection to one device.'),
            <Toggle checked={f.dmz_enabled} disabled={busy !== null} label="DMZ" onChange={(v) => void applyDmz(v)} />,
          )}
          <div className="flex flex-wrap items-end gap-2">
            <div className="w-48">
              <Field label={t('DMZ host')} error={dmzError}>
                <Input
                  value={dmzDraft ?? f.dmz_ip ?? ''}
                  list="dmz-ips"
                  placeholder="192.168.0.20"
                  onChange={(e) => {
                    setDmzDraft(e.target.value)
                    setDmzError(undefined)
                  }}
                />
              </Field>
              <ClientIps id="dmz-ips" clients={clients} />
            </div>
            {f.dmz_enabled && dmzDraft !== null && dmzDraft !== f.dmz_ip && (
              <Button size="sm" variant="primary" onClick={() => void applyDmz(true)} loading={busy === 'dmz'}>
                {t('Save')}
              </Button>
            )}
          </div>
        </div>
        {row(
          t('Remote management'),
          t('Lets the stock web interface be opened from the internet side. Lowers security; keep it off unless you need it.'),
          <Toggle checked={f.remote_web_access} disabled={busy !== null} label={t('Remote management')} onChange={(v) => void setRemote(v)} />,
        )}
        {row(
          t('Answer ping from the internet'),
          t('Off hides the router from ping scans on the mobile network side.'),
          <Toggle checked={f.wan_ping} disabled={busy !== null} label={t('Answer ping from the internet')} onChange={(v) => void save('ping', { wan_ping: v }, v ? t('Turned on') : t('Turned off'))} />,
        )}
      </div>
    </Card>
  )
}
