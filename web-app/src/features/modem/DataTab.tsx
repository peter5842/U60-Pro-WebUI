import { useState, type ReactNode } from 'react'
import { useHome } from '../../app/HomeContext'
import { api } from '../../data/api'
import { usageTotal } from '../../data/usage'
import { formatBytes, formatCounterTime } from '../../format'
import { t } from '../../i18n'
import { IDownload, IUpload } from '../../icons'
import type { UsagePeriod } from '../../types'
import { Button, Field, Input } from '../../ui/controls'
import { toast, toastError } from '../../ui/feedback'
import { Card, InlineStatus, Skeleton, Unavailable } from '../../ui/primitives'
import { cycleView, parseResetDay, resetDayCopy } from './usageView'
import DataLimitCard from './DataLimitCard'
import MobileDataCard from './MobileDataCard'

/** A byte counter: a real zero reads "0 B"; an unknown counter reads as unavailable, never zero. */
function Bytes({ value }: { value: number | null }) {
  return value === null ? <Unavailable /> : <>{formatBytes(value)}</>
}

function CounterTime({ secs }: { secs: number | null }) {
  return secs === null ? <Unavailable /> : <>{formatCounterTime(secs)}</>
}

function UsageTotals({ usage }: { usage: UsagePeriod }) {
  const total = usageTotal(usage)
  return (
    <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
      <div className="rounded-ctl bg-surface2/70 p-3">
        <p className="label text-ok">{t('Download')}</p>
        <p className="tnum font-mono mt-1 text-xl font-medium text-ink"><Bytes value={usage.rx_bytes} /></p>
      </div>
      <div className="rounded-ctl bg-surface2/70 p-3">
        <p className="label text-accent">{t('Upload')}</p>
        <p className="tnum font-mono mt-1 text-xl font-medium text-ink"><Bytes value={usage.tx_bytes} /></p>
      </div>
      <div className="rounded-ctl bg-surface2/70 p-3">
        <p className="label">{t('Total')}</p>
        <p className="tnum font-mono mt-1 text-xl font-medium text-ink"><Bytes value={total} /></p>
      </div>
    </div>
  )
}

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex gap-1">
      <dt>{t('{label}:', { label })}</dt>
      <dd className="font-bold text-ink">{children}</dd>
    </div>
  )
}

export default function DataTab() {
  // The home batch already carries data_usage; a second poll for it was
  // duplicate ubus load on the agent. Reset-day saves publish the agent's returned
  // usage into that same resource so Home and this tab cannot disagree.
  const home = useHome()
  const usage = home.data?.usage ?? null
  const [editing, setEditing] = useState(false)
  const [resetDay, setResetDay] = useState('')
  const [fieldError, setFieldError] = useState<string | undefined>()
  const [busy, setBusy] = useState(false)

  function openEditor() {
    if (editing) {
      setEditing(false)
      return
    }
    // Seed from the device value only when it is known; never default an unknown day to 1.
    setResetDay(usage?.reset_day != null ? String(usage.reset_day) : '')
    setFieldError(undefined)
    setEditing(true)
  }

  async function saveResetDay() {
    if (busy || !usage || !home.data) return
    const parsed = parseResetDay(resetDay)
    if (!parsed.ok) {
      setFieldError(parsed.error)
      return
    }
    setFieldError(undefined)
    const copy = resetDayCopy(cycleView(usage).state)
    setBusy(true)
    try {
      // The reply is the full usage payload read back after the change (it also turns automatic
      // reset on), so it is authoritative: publish it, then re-read the heartbeat.
      const next = await api.dataUsageResetDaySet(parsed.day)
      home.mutate({ ...home.data, usage: next })
      home.refresh()
      setEditing(false)
      if (copy.turnsOn) toast(t('Reset day saved as day {day}. Automatic reset is now on.', { day: parsed.day }))
    } catch (e) {
      // Keep the editor and the draft so the user can retry.
      toastError(e, t('Failed to set reset day'))
    } finally {
      setBusy(false)
    }
  }

  if (!usage) {
    if (home.status === 'error') {
      return (
        <InlineStatus kind="error" action={{ label: t('Retry'), onClick: home.refresh, loading: home.refreshing }}>
          {home.error ? t('Data usage could not be loaded: {error}', { error: home.error }) : t('Data usage could not be loaded.')}
        </InlineStatus>
      )
    }
    if (home.data) {
      return (
        <InlineStatus kind="warn" action={{ label: t('Retry'), onClick: home.refresh, loading: home.refreshing }}>
          {t('The router did not report data usage.')}
        </InlineStatus>
      )
    }
    return (
      <div className="space-y-3">
        <Skeleton className="h-48" />
        <Skeleton className="h-32" />
      </div>
    )
  }

  const cycle = usage.cycle ?? usage.month
  const view = cycleView(usage)
  const copy = resetDayCopy(view.state)
  const sincePowerOn = usage.since_power_on
  const otherCounters = [
    { label: t('Today'), data: usage.day },
    { label: t('Device lifetime'), data: usage.total },
  ]

  return (
    <div className="space-y-3">
      <MobileDataCard />
      {home.status === 'stale' && (
        <InlineStatus kind="stale" action={{ label: t('Retry'), onClick: home.refresh, loading: home.refreshing }}>
          {t('Showing the last usage the router reported. The latest refresh failed.')}
        </InlineStatus>
      )}

      <Card
        title={t('Current cycle')}
        action={
          <Button size="sm" variant="ghost" onClick={openEditor} aria-expanded={editing}>
            {t('Set reset day')}
          </Button>
        }
      >
        {editing && (
          <form
            noValidate
            className="mb-3 flex flex-wrap items-start gap-2 rounded-ctl bg-surface2/70 p-3"
            onSubmit={(e) => {
              e.preventDefault()
              void saveResetDay()
            }}
          >
            <div className="w-full max-w-sm">
              <Field label={t('Reset day')} hint={copy.hint} error={fieldError}>
                {(ids) => (
                  <Input
                    id={ids.id}
                    type="number"
                    inputMode="numeric"
                    min={1}
                    max={31}
                    step={1}
                    value={resetDay}
                    onChange={(e) => setResetDay(e.target.value)}
                    aria-describedby={ids.describedBy}
                    aria-invalid={ids.invalid || undefined}
                  />
                )}
              </Field>
            </div>
            <div className="flex items-center gap-2 pt-5">
              <Button type="submit" variant="primary" loading={busy}>
                {copy.button}
              </Button>
              <Button type="button" variant="ghost" disabled={busy} onClick={() => setEditing(false)}>
                {t('Cancel')}
              </Button>
            </div>
          </form>
        )}

        {cycle ? (
          <div className="space-y-3">
            {view.headline && <p className="text-body font-semibold text-ink">{view.headline}</p>}
            <dl className="flex flex-wrap gap-x-4 gap-y-1 text-body text-ink2">
              <Fact label={t('Automatic reset')}>{view.stateLabel}</Fact>
              <Fact label={t('Reset day')}>{view.resetDay ?? <Unavailable />}</Fact>
              <Fact label={t('Cycle start')}>{view.cycleStart ?? <Unavailable />}</Fact>
              {view.showNextReset && <Fact label={t('Next reset')}>{view.nextReset ?? <Unavailable />}</Fact>}
            </dl>
            <UsageTotals usage={cycle} />
            <p className="text-meta text-ink3">{view.note}</p>
          </div>
        ) : (
          <p className="text-body text-ink3">{t('No cycle data')}</p>
        )}
      </Card>

      <DataLimitCard cycleUsed={cycle ? usageTotal(cycle) : null} />

      {sincePowerOn && (
        <Card title={t('Connection counters')}>
          <UsageTotals usage={sincePowerOn} />
          <p className="mt-2 text-meta text-ink3">
            {t('Counter time:')} <CounterTime secs={sincePowerOn.time_secs} />
          </p>
          <p className="mt-0.5 text-meta text-ink3">{t('These counters restart when the mobile data connection restarts.')}</p>
        </Card>
      )}

      <Card title={t('Other counters')} pad={false}>
        {/* Mobile: stacked rows instead of a five-column table */}
        <ul className="divide-y divide-line/6 px-4 sm:hidden">
          {otherCounters.map(({ label, data: d }) => (
            <li key={label} className="py-2.5">
              <div className="flex items-baseline justify-between gap-2 text-body">
                <span className="text-ink2">{label}</span>
                <span className="tnum font-mono font-semibold text-ink"><Bytes value={usageTotal(d)} /></span>
              </div>
              <div className="tnum font-mono mt-0.5 flex flex-wrap gap-x-3 text-meta">
                <span className="flex items-center gap-1 text-ok"><IDownload size={12} /> <Bytes value={d.rx_bytes} /></span>
                <span className="flex items-center gap-1 text-accent"><IUpload size={12} /> <Bytes value={d.tx_bytes} /></span>
                <span className="ml-auto text-ink3"><CounterTime secs={d.time_secs} /></span>
              </div>
            </li>
          ))}
        </ul>
        <div className="hidden overflow-x-auto px-4 pb-3 sm:block">
          <table className="w-full text-body">
            <thead>
              <tr className="label border-b border-line/8 text-left">
                <th className="pb-1.5 pr-4 font-semibold">{t('Period')}</th>
                <th className="pb-1.5 pr-4 text-right font-semibold">{t('Down')}</th>
                <th className="pb-1.5 pr-4 text-right font-semibold">{t('Up')}</th>
                <th className="pb-1.5 pr-4 text-right font-semibold">{t('Total')}</th>
                <th className="pb-1.5 text-right font-semibold">{t('Time')}</th>
              </tr>
            </thead>
            <tbody>
              {otherCounters.map(({ label, data: d }) => (
                <tr key={label} className="border-b border-line/6 last:border-0">
                  <td className="py-2 pr-4 text-ink2">{label}</td>
                  <td className="tnum font-mono py-2 pr-4 text-right text-ok"><Bytes value={d.rx_bytes} /></td>
                  <td className="tnum font-mono py-2 pr-4 text-right text-accent"><Bytes value={d.tx_bytes} /></td>
                  <td className="tnum font-mono py-2 pr-4 text-right font-semibold text-ink">
                    <Bytes value={usageTotal(d)} />
                  </td>
                  <td className="tnum font-mono py-2 text-right text-ink3"><CounterTime secs={d.time_secs} /></td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Card>
    </div>
  )
}
