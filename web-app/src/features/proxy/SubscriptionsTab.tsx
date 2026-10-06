import { useState } from 'react'
import { api } from '../../data/api'
import { usePoll } from '../../data/poll'
import { formatBytes } from '../../format'
import { ITrash } from '../../icons'
import type { ProxySubscription, ProxySubscriptions } from '../../types'
import { Button, Field, Input, Select, Toggle } from '../../ui/controls'
import { confirm, toast, toastError } from '../../ui/feedback'
import { Card, Chip, Empty, InlineStatus, Meter, Skeleton } from '../../ui/primitives'
import { ago, expiry, INTERVAL_OPTIONS, intervalLabel, usagePct, usageTone, usedBytes, validateSubscription } from './proxyView'

export default function SubscriptionsTab() {
  const subs = usePoll<ProxySubscriptions>('proxy-subscriptions', api.proxySubscriptions, 15_000)
  const [updatingAll, setUpdatingAll] = useState(false)

  async function updateAll() {
    setUpdatingAll(true)
    try {
      const r = await api.proxySubscriptionUpdate()
      const results = (r.results ?? {}) as Record<string, { ok?: boolean }>
      const failed = Object.values(results).filter((x) => !x.ok).length
      if (failed) toast(`${failed} subscription${failed > 1 ? 's' : ''} failed to update`, 'err')
      else toast('Subscriptions updated')
    } catch (e) {
      toastError(e, 'Failed to update subscriptions')
    } finally {
      setUpdatingAll(false)
      subs.refresh()
    }
  }

  let list
  if (subs.status === 'loading') {
    list = <Skeleton className="h-24" />
  } else if (!subs.data) {
    list = (
      <InlineStatus kind="error" action={{ label: 'Retry', onClick: subs.refresh, loading: subs.refreshing }}>
        Subscriptions could not be read{subs.error ? `: ${subs.error}` : '.'}
      </InlineStatus>
    )
  } else if (subs.data.subscriptions.length === 0) {
    list = <Empty title="No subscriptions yet" body="Add your provider's subscription link below to get nodes." />
  } else {
    list = (
      <div className="space-y-3">
        {!subs.data.running && (
          <InlineStatus kind="info" live={false}>
            The proxy is stopped. Node counts, usage and updates appear once it runs.
          </InlineStatus>
        )}
        <ul className="divide-y divide-line/6">
          {subs.data.subscriptions.map((s) => (
            <SubscriptionRow key={s.id} sub={s} running={subs.data!.running} onChanged={subs.refresh} />
          ))}
        </ul>
      </div>
    )
  }

  const canUpdateAll = !!subs.data?.running && subs.data.subscriptions.some((s) => s.enabled)

  return (
    <>
      <Card
        title="Subscriptions"
        action={
          <Button size="sm" variant="outline" onClick={() => void updateAll()} loading={updatingAll} disabled={!canUpdateAll}>
            Update all
          </Button>
        }
      >
        {list}
      </Card>
      <AddSubscription onAdded={subs.refresh} />
    </>
  )
}

function SubscriptionRow({ sub, running, onChanged }: { sub: ProxySubscription; running: boolean; onChanged: () => void }) {
  const [busy, setBusy] = useState<'update' | 'toggle' | 'delete' | null>(null)
  const [editing, setEditing] = useState(false)
  const pct = usagePct(sub.usage)
  const used = usedBytes(sub.usage)
  const exp = expiry(sub.usage)
  const updated = ago(sub.updated_at)

  async function update() {
    setBusy('update')
    try {
      const r = await api.proxySubscriptionUpdate(sub.id)
      const result = ((r.results ?? {}) as Record<string, { ok?: boolean; error?: string }>)[sub.id]
      if (result && !result.ok) toast(`${sub.name}: ${result.error ?? 'update failed'}`, 'err')
      else toast(`${sub.name} updated`)
    } catch (e) {
      toastError(e, 'Failed to update the subscription')
    } finally {
      setBusy(null)
      onChanged()
    }
  }

  async function toggle(enabled: boolean) {
    setBusy('toggle')
    try {
      await api.proxySubscriptionEdit({ id: sub.id, enabled })
    } catch (e) {
      toastError(e, 'Failed to change the subscription')
    } finally {
      setBusy(null)
      onChanged()
    }
  }

  async function remove() {
    const ok = await confirm({
      title: `Delete ${sub.name}?`,
      body: 'Its nodes are removed from the proxy. The link is deleted from the router.',
      kind: 'danger',
      confirmLabel: 'Delete',
      details: [{ label: 'Link', value: sub.url_masked }],
    })
    if (!ok) return
    setBusy('delete')
    try {
      await api.proxySubscriptionDelete(sub.id)
      toast(`${sub.name} deleted`)
    } catch (e) {
      toastError(e, 'Failed to delete the subscription')
    } finally {
      setBusy(null)
      onChanged()
    }
  }

  return (
    <li className="space-y-2 py-3 first:pt-0 last:pb-0">
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="font-semibold text-ink">{sub.name}</span>
            {!sub.enabled && <Chip>Off</Chip>}
            {sub.node_count !== undefined && <Chip tone="accent">{sub.node_count} nodes</Chip>}
            <Chip>{intervalLabel(sub.interval_hours)}</Chip>
          </div>
          <p className="mt-0.5 break-all font-mono text-caption text-ink3">{sub.url_masked}</p>
        </div>
        <div className="flex items-center gap-1">
          <Toggle
            checked={sub.enabled}
            onChange={(v) => void toggle(v)}
            disabled={!!busy}
            label={`Use ${sub.name}`}
          />
          <Button size="sm" variant="ghost" onClick={() => setEditing((v) => !v)} disabled={!!busy}>
            {editing ? 'Close' : 'Edit'}
          </Button>
          <Button
            size="sm"
            variant="outline"
            onClick={() => void update()}
            loading={busy === 'update'}
            disabled={!!busy || !running || !sub.enabled}
          >
            Update
          </Button>
          <Button size="sm" variant="ghost" onClick={() => void remove()} loading={busy === 'delete'} disabled={!!busy} aria-label={`Delete ${sub.name}`}>
            <ITrash size={15} />
          </Button>
        </div>
      </div>

      {(pct !== undefined || used !== undefined || exp) && (
        <div className="space-y-1">
          {pct !== undefined && <Meter pct={pct} tone={usageTone(pct)} />}
          <div className="flex flex-wrap items-center justify-between gap-2 text-meta text-ink2">
            <span className="tnum font-mono">
              {formatBytes(used)}
              {sub.usage?.total ? ` of ${formatBytes(sub.usage.total)}` : ''}
              {pct !== undefined ? ` (${pct}%)` : ''}
            </span>
            {exp && (
              <Chip tone={exp.tone}>
                {exp.daysLeft < 0 ? `Expired ${exp.date}` : `Expires ${exp.date} · ${exp.daysLeft} d`}
              </Chip>
            )}
          </div>
        </div>
      )}

      <p className="text-caption text-ink3">{updated ? `Updated ${updated}` : running && sub.enabled ? 'Not fetched yet' : ''}</p>
      {sub.error && <InlineStatus kind="error">{sub.error}</InlineStatus>}
      {editing && (
        <EditSubscription
          sub={sub}
          onDone={() => {
            setEditing(false)
            onChanged()
          }}
        />
      )}
    </li>
  )
}

function EditSubscription({ sub, onDone }: { sub: ProxySubscription; onDone: () => void }) {
  const [name, setName] = useState(sub.name)
  const [url, setUrl] = useState('')
  const [intervalHours, setIntervalHours] = useState(sub.interval_hours)
  const [errors, setErrors] = useState<{ name?: string; url?: string }>({})
  const [busy, setBusy] = useState(false)

  async function save() {
    const v = validateSubscription(name, url || 'https://unchanged')
    if (v.name || (url && v.url)) {
      setErrors({ name: v.name, url: url ? v.url : undefined })
      return
    }
    setBusy(true)
    try {
      await api.proxySubscriptionEdit({
        id: sub.id,
        name: name.trim(),
        interval_hours: intervalHours,
        ...(url.trim() ? { url: url.trim() } : {}),
      })
      toast(`${name.trim()} saved`)
      onDone()
    } catch (e) {
      toastError(e, 'Failed to save the subscription')
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="grid gap-3 rounded-ctl border border-line/10 p-3 sm:grid-cols-2">
      <Field label="Name" error={errors.name}>
        <Input value={name} maxLength={32} onChange={(e) => setName(e.target.value)} />
      </Field>
      <Field label="Update interval">
        <Select value={intervalHours} onChange={(e) => setIntervalHours(Number(e.target.value))}>
          {INTERVAL_OPTIONS.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </Select>
      </Field>
      <div className="sm:col-span-2">
        <Field label="New link" hint="Leave empty to keep the current link." error={errors.url}>
          <Input
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            placeholder="https://…"
            autoComplete="off"
            spellCheck={false}
            inputMode="url"
          />
        </Field>
      </div>
      <div className="flex gap-2 sm:col-span-2">
        <Button variant="primary" onClick={() => void save()} loading={busy}>
          Save
        </Button>
        <Button variant="ghost" onClick={onDone} disabled={busy}>
          Cancel
        </Button>
      </div>
    </div>
  )
}

function AddSubscription({ onAdded }: { onAdded: () => void }) {
  const [name, setName] = useState('')
  const [url, setUrl] = useState('')
  const [intervalHours, setIntervalHours] = useState(24)
  const [errors, setErrors] = useState<{ name?: string; url?: string }>({})
  const [busy, setBusy] = useState(false)

  async function add() {
    const v = validateSubscription(name, url)
    setErrors(v)
    if (v.name || v.url) return
    setBusy(true)
    try {
      await api.proxySubscriptionAdd({ name: name.trim(), url: url.trim(), interval_hours: intervalHours })
      toast(`${name.trim()} added`)
      setName('')
      setUrl('')
      onAdded()
    } catch (e) {
      toastError(e, 'Failed to add the subscription')
    } finally {
      setBusy(false)
    }
  }

  return (
    <Card title="Add subscription">
      <form
        className="grid gap-3 sm:grid-cols-2"
        onSubmit={(e) => {
          e.preventDefault()
          void add()
        }}
      >
        <Field label="Name" error={errors.name}>
          <Input value={name} maxLength={32} placeholder="My provider" onChange={(e) => setName(e.target.value)} />
        </Field>
        <Field label="Update interval">
          <Select value={intervalHours} onChange={(e) => setIntervalHours(Number(e.target.value))}>
            {INTERVAL_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </Select>
        </Field>
        <div className="sm:col-span-2">
          <Field
            label="Subscription link"
            hint="Clash/mihomo or base64 (v2ray) format. Stored on the router readable by root only, and shown masked."
            error={errors.url}
          >
            <Input
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="https://…"
              autoComplete="off"
              spellCheck={false}
              inputMode="url"
            />
          </Field>
        </div>
        <div className="sm:col-span-2">
          <Button type="submit" variant="primary" loading={busy}>
            Add subscription
          </Button>
        </div>
      </form>
    </Card>
  )
}
