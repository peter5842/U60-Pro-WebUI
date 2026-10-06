import { useState } from 'react'
import { api } from '../../data/api'
import { t } from '../../i18n'
import { usePoll } from '../../data/poll'
import { tempColorClass } from '../../format'
import type { BatteryBspInfo, BatteryDetail, ChargeControlState, CpuInfo, MemInfo, ThermalAll } from '../../types'
import { formatBytes } from '../../format'
import { Button, Field, Toggle } from '../../ui/controls'
import { Card, InlineStatus, Meter, Skeleton } from '../../ui/primitives'
import { LIMIT_MAX, LIMIT_MIN, LIMIT_STEP, limitToApply } from './chargeLimit'

interface MetricsData {
  thermal: ThermalAll | null
  battery: BatteryDetail | null
  batteryInfo: BatteryBspInfo | null
  cpu: CpuInfo | null
  mem: MemInfo | null
}

function ThermalBar({ label, value }: { label: string; value?: number | null }) {
  if (value == null) {
    return (
      <div className="flex justify-between text-meta">
        <span className="text-ink2">{label}</span>
        <span className="font-medium text-ink3">{t('Unavailable')}</span>
      </div>
    )
  }
  const pct = Math.min((value / 100) * 100, 100)
  const tone = value > 80 ? 'bg-danger' : value > 60 ? 'bg-warn' : 'bg-ok'
  return (
    <div>
      <div className="mb-0.5 flex justify-between text-meta">
        <span className="text-ink2">{label}</span>
        <span className={`tnum font-mono font-semibold ${tempColorClass(value)}`}>{value.toFixed(1)}°C</span>
      </div>
      <Meter pct={pct} tone={tone} />
    </div>
  )
}

// ── Charge control ────────────────────────────────────────────────────────────

function ChargeControlCard() {
  const { data: cc, error, status, refresh, mutate } = usePoll('charge-control', api.chargeControl, 10000)
  const [busy, setBusy] = useState(false)
  const [failure, setFailure] = useState<string | null>(null)
  // Local edit of the limit. `null` = pristine: the slider shows the device's
  // value, and polls can never overwrite a value the user is still choosing.
  const [draft, setDraft] = useState<number | null>(null)

  async function apply(body: Partial<ChargeControlState>) {
    if (busy) return
    setBusy(true)
    setFailure(null)
    try {
      // The PUT returns the authoritative new state — publish it rather than
      // spending a second request re-reading what we were just handed.
      mutate(await api.chargeControlSet(body))
      if ('charge_limit' in body) setDraft(null)
    } catch (e) {
      setFailure(e instanceof Error ? e.message : t('Charge control failed'))
    } finally {
      setBusy(false)
    }
  }

  if (!cc) {
    return (
      <Card title={t('Charge control')}>
        {status === 'error' ? (
          <InlineStatus kind="error" action={{ label: t('Retry'), onClick: refresh }}>
            {t('Charge control unavailable: {error}', { error: error ?? '' })}
          </InlineStatus>
        ) : (
          <Skeleton className="h-32" />
        )}
      </Card>
    )
  }

  const shown = draft ?? cc.charge_limit
  const pending = limitToApply(draft, cc.charge_limit)
  const limitEditable = cc.charge_limit_enabled && cc.battery_available
  const batteryText = cc.battery_status != null ? batteryStatusLabel(cc.battery_status) : t('Battery data unavailable')
  const chargingSummary = [
    cc.capacity != null ? t('{status} at {capacity}%', { status: batteryText, capacity: cc.capacity }) : batteryText,
    ...(cc.manual_override ? [t('manual override')] : []),
  ].join(' · ')

  return (
    <Card title={t('Charge control')}>
      <div className="space-y-3">
        {status === 'stale' && (
          <InlineStatus kind="stale" action={{ label: t('Retry'), onClick: refresh }}>
            {t('Showing the last reading; refresh failed: {error}', { error: error ?? '' })}
          </InlineStatus>
        )}
        {cc.last_error && <InlineStatus kind="error">{cc.last_error}</InlineStatus>}
        {failure && <InlineStatus kind="error">{t('Change not applied: {failure}', { failure })}</InlineStatus>}
        <div className="flex items-center justify-between gap-2">
          <div className="min-w-0">
            <p className="text-body font-medium text-ink">{t('Charging')}</p>
            <p className="truncate text-meta text-ink2">{chargingSummary}</p>
          </div>
          <Button
            size="sm"
            variant={cc.charging_stopped ? 'primary' : 'outline'}
            loading={busy}
            disabled={!cc.charger_available || cc.charging_stopped == null}
            onClick={() => apply({ charging_stopped: !cc.charging_stopped })}
          >
            {cc.charging_stopped ? t('Resume charging') : t('Stop charging')}
          </Button>
        </div>

        <div className="border-t border-line/8 pt-3">
          <div className="flex items-center justify-between">
            <div>
              <p id="charge-limit-enforcer" className="text-body font-medium text-ink">{t('Charge limit')}</p>
              <p className="text-meta text-ink2">
                {t('Stop at limit, resume {n}% below', { n: cc.hysteresis })}
              </p>
            </div>
            <Toggle
              checked={cc.charge_limit_enabled}
              disabled={busy || !cc.battery_available}
              onChange={(v) => apply({ charge_limit_enabled: v })}
              labelledBy="charge-limit-enforcer"
            />
          </div>
          <div className="mt-3">
            <Field label={t('Stop charging at')} hint={pending != null ? t('Not applied yet. The device limit is {limit}%.', { limit: cc.charge_limit }) : undefined}>
              {(ids) => (
                <div className="flex items-center gap-3">
                  <input
                    id={ids.id}
                    aria-describedby={ids.describedBy}
                    type="range"
                    min={LIMIT_MIN}
                    max={LIMIT_MAX}
                    step={LIMIT_STEP}
                    value={shown}
                    aria-valuetext={`${shown}%`}
                    disabled={busy || !limitEditable}
                    onChange={(e) => setDraft(Number(e.target.value))}
                    onKeyDown={(e) => {
                      if (e.key === 'Escape' && draft != null) {
                        e.preventDefault()
                        setDraft(null)
                      }
                    }}
                    className="w-full accent-[rgb(var(--accent))] disabled:opacity-40"
                  />
                  <span className="tnum w-12 text-right font-mono text-body font-semibold text-ink" aria-hidden="true">
                    {shown}%
                  </span>
                </div>
              )}
            </Field>
            {draft != null && (
              <div className="mt-2 flex justify-end gap-2">
                <Button size="sm" variant="ghost" disabled={busy} onClick={() => setDraft(null)}>
                  {t('Cancel')}
                </Button>
                <Button
                  size="sm"
                  variant="primary"
                  loading={busy}
                  disabled={pending == null || !limitEditable}
                  onClick={() => pending != null && apply({ charge_limit: pending })}
                >
                  {t('Apply limit')}
                </Button>
              </div>
            )}
          </div>
        </div>

        <p className="text-caption leading-snug text-ink3">
          {t('Charging resumes automatically when the charger is unplugged or the limit is turned off.')}
        </p>
        {!cc.available && <p className="text-meta font-medium text-warn">{t('Battery and charger hardware data are unavailable; controls are disabled.')}</p>}
      </div>
    </Card>
  )
}

// ── Tab ───────────────────────────────────────────────────────────────────────

export default function MetricsTab() {
  const { data } = usePoll<MetricsData>(
    'metrics',
    async () => {
      const [t, b, bi, c, m] = await Promise.allSettled([
        api.thermalAll(),
        api.batteryDetail(),
        api.batteryInfoUbus(),
        api.cpu(),
        api.memory(),
      ])
      return {
        thermal: t.status === 'fulfilled' ? t.value : null,
        battery: b.status === 'fulfilled' ? b.value : null,
        batteryInfo: bi.status === 'fulfilled' ? bi.value : null,
        cpu: c.status === 'fulfilled' ? c.value : null,
        mem: m.status === 'fulfilled' ? m.value : null,
      }
    },
    5000,
  )

  if (!data) {
    return (
      <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
        <Skeleton className="h-64" />
        <Skeleton className="h-64" />
        <Skeleton className="h-48" />
        <Skeleton className="h-48" />
      </div>
    )
  }

  const { thermal, battery, batteryInfo, cpu, mem } = data

  const cpuAvg =
    thermal?.available && thermal.cpu_0 != null
      ? [thermal.cpu_0, thermal.cpu_1, thermal.cpu_2, thermal.cpu_3]
          .filter((v): v is number => v != null)
          .reduce((a, b) => a + b, 0) /
        [thermal.cpu_0, thermal.cpu_1, thermal.cpu_2, thermal.cpu_3].filter((v) => v != null).length
      : undefined

  const sensor = thermal?.available ? thermal : null
  const sensors: { label: string; value?: number | null }[] = [
    { label: t('CPU (avg)'), value: cpuAvg },
    { label: t('Modem (Q6 DSP)'), value: sensor?.modem },
    { label: t('Modem SS'), value: sensor?.modem_ss0 },
    { label: t('PA (power amplifier)'), value: sensor?.pa },
    { label: t('SDR (radio)'), value: sensor?.sdr },
    { label: t('Battery'), value: sensor?.battery },
    { label: 'USB', value: sensor?.usb },
    { label: t('Ethernet PHY'), value: sensor?.eth_phy },
    { label: 'PMIC', value: sensor?.pmic },
    { label: t('Board (XO)'), value: sensor?.xo_therm },
  ]

  const batteryHealth =
    battery?.available && battery.charge_full_design_mah != null && battery.charge_full_design_mah > 0 && battery.charge_full_mah != null
      ? Math.round((battery.charge_full_mah / battery.charge_full_design_mah) * 100)
      : undefined

  return (
    <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
      <Card title={t('Temperatures')}>
        {sensor ? (
          <div className="space-y-2.5">
            {sensors.map((s) => (
              <ThermalBar key={s.label} label={s.label} value={s.value} />
            ))}
          </div>
        ) : (
          <div className="space-y-2.5">
            <p className="text-meta font-medium text-warn">{t('Thermal sensors are unavailable.')}</p>
            {sensors.map((s) => (
              <ThermalBar key={s.label} label={s.label} />
            ))}
          </div>
        )}
      </Card>

      <div className="space-y-3">
        <Card title={t('CPU usage')}>
          {cpu ? (
            <div className="space-y-3">
              <div>
                <div className="mb-1 flex justify-between text-body">
                  <span className="font-medium text-ink">{t('Overall')}</span>
                  <span className="tnum font-mono text-ink2">{cpu.overall.toFixed(1)}%</span>
                </div>
                <Meter pct={cpu.overall} />
              </div>
              {cpu.cores.length > 0 && (
                <div className="space-y-1.5 border-t border-line/8 pt-2.5">
                  {cpu.cores.map((pct, i) => (
                    <div key={i}>
                      <div className="mb-0.5 flex justify-between text-caption">
                        <span className="text-ink3">{t('Core {n}', { n: i })}</span>
                        <span className="tnum font-mono text-ink2">{pct.toFixed(1)}%</span>
                      </div>
                      <Meter pct={pct} />
                    </div>
                  ))}
                </div>
              )}
            </div>
          ) : (
            <p className="text-body text-ink3">{t('No CPU data')}</p>
          )}
        </Card>

        <Card title={t('Memory')}>
          {mem ? (
            <div>
              <div className="mb-1 flex justify-between text-body">
                <span className="text-ink2">{t('Usage')}</span>
                <span className="tnum font-mono text-ink">
                  {formatBytes(mem.used_kb * 1024)} / {formatBytes(mem.total_kb * 1024)} ({mem.usage_pct.toFixed(0)}%)
                </span>
              </div>
              <Meter pct={mem.usage_pct} tone="bg-warn" />
            </div>
          ) : (
            <p className="text-body text-ink3">{t('No memory data')}</p>
          )}
        </Card>
      </div>

      <Card title={t('Battery')}>
        {battery?.available ? (
          <div className="space-y-3">
            <div>
              <div className="mb-1 flex justify-between text-body">
                <span className="tnum font-mono font-semibold text-ink">{battery.capacity != null ? `${battery.capacity}%` : t('Unavailable')}</span>
                <span className="text-ink2">{battery.status != null ? batteryStatusLabel(battery.status) : t('Unavailable')}</span>
              </div>
              {battery.capacity != null && <Meter pct={battery.capacity} tone={battery.capacity > 20 ? 'bg-ok' : 'bg-danger'} />}
            </div>
            <div className="grid grid-cols-2 gap-x-4 gap-y-2 text-body">
              <Info label={t('Power')} value={formatMeasure(battery.power_mw, (value) => `${(value / 1000).toFixed(2)} W`)} />
              <Info label={t('Voltage')} value={formatMeasure(battery.voltage_mv, (value) => `${(value / 1000).toFixed(3)} V`)} />
              <Info label={t('Current')} value={formatMeasure(battery.current_ma, (value) => `${value} mA`)} />
              <Info label={t('Charge type')} value={battery.charge_type ?? t('Unavailable')} />
              <Info label={t('Temperature')} value={formatMeasure(battery.temperature_c, (value) => `${value.toFixed(1)}°C`)} cls={battery.temperature_c != null ? tempColorClass(battery.temperature_c) : 'text-ink3'} />
              <Info
                label={battery.status === 'Charging' ? t('Time to full') : t('Time to empty')}
                value={formatClock(battery.status === 'Charging' ? battery.time_to_full_secs : battery.time_to_empty_secs)}
              />
              <Info label={t('Charge counter')} value={formatMeasure(battery.charge_counter_mah, (value) => `${value.toLocaleString()} mAh`)} />
              <Info label={t('Cycles')} value={formatMeasure(battery.cycle_count, String)} />
              <Info label={t('Fuel gauge')} value={batteryInfo?.available && batteryInfo.using_hw_fg_chip != null ? (batteryInfo.using_hw_fg_chip ? t('Hardware') : t('Software')) : t('Unavailable')} />
              <Info label={t('Battery online')} value={batteryInfo?.available && batteryInfo.online != null ? (batteryInfo.online ? t('Yes') : t('No')) : t('Unavailable')} />
            </div>
          </div>
        ) : (
          <p className="text-body font-medium text-warn">{t('Battery hardware data are unavailable.')}</p>
        )}
      </Card>

      <div className="space-y-3">
        <Card title={t('Battery health')}>
          {battery?.available ? (
            <div className="space-y-1.5 text-body">
              <KV k={t('Health')} v={battery.health != null ? batteryHealthLabel(battery.health) : t('Unavailable')} />
              <KV k={t('Capacity')} v={battery.charge_full_mah != null && battery.charge_full_design_mah != null ? `${battery.charge_full_mah.toLocaleString()} / ${battery.charge_full_design_mah.toLocaleString()} mAh` : t('Unavailable')} />
              {batteryHealth != null && (
                <div className="flex justify-between">
                  <span className="text-ink2">{t('Capacity retention')}</span>
                  <span className={`tnum font-mono font-semibold ${batteryHealth > 80 ? 'text-ok' : 'text-warn'}`}>{batteryHealth}%</span>
                </div>
              )}
              <KV k={t('OCV')} v={formatMeasure(battery.voltage_ocv_mv, (value) => `${(value / 1000).toFixed(3)} V`)} />
            </div>
          ) : (
          <p className="text-body font-medium text-warn">{t('Battery health measures are unavailable.')}</p>
          )}
        </Card>

        <ChargeControlCard />
      </div>
    </div>
  )
}

function Info({ label, value, cls = 'text-ink' }: { label: string; value: string; cls?: string }) {
  return (
    <div className="min-w-0">
      <p className="label">{label}</p>
      <p className={`tnum font-mono truncate font-medium ${cls}`}>{value}</p>
    </div>
  )
}

function KV({ k, v }: { k: string; v: string }) {
  return (
    <div className="flex justify-between">
      <span className="text-ink2">{k}</span>
      <span className="tnum font-mono font-medium text-ink">{v}</span>
    </div>
  )
}

function formatMeasure(value: number | null, format: (value: number) => string) {
  return value == null ? t('Unavailable') : format(value)
}

function formatClock(secs: number | null) {
  if (secs == null || secs <= 0) return t('Unavailable')
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  return h > 0 ? `${h}h ${m}m` : `${m}m`
}

// Kernel power_supply values, shown translated. Anything else is shown as the firmware reported it.
function batteryStatusLabel(status: string) {
  switch (status) {
    case 'Charging':
      return t('Charging')
    case 'Discharging':
      return t('Discharging')
    case 'Full':
      return t('Full')
    case 'Not charging':
      return t('Not charging')
    default:
      return status
  }
}

function batteryHealthLabel(health: string) {
  switch (health) {
    case 'Good':
      return t('Good')
    case 'Overheat':
      return t('Overheat')
    case 'Cold':
      return t('Cold')
    case 'Dead':
      return t('Dead')
    case 'Over voltage':
      return t('Over voltage')
    default:
      return health
  }
}
