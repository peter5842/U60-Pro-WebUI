import { useState } from 'react'
import { api } from '../../data/api'
import { useResource } from '../../data/poll'
import { t } from '../../i18n'
import type { ClockStatus, WatchdogSettings } from '../../types'
import { Button, Field, Input, Toggle } from '../../ui/controls'
import { toast, toastError } from '../../ui/feedback'
import { Card, InlineStatus, Row, Skeleton } from '../../ui/primitives'
import { validateWatchdog } from './scheduleView'

export default function WatchdogCard() {
  const res = useResource<WatchdogSettings>('router:watchdog', api.watchdog)
  const [draft, setDraft] = useState<{ enabled: boolean; host: string; interval: string; failures: string } | null>(null)
  const [busy, setBusy] = useState(false)
  const [errors, setErrors] = useState<ReturnType<typeof validateWatchdog>>({})
  const w = res.data

  if (res.status === 'loading') return <Skeleton className="h-24" />
  if (!w) {
    return (
      <InlineStatus kind="error" action={{ label: t('Retry'), onClick: res.refresh, loading: res.refreshing }}>
        {t('The connection watchdog could not be read.')}
      </InlineStatus>
    )
  }
  const cur = draft ?? { enabled: w.enabled, host: w.host ?? '223.5.5.5', interval: String(w.interval_minutes ?? 5), failures: String(w.failures ?? 3) }
  const dirty =
    draft !== null &&
    (draft.enabled !== w.enabled ||
      (draft.enabled && (draft.host !== (w.host ?? '') || draft.interval !== String(w.interval_minutes ?? '') || draft.failures !== String(w.failures ?? ''))))

  async function save() {
    if (!draft) return
    const e = draft.enabled ? validateWatchdog(draft.host, draft.interval, draft.failures) : {}
    setErrors(e)
    if (Object.keys(e).length > 0) return
    setBusy(true)
    try {
      const next = await api.watchdogSet(
        draft.enabled ? { enabled: true, host: draft.host.trim(), interval_minutes: Number(draft.interval), failures: Number(draft.failures) } : { enabled: false },
      )
      res.mutate(next)
      setDraft(null)
      toast(next.enabled ? t('Watchdog on') : t('Watchdog off'))
    } catch (err) {
      toastError(err, t('Failed to save the watchdog'))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Card title={t('Connection watchdog')}>
      <div className="space-y-3">
        <div className="flex items-start justify-between gap-4">
          <p className="text-meta text-ink2">
            {t('The router pings an address regularly and reboots itself when the pings keep failing, which recovers a stuck mobile connection. After 5 frequent reboots the firmware turns it off.')}
          </p>
          <Toggle checked={cur.enabled} onChange={(v) => setDraft({ ...cur, enabled: v })} disabled={busy} label={t('Connection watchdog')} />
        </div>
        {cur.enabled && (
          <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-3">
            <Field label={t('Address to ping')} error={errors.host}>
              <Input value={cur.host} onChange={(e) => setDraft({ ...cur, host: e.target.value })} disabled={busy} placeholder="223.5.5.5" />
            </Field>
            <Field label={t('Interval (minutes)')} error={errors.interval}>
              <Input inputMode="numeric" value={cur.interval} onChange={(e) => setDraft({ ...cur, interval: e.target.value })} disabled={busy} />
            </Field>
            <Field label={t('Reboot after failures')} error={errors.failures}>
              <Input inputMode="numeric" value={cur.failures} onChange={(e) => setDraft({ ...cur, failures: e.target.value })} disabled={busy} />
            </Field>
          </div>
        )}
        {dirty && (
          <div className="flex flex-wrap items-center gap-2">
            <Button variant="primary" onClick={() => void save()} loading={busy}>
              {t('Save')}
            </Button>
            <Button variant="ghost" onClick={() => setDraft(null)} disabled={busy}>
              {t('Cancel')}
            </Button>
            {busy && draft?.enabled && <span className="text-meta text-ink3">{t('Checking that the address answers…')}</span>}
          </div>
        )}
      </div>
    </Card>
  )
}

/** The router clock, read only: the firmware keeps local time and sets it from the mobile network. */
export function ClockRow() {
  const res = useResource<ClockStatus>('system:clock', api.clock)
  const c = res.data
  if (!c?.local_time) return null
  const source = c.source === 'NITZ' ? t('from the mobile network') : c.source === 'SNTP' ? t('from NTP servers') : c.source === 'MANUAL' ? t('set by hand') : c.source
  const offset = c.utc_offset_hours != null ? ` (UTC${c.utc_offset_hours >= 0 ? '+' : ''}${c.utc_offset_hours})` : ''
  return <Row label={t('Router clock')} value={`${c.local_time}${offset}${source ? ` · ${source}` : ''}`} mono />
}
