import { useHome } from '../../app/HomeContext'
import { formatBandwidthMHz, formatBytes, formatSpeed, formatUptime, modemMode } from '../../format'
import { t } from '../../i18n'
import { IBolt, IDownload, IUpload } from '../../icons'
import { Card, Chip, Meter, Row, SignalBars, Skeleton, Unavailable } from '../../ui/primitives'
import { MetricValue } from '../signal/MetricValue'
import { Tip } from '../signal/Tip'
import {
  METRIC_HELP,
  METRIC_LABEL,
  bandwidthSummary,
  barsText,
  carrierCounts,
  homeUsageRows,
  metricView,
  servingView,
} from '../signal/telemetryView'
import type { BatteryInfo } from '../../types'

function PageSkeleton() {
  return (
    <div className="space-y-4">
      <Skeleton className="h-8 w-48" />
      <Skeleton className="h-64 xl:h-44" />
      <div className="grid grid-cols-1 gap-3 lg:grid-cols-3">
        <Skeleton className="h-48" />
        <Skeleton className="h-48" />
        <Skeleton className="h-48" />
      </div>
    </div>
  )
}

export default function HomePage() {
  const { data, error } = useHome()

  if (!data && !error) return <PageSkeleton />

  const signal = data?.signal ?? null
  const battery = data?.battery ?? null
  const speed = data?.speed ?? null
  const device = data?.device ?? null
  const wan = data?.wan ?? null
  const wan6 = data?.wan6 ?? null
  const cpu = data?.cpu ?? null
  const mem = data?.memory ?? null
  const usage = data?.usage ?? null

  // Mapper-validated serving carrier (SA -> NR PCC, LTE/NSA -> LTE anchor PCC); never a raw field.
  const serving = servingView(signal)
  const primary = serving.carrier
  const rsrp = metricView('rsrp', primary?.rsrp)
  const mode = modemMode(signal?.type)
  const lte = signal ? carrierCounts(signal.lte_carriers) : null
  const nr = signal ? carrierCounts(signal.nr_carriers) : null
  const lteBw = bandwidthSummary(signal?.lte_carriers ?? []).reportedMHz
  const nrBw = bandwidthSummary(signal?.nr_carriers ?? []).reportedMHz
  const totalBw = lteBw + nrBw
  const reported = (lte?.reported ?? 0) + (nr?.reported ?? 0)
  const active = (lte?.active ?? 0) + (nr?.active ?? 0)
  const idle = (lte?.idle ?? 0) + (nr?.idle ?? 0)
  const bars = barsText(signal?.signal_bars)

  return (
    <div className="space-y-4">
      <div>
        <h1 className="hidden font-display text-2xl font-semibold tracking-[-0.015em] text-ink lg:block">{t('Overview')}</h1>
        <p className="lg:mt-0.5 text-body text-ink2">{signal?.carrier ?? t('Mobile broadband status')}</p>
      </div>

      {error && !data && (
        <Card>
          <p className="text-body text-danger">{error}</p>
        </Card>
      )}

      {/* Readout band — the page's one graphite beat. Hairline grid via gap-px on a line-tinted ground. */}
      <section className="band overflow-hidden rounded-panel border border-line/10" aria-label={t('Live readouts')}>
        <div className="grid grid-cols-6 gap-px bg-line/10 xl:grid-cols-5">
          <div className="col-span-6 bg-band p-4 xl:col-span-2">
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0">
                <p className="label">{t('Signal · RSRP')}</p>
                <div className="mt-2 flex items-baseline gap-2">
                  <span
                    data-testid="home-rsrp"
                    data-level={rsrp.level}
                    className={`tnum font-mono text-5xl font-medium leading-none tracking-[-0.03em] ${rsrp.className}`}
                  >
                    {rsrp.text ?? <Unavailable label={t('RSRP unavailable')} />}
                  </span>
                  {rsrp.text !== null && <span className="font-display text-sm font-medium text-ink2">dBm</span>}
                </div>
                <p data-testid="home-rsrp-word" className={`mt-1.5 text-body font-semibold ${rsrp.className}`}>
                  {rsrp.text === null ? t('No serving measurement') : rsrp.word}
                </p>
              </div>
              {bars === null ? (
                <p className="text-caption text-ink3">
                  {t('Bars')} <Unavailable label={t('Signal bars unavailable')} />
                </p>
              ) : (
                <SignalBars bars={signal?.signal_bars} large />
              )}
            </div>
            <div className="tnum mt-4 flex flex-wrap items-center gap-x-3 gap-y-1.5 font-mono text-meta text-ink2">
              {serving.available && primary ? (
                <Chip tone={signal?.primary?.rat === 'lte' ? 'lte' : 'nr'}>{serving.label}</Chip>
              ) : (
                <span className="text-ink3">
                  {t('Serving cell')} <Unavailable label={t('No serving carrier reported')} />
                </span>
              )}
              {serving.pci !== undefined && <span>PCI {serving.pci}</span>}
              <span>{bars ?? t('Bars unavailable')}</span>
            </div>
            <dl className="mt-3 grid grid-cols-3 gap-x-3 gap-y-1.5 border-t border-line/10 pt-3 font-mono text-meta">
              {(['rsrq', 'sinr', 'rssi'] as const).map((m) => (
                <div key={m} className="min-w-0">
                  <dt className="label">
                    <Tip text={METRIC_HELP[m]} className="label">
                      {METRIC_LABEL[m]}
                    </Tip>
                  </dt>
                  <dd className="tnum mt-0.5 font-medium">
                    <MetricValue metric={m} value={primary?.[m]} />
                  </dd>
                </div>
              ))}
            </dl>
          </div>

          <div className="col-span-6 bg-band p-4 sm:col-span-2 xl:col-span-1">
            <p className="label">{t('Throughput')}</p>
            <div className="tnum mt-2 flex gap-x-6 font-mono font-medium text-ink sm:block sm:space-y-1.5">
              <p className="flex items-center gap-1.5 text-base leading-none">
                <IDownload size={14} className="shrink-0 text-ok" />
                <span className="truncate">{speed ? formatSpeed(speed.rx_bps) : '\u2014'}</span>
              </p>
              <p className="flex items-center gap-1.5 text-base leading-none">
                <IUpload size={14} className="shrink-0 text-accent" />
                <span className="truncate">{speed ? formatSpeed(speed.tx_bps) : '\u2014'}</span>
              </p>
            </div>
            <p className="tnum mt-3 truncate font-mono text-caption text-ink3">
              {t('Peak down {speed}', { speed: speed && speed.max_rx_bps > 0 ? formatSpeed(speed.max_rx_bps) : '\u2014' })}
            </p>
          </div>

          <div className="col-span-3 bg-band p-4 sm:col-span-2 xl:col-span-1">
            <p className="label">{t('Mode')}</p>
            <p className="tnum mt-2 font-mono text-2xl font-medium leading-none text-ink">{mode}</p>
            <p className="mt-1.5 text-meta text-ink2">
              {reported === 1 ? t('{n} carrier reported', { n: reported }) : t('{n} carriers reported', { n: reported })}
            </p>
            {reported > 0 && (
              <p className="text-caption text-ink3">
                {idle > 0 ? t('{active} active · {idle} idle', { active, idle }) : t('{active} active', { active })}
              </p>
            )}
            <div className="mt-3 flex flex-wrap gap-1">
              {nrBw > 0 && <Chip tone="nr">NR {formatBandwidthMHz(nrBw)}</Chip>}
              {lteBw > 0 && <Chip tone="lte">LTE {formatBandwidthMHz(lteBw)}</Chip>}
              {totalBw <= 0 && <span className="text-caption text-ink3">{t('No bandwidth reported')}</span>}
            </div>
            {totalBw > 0 && <p className="mt-1 text-caption text-ink3">{t('Sum of reported carriers')}</p>}
          </div>

          <div className="col-span-3 bg-band p-4 sm:col-span-2 xl:col-span-1">
            <p className="label flex items-center gap-1">
              {t('Battery')} {battery?.charging && <IBolt size={12} className="text-warn" />}
            </p>
            <p className="tnum mt-2 font-mono text-2xl font-medium leading-none text-ink">
              {battery?.percent != null ? `${battery.percent}%` : '\u2014'}
            </p>
            <p className="mt-1.5 text-meta text-ink2">{batteryState(battery)}</p>
            <p className="tnum mt-3 truncate font-mono text-caption text-ink3">
              {battery?.voltage_mv ? `${(battery.voltage_mv / 1000).toFixed(2)} V` : '\u2014'}
              {battery?.temperature_c != null ? ` · ${battery.temperature_c.toFixed(1)}°C` : ''}
            </p>
          </div>
        </div>
      </section>

      {/* Radio details */}
      {signal && (signal.lte_carriers.length > 0 || signal.nr_carriers.length > 0) && (
        <Card title={t('Reported carriers')}>
          <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
            <div>
              <p className="label mb-1.5 text-accent">LTE</p>
              {signal.lte_carriers.length > 0 ? (
                <div className="flex flex-wrap gap-1.5">
                  {signal.lte_carriers.map((c, i) => (
                    <Chip key={i} tone="lte">
                      {c.label} · {c.band}
                      {c.rsrp != null ? ` · ${c.rsrp} dBm` : ''}
                      {c.active === false ? ` · ${t('idle')}` : ''}
                    </Chip>
                  ))}
                </div>
              ) : (
                <p className="text-body text-ink3">{t('No LTE carrier reported')}</p>
              )}
            </div>
            <div>
              <p className="label mb-1.5 text-nr">
                5G NR
              </p>
              {signal.nr_carriers.length > 0 ? (
                <div className="flex flex-wrap gap-1.5">
                  {signal.nr_carriers.map((c, i) => (
                    <Chip key={i} tone="nr">
                      {c.label} · {c.band}
                      {c.rsrp != null ? ` · ${c.rsrp} dBm` : ''}
                      {c.active === false ? ` · ${t('idle')}` : ''}
                    </Chip>
                  ))}
                </div>
              ) : (
                <p className="text-body text-ink3">{t('No NR carrier reported')}</p>
              )}
            </div>
          </div>
        </Card>
      )}

      {/* Details row */}
      <div className="grid grid-cols-1 gap-3 lg:grid-cols-3">
        <Card title={t('Connection')}>
          <Row label={t('Operator')} value={signal?.carrier ?? '\u2014'} />
          <Row label="IPv4" value={wan?.ipv4 ?? '\u2014'} mono />
          <Row label={t('Gateway')} value={wan?.gateway ?? '\u2014'} mono />
          <Row label="IPv6" value={wan6?.ipv6 ?? '\u2014'} mono wrap />
          {wan6?.prefix && <Row label={t('IPv6 prefix')} value={wan6.prefix} mono wrap />}
          {wan?.dns && wan.dns.length > 0 && (
            <Row label="DNS" value={wan.dns.filter((d) => !d.includes(':')).join(', ') || '\u2014'} mono wrap />
          )}
        </Card>

        <Card title={t('Device')}>
          <Row label={t('Model')} value={device?.model ?? '\u2014'} />
          <Row label={t('Firmware')} value={device?.firmware ?? '\u2014'} />
          <Row label={t('Uptime')} value={formatUptime(device?.uptime_secs)} />
          <div className="mt-2 space-y-2 border-t border-line/8 pt-2.5">
            <div>
              <div className="mb-1 flex justify-between text-caption">
                <span className="font-medium text-ink2">CPU</span>
                <span className="tnum font-mono text-ink2">{cpu ? `${cpu.overall.toFixed(0)}%` : '\u2014'}</span>
              </div>
              <Meter pct={cpu?.overall ?? 0} />
            </div>
            <div>
              <div className="mb-1 flex justify-between text-caption">
                <span className="font-medium text-ink2">{t('Memory')}</span>
                <span className="tnum font-mono text-ink2">{mem ? `${mem.usage_pct.toFixed(0)}%` : '\u2014'}</span>
              </div>
              <Meter pct={mem?.usage_pct ?? 0} tone="bg-warn" />
            </div>
          </div>
        </Card>

        <Card title={t('Data usage')}>
          {usage ? (
            <div className="space-y-2.5">
              {homeUsageRows(usage).map(({ label, rx, tx, total }) => (
                <div key={label} data-usage={label}>
                  <p className="label">{label}</p>
                  <div className="tnum font-mono mt-0.5 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-body font-medium">
                    <span className="flex items-center gap-1 text-ok">
                      <IDownload size={12} /> <Bytes value={rx} />
                    </span>
                    <span className="flex items-center gap-1 text-accent">
                      <IUpload size={12} /> <Bytes value={tx} />
                    </span>
                    <span className="text-ink2">
                      <Bytes value={total} /> <span className="font-sans text-meta font-normal text-ink3">{t('total')}</span>
                    </span>
                  </div>
                </div>
              ))}
            </div>
          ) : (
            <p className="text-body text-ink3">{t('Not available')}</p>
          )}
        </Card>
      </div>
    </div>
  )
}

/** Measured bytes (0 stays "0 B"); unknown is an em dash with an Unavailable label. */
function Bytes({ value }: { value: number | null }) {
  return value === null ? <Unavailable /> : <>{formatBytes(value)}</>
}

function batteryState(battery: BatteryInfo | null | undefined): string {
  if (!battery) return '\u2014'
  if (battery.charging) return t('Charging')
  if (battery.status === 'Full') return t('Full')
  return battery.plugged ? t('Plugged in, not charging') : t('On battery')
}
