import { useState } from 'react'
import { api } from '../../data/api'
import { useResource } from '../../data/poll'
import { t } from '../../i18n'
import type { RebootSchedule, SleepSetting } from '../../types'
import { Button, Field, Input, Segmented, Select, Toggle } from '../../ui/controls'
import { toast, toastError } from '../../ui/feedback'
import { Card, InlineStatus, Skeleton } from '../../ui/primitives'
import { formatTime, parseTime, scheduleChanges, scheduleSummary, sleepLabel, weekdayName } from './scheduleView'

const WINDOWS = [0, 1, 2, 3, 4, 5, 6]

export default function PowerScheduleCard() {
  return (
    <Card title={t('Sleep and reboot')}>
      <div className="space-y-4">
        <SleepRow />
        <div className="border-t border-line/8 pt-3">
          <RebootRow />
        </div>
      </div>
    </Card>
  )
}

function SleepRow() {
  const res = useResource<SleepSetting>('device-sleep', api.sleep)
  const [busy, setBusy] = useState(false)
  const s = res.data

  async function change(minutes: number) {
    setBusy(true)
    try {
      res.mutate(await api.sleepSet(minutes))
      toast(t('Device sleep: {value}', { value: sleepLabel(minutes) }))
    } catch (e) {
      toastError(e, t('Failed to set device sleep'))
    } finally {
      setBusy(false)
    }
  }

  if (res.status === 'loading') return <Skeleton className="h-10" />
  if (!s) {
    return (
      <InlineStatus kind="error" action={{ label: t('Retry'), onClick: res.refresh, loading: res.refreshing }}>
        {t('Device sleep could not be read.')}
      </InlineStatus>
    )
  }
  const options = s.options.includes(s.minutes) ? s.options : [s.minutes, ...s.options]
  return (
    <div className="flex flex-wrap items-center justify-between gap-3">
      <div className="min-w-0 flex-1">
        <p className="text-body font-semibold text-ink">{t('Device sleep')}</p>
        <p className="text-meta text-ink2">{t('After this long idle the device sleeps and Wi-Fi is unavailable until it wakes.')}</p>
      </div>
      <div className="w-36">
        <Select
          aria-label={t('Device sleep')}
          value={s.minutes}
          disabled={busy}
          onChange={(e) => void change(Number(e.target.value))}
        >
          {options.map((m) => (
            <option key={m} value={m}>
              {sleepLabel(m)}
            </option>
          ))}
        </Select>
      </div>
    </div>
  )
}

function RebootRow() {
  const res = useResource<RebootSchedule>('reboot-schedule', api.rebootSchedule)
  const [draft, setDraft] = useState<RebootSchedule | null>(null)
  const [timeText, setTimeText] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const saved = res.data

  if (res.status === 'loading') return <Skeleton className="h-16" />
  if (!saved) {
    return (
      <InlineStatus kind="error" action={{ label: t('Retry'), onClick: res.refresh, loading: res.refreshing }}>
        {t('The reboot schedule could not be read.')}
      </InlineStatus>
    )
  }

  const cur = draft ?? saved
  const time = timeText ?? formatTime(cur.hour, cur.minute)
  const parsedTime = parseTime(time)
  const changes = draft ? scheduleChanges(saved, draft) : {}
  const dirty = Object.keys(changes).length > 0
  const edit = (patch: Partial<RebootSchedule>) => setDraft({ ...cur, ...patch })

  async function save() {
    if (!dirty || !parsedTime || busy) return
    setBusy(true)
    try {
      const next = await api.rebootScheduleSet(changes)
      res.mutate(next)
      setDraft(null)
      setTimeText(null)
      toast(next.enabled ? t('Reboot schedule saved') : t('Scheduled reboot turned off'))
    } catch (e) {
      toastError(e, t('Failed to save the reboot schedule'))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="min-w-0 flex-1">
          <p className="text-body font-semibold text-ink">{t('Scheduled reboot')}</p>
          <p className="text-meta text-ink2">{scheduleSummary(saved)}</p>
        </div>
        <Toggle checked={cur.enabled} onChange={(v) => edit({ enabled: v })} disabled={busy} label={t('Scheduled reboot')} />
      </div>

      {cur.enabled && (
        <div className="space-y-3">
          <Segmented<RebootSchedule['mode']>
            label={t('Repeat')}
            options={[
              { value: 'weekly', label: t('Weekly') },
              { value: 'interval', label: t('Every N days') },
            ]}
            value={cur.mode}
            onChange={(mode) => edit({ mode })}
            disabled={busy}
          />
          <div className="flex flex-wrap items-end gap-3">
            {cur.mode === 'weekly' ? (
              <div className="w-36">
                <Field label={t('Day')}>
                  <Select value={cur.weekday} onChange={(e) => edit({ weekday: Number(e.target.value) })} disabled={busy}>
                    {[1, 2, 3, 4, 5, 6, 0].map((d) => (
                      <option key={d} value={d}>
                        {weekdayName(d)}
                      </option>
                    ))}
                  </Select>
                </Field>
              </div>
            ) : (
              <div className="w-28">
                <Field label={t('Every (days)')}>
                  <Select value={cur.interval_days} onChange={(e) => edit({ interval_days: Number(e.target.value) })} disabled={busy}>
                    {Array.from({ length: 30 }, (_, i) => i + 1).map((n) => (
                      <option key={n} value={n}>
                        {n}
                      </option>
                    ))}
                  </Select>
                </Field>
              </div>
            )}
            <div className="w-32">
              <Field label={t('Reboot time')} error={parsedTime ? undefined : t('Enter a time')}>
                <Input
                  type="time"
                  value={time}
                  disabled={busy}
                  onChange={(e) => {
                    setTimeText(e.target.value)
                    const p = parseTime(e.target.value)
                    if (p) edit(p)
                  }}
                />
              </Field>
            </div>
            <div className="w-32">
              <Field label={t('Random delay')}>
                <Select value={cur.window_hours} onChange={(e) => edit({ window_hours: Number(e.target.value) })} disabled={busy}>
                  {WINDOWS.map((h) => (
                    <option key={h} value={h}>
                      {h === 0 ? t('None') : t('Up to {n} h', { n: h })}
                    </option>
                  ))}
                </Select>
              </Field>
            </div>
          </div>
          <p className="text-meta text-ink3">
            {t('The device reboots at a random moment within the delay after the set time. All connections drop for about a minute; the proxy and other services start again on their own.')}
          </p>
        </div>
      )}

      {dirty && (
        <div className="flex gap-2">
          <Button variant="primary" onClick={() => void save()} loading={busy} disabled={!parsedTime}>
            {t('Save')}
          </Button>
          <Button
            variant="ghost"
            onClick={() => {
              setDraft(null)
              setTimeText(null)
            }}
            disabled={busy}
          >
            {t('Cancel')}
          </Button>
        </div>
      )}
    </div>
  )
}
