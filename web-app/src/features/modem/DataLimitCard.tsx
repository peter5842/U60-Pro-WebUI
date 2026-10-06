import { useState } from 'react'
import { api } from '../../data/api'
import { useResource } from '../../data/poll'
import { formatBytes } from '../../format'
import { t } from '../../i18n'
import type { DataLimit } from '../../types'
import { Button, Field, Input, Toggle } from '../../ui/controls'
import { toast, toastError } from '../../ui/feedback'
import { Card, InlineStatus, Meter, Skeleton } from '../../ui/primitives'
import { bytesToGbText, limitPct, parseAlertPercent, parseLimitGb } from './dataLimitView'

/** `cycleUsed`: bytes used in the current cycle (from the home batch). */
export default function DataLimitCard({ cycleUsed }: { cycleUsed: number | null }) {
  const limit = useResource<DataLimit>('data-limit', api.dataLimit)
  const [draft, setDraft] = useState<{ enabled: boolean; gb: string; alert: string } | null>(null)
  const [errors, setErrors] = useState<{ gb?: string; alert?: string }>({})
  const [busy, setBusy] = useState(false)
  const l = limit.data

  const current = draft ?? {
    enabled: l?.enabled ?? false,
    gb: bytesToGbText(l?.limit_bytes) || '100',
    alert: String(l?.alert_percent ?? 80),
  }

  async function save() {
    if (busy) return
    let body: { enabled: boolean; limit_bytes?: number; alert_percent?: number } = { enabled: current.enabled }
    if (current.enabled) {
      const gb = parseLimitGb(current.gb)
      const alert = parseAlertPercent(current.alert)
      setErrors({ gb: gb.ok ? undefined : gb.error, alert: alert.ok ? undefined : alert.error })
      if (!gb.ok || !alert.ok) return
      body = { enabled: true, limit_bytes: gb.bytes, alert_percent: alert.percent }
    }
    setBusy(true)
    try {
      limit.mutate(await api.dataLimitSet(body))
      setDraft(null)
      toast(current.enabled ? t('Monthly limit saved') : t('Monthly limit turned off'))
    } catch (e) {
      toastError(e, t('Failed to save the monthly limit'))
    } finally {
      setBusy(false)
    }
  }

  let body
  if (limit.status === 'loading') body = <Skeleton className="h-20" />
  else if (!l)
    body = (
      <InlineStatus kind="error" action={{ label: t('Retry'), onClick: limit.refresh, loading: limit.refreshing }}>
        {limit.error
          ? t('The monthly limit could not be read: {error}', { error: limit.error })
          : t('The monthly limit could not be read.')}
      </InlineStatus>
    )
  else if (l.kind === 'time' && l.enabled)
    body = (
      <InlineStatus kind="info" live={false}>
        {t('A connection-time limit is set in the stock web UI; it can be changed there.')}
      </InlineStatus>
    )
  else {
    const pct = l.enabled ? limitPct(cycleUsed, l.limit_bytes) : undefined
    const changed =
      draft !== null &&
      (draft.enabled !== l.enabled ||
        (draft.enabled && (draft.gb !== bytesToGbText(l.limit_bytes) || draft.alert !== String(l.alert_percent ?? ''))))
    body = (
      <div className="space-y-3">
        <div className="flex items-center justify-between gap-4">
          <p className="text-meta text-ink2">{t('Warn when this billing cycle’s usage approaches the limit.')}</p>
          <Toggle
            checked={current.enabled}
            onChange={(v) => setDraft({ ...current, enabled: v })}
            disabled={busy}
            label={t('Monthly limit')}
          />
        </div>
        {pct !== undefined && (
          <div className="space-y-1">
            <Meter pct={pct} tone={pct >= 100 ? 'bg-danger' : pct >= (l.alert_percent ?? 80) ? 'bg-warn' : 'bg-accent'} />
            <p className="tnum font-mono text-meta text-ink2">
              {t('{used} of {limit} ({pct}%)', { used: formatBytes(cycleUsed), limit: formatBytes(l.limit_bytes), pct })}
            </p>
          </div>
        )}
        {current.enabled && (
          <div className="flex flex-wrap items-end gap-3">
            <div className="w-36">
              <Field label={t('Limit (GB)')} error={errors.gb}>
                <Input
                  inputMode="decimal"
                  value={current.gb}
                  onChange={(e) => setDraft({ ...current, gb: e.target.value })}
                  disabled={busy}
                />
              </Field>
            </div>
            <div className="w-36">
              <Field label={t('Alert at (%)')} error={errors.alert}>
                <Input
                  inputMode="numeric"
                  value={current.alert}
                  onChange={(e) => setDraft({ ...current, alert: e.target.value })}
                  disabled={busy}
                />
              </Field>
            </div>
          </div>
        )}
        {changed && (
          <div className="flex gap-2">
            <Button variant="primary" onClick={() => void save()} loading={busy}>
              {t('Save')}
            </Button>
            <Button variant="ghost" onClick={() => setDraft(null)} disabled={busy}>
              {t('Cancel')}
            </Button>
          </div>
        )}
      </div>
    )
  }

  return <Card title={t('Monthly limit')}>{body}</Card>
}
