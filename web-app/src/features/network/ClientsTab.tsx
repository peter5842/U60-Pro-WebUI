import { Fragment, type ReactNode } from 'react'
import { api } from '../../data/api'
import { usePoll } from '../../data/poll'
import { t } from '../../i18n'
import type { Client } from '../../types'
import { ICable, ILaptop, IRefresh, IUsb, IWifi } from '../../icons'
import { Button } from '../../ui/controls'
import { Card, Chip, Empty, InlineStatus, Loading, Skeleton, Unavailable } from '../../ui/primitives'
import {
  clientFields,
  formatBitrate,
  formatLinkMbps,
  formatWifiLink,
  groupClients,
  type GroupedClients,
} from './clientsView'

const TH_CLS = 'pb-1.5 pr-4 font-semibold'
const TD_CLS = 'py-2 pr-4'
const MONO_TD = `${TD_CLS} tnum font-mono text-meta text-ink2`

const Value = ({ v }: { v: string | undefined }) => (v ? <>{v}</> : <Unavailable />)
const hostname = (c: Client) => c.hostname || <Unavailable label={t('No hostname')} />

/** Below `sm`: one stacked row per client with every field the desktop table has (U07). */
function StackedClients({
  group,
  items,
  badge,
}: {
  group: keyof GroupedClients
  items: Client[]
  badge?: (c: Client) => ReactNode
}) {
  return (
    <ul className="divide-y divide-line/6 px-4 sm:hidden">
      {items.map((c) => (
        <li key={c.mac} className="py-2.5">
          <div className="flex items-start justify-between gap-2">
            <span className="min-w-0 break-words text-body font-medium text-ink">{hostname(c)}</span>
            {badge && <span className="shrink-0">{badge(c)}</span>}
          </div>
          <dl className="mt-1 grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-0.5 text-meta">
            {clientFields(group, c).map((f) => (
              <Fragment key={f.label}>
                <dt className="text-ink3">{f.label}</dt>
                <dd className={`min-w-0 break-all ${f.mono ? 'tnum font-mono' : ''} text-ink2`}>
                  <Value v={f.value} />
                </dd>
              </Fragment>
            ))}
          </dl>
        </li>
      ))}
    </ul>
  )
}

const bandChip = (c: Client) => <Chip tone={c.wifi_band === '5 GHz' ? 'accent' : 'ok'}>{c.wifi_band ?? 'Wi-Fi'}</Chip>

export default function ClientsTab() {
  // Clients is an expensive endpoint (iw station dump + bridge fdb + arp): poll slowly and offer a
  // manual refresh. The USB link comes from its own source so one failing does not hide the other.
  const clientsPoll = usePoll('network:clients', api.clients, 15000)
  const usbPoll = usePoll('network:usb-link', api.usbStatus, 15000)

  const refreshAll = () => {
    clientsPoll.refresh()
    usbPoll.refresh()
  }

  if (!clientsPoll.data) {
    if (clientsPoll.status === 'error') {
      return (
        <InlineStatus
          kind="error"
          action={{ label: t('Retry'), onClick: refreshAll, loading: clientsPoll.refreshing }}
        >
          {t('Could not read the connected clients: {error}', { error: clientsPoll.error ?? '' })}
        </InlineStatus>
      )
    }
    return (
      <Loading label={t('Loading clients')} className="space-y-3">
        <Skeleton className="h-20" />
        <Skeleton className="h-56" />
      </Loading>
    )
  }

  const clients = clientsPoll.data
  const grouped = groupClients(clients)
  const usbLink = usbPoll.data?.link
  const usbNegotiatedRate = formatBitrate(usbLink?.negotiated_mbps)
  const usbMaxRate = formatBitrate(usbLink?.max_mbps)
  const usbFailed = usbPoll.error != null
  const staleClients = clientsPoll.status === 'stale'

  return (
    <div className="space-y-3">
      <Card
        title={t('Connected clients ({n})', { n: clients.length })}
        action={
          <Button size="sm" variant="ghost" onClick={refreshAll} loading={clientsPoll.refreshing || usbPoll.refreshing}>
            <IRefresh size={13} /> {t('Refresh')}
          </Button>
        }
      >
        {staleClients && (
          <InlineStatus
            kind="stale"
            className="mb-3"
            action={{ label: t('Retry'), onClick: clientsPoll.refresh, loading: clientsPoll.refreshing }}
          >
            {t('Showing the last client list that loaded. The latest refresh failed: {error}', { error: clientsPoll.error ?? '' })}
          </InlineStatus>
        )}
        {clients.length === 0 ? (
          clientsPoll.status === 'ready' && <Empty icon={<ILaptop size={28} />} title={t('No clients connected')} />
        ) : (
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
            {[
              { label: 'Wi-Fi', count: grouped.wifi.length, icon: <IWifi size={15} /> },
              { label: 'USB-C', count: grouped.usb.length, icon: <IUsb size={15} /> },
              { label: t('Ethernet'), count: grouped.ethernet.length, icon: <ICable size={15} /> },
              { label: t('Other'), count: grouped.other.length, icon: <ILaptop size={15} /> },
            ].map((g) => (
              <div key={g.label} className="rounded-ctl bg-surface2/70 px-3 py-2.5">
                <div className="flex items-center gap-1.5 text-ink3">
                  {g.icon}
                  <p className="label">{g.label}</p>
                </div>
                <p className="tnum font-mono mt-1 text-2xl font-medium text-ink">{g.count}</p>
              </div>
            ))}
          </div>
        )}
      </Card>

      {grouped.wifi.length > 0 && (
        <Card title={t('Wi-Fi ({n})', { n: grouped.wifi.length })} pad={false}>
          <StackedClients group="wifi" items={grouped.wifi} badge={bandChip} />
          <div className="hidden overflow-x-auto px-4 pb-3 sm:block">
            <table className="w-full text-body">
              <thead>
                <tr className="label border-b border-line/8 text-left">
                  <th className={TH_CLS}>{t('Hostname')}</th>
                  <th className={TH_CLS}>IP</th>
                  <th className={TH_CLS}>{t('Radio')}</th>
                  <th className={TH_CLS}>{t('Signal')}</th>
                  <th className={TH_CLS}>{t('Link')}</th>
                  <th className="pb-1.5 font-semibold">MAC</th>
                </tr>
              </thead>
              <tbody>
                {grouped.wifi.map((c) => (
                  <tr key={c.mac} className="border-b border-line/6 last:border-0">
                    <td className={`${TD_CLS} font-medium text-ink`}>{hostname(c)}</td>
                    <td className={MONO_TD}>
                      <Value v={c.ip} />
                    </td>
                    <td className={TD_CLS}>{bandChip(c)}</td>
                    <td className={`${TD_CLS} tnum font-mono text-ink2`}>
                      <Value v={c.signal_dbm != null ? `${c.signal_dbm} dBm` : undefined} />
                    </td>
                    <td className={`${TD_CLS} tnum font-mono text-ink2`}>
                      <Value v={formatWifiLink(c)} />
                    </td>
                    <td className="tnum py-2 font-mono text-caption text-ink3">{c.mac}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>
      )}

      {(grouped.usb.length > 0 || usbLink || usbFailed) && (
        <Card title={t('USB-C ({n})', { n: grouped.usb.length })} pad={false}>
          <div className="pb-3">
            <div className="space-y-3 px-4">
              {usbFailed && (
                <InlineStatus
                  kind={usbPoll.data ? 'stale' : 'error'}
                  action={{ label: t('Retry'), onClick: usbPoll.refresh, loading: usbPoll.refreshing }}
                >
                  {usbPoll.data
                    ? t('Showing the last USB link details that loaded. The latest refresh failed: {error}', { error: usbPoll.error ?? '' })
                    : t('USB link details are unavailable: {error}', { error: usbPoll.error ?? '' })}
                </InlineStatus>
              )}
              {usbLink && (
                <div className="flex flex-wrap items-center gap-2 rounded-ctl bg-surface2/70 px-3 py-2">
                  <span className="label">{t('Tether link')}</span>
                  <span className="text-body font-bold text-ink">
                    {usbLink.negotiated_label ?? usbLink.negotiated ?? t('Unknown')}
                    {usbNegotiatedRate && <span className="font-medium text-ink2"> · {usbNegotiatedRate}</span>}
                  </span>
                  {usbLink.at_full_speed === false && usbMaxRate && (
                    <Chip tone="warn" wrap>
                      {usbLink.max_label
                        ? t('{label} capable · {rate} — cable/port limiting', { label: usbLink.max_label, rate: usbMaxRate })
                        : t('Higher capable · {rate} — cable/port limiting', { rate: usbMaxRate })}
                    </Chip>
                  )}
                  {usbLink.at_full_speed === true && <Chip tone="ok">{t('Full speed')}</Chip>}
                </div>
              )}
            </div>
            {grouped.usb.length > 0 ? (
              <>
                <StackedClients group="usb" items={grouped.usb} />
                <div className="hidden overflow-x-auto px-4 sm:block">
                  <table className="mt-3 w-full text-body">
                    <thead>
                      <tr className="label border-b border-line/8 text-left">
                        <th className={TH_CLS}>{t('Hostname')}</th>
                        <th className={TH_CLS}>IP</th>
                        <th className={TH_CLS}>{t('Interface')}</th>
                        <th className="pb-1.5 font-semibold">MAC</th>
                      </tr>
                    </thead>
                    <tbody>
                      {grouped.usb.map((c) => (
                        <tr key={c.mac} className="border-b border-line/6 last:border-0">
                          <td className={`${TD_CLS} font-medium text-ink`}>{hostname(c)}</td>
                          <td className={MONO_TD}>
                            <Value v={c.ip} />
                          </td>
                          <td className={MONO_TD}>
                            <Value v={c.interface} />
                          </td>
                          <td className="tnum py-2 font-mono text-caption text-ink3">{c.mac}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </>
            ) : (
              !staleClients && <p className="mt-3 px-4 text-body text-ink3">{t('No USB-C clients connected')}</p>
            )}
          </div>
        </Card>
      )}

      {grouped.ethernet.length > 0 && (
        <Card title={t('Ethernet ({n})', { n: grouped.ethernet.length })} pad={false}>
          <StackedClients group="ethernet" items={grouped.ethernet} />
          <div className="hidden overflow-x-auto px-4 pb-3 sm:block">
            <table className="w-full text-body">
              <thead>
                <tr className="label border-b border-line/8 text-left">
                  <th className={TH_CLS}>{t('Hostname')}</th>
                  <th className={TH_CLS}>IP</th>
                  <th className={TH_CLS}>{t('Speed')}</th>
                  <th className="pb-1.5 font-semibold">MAC</th>
                </tr>
              </thead>
              <tbody>
                {grouped.ethernet.map((c) => (
                  <tr key={c.mac} className="border-b border-line/6 last:border-0">
                    <td className={`${TD_CLS} font-medium text-ink`}>{hostname(c)}</td>
                    <td className={MONO_TD}>
                      <Value v={c.ip} />
                    </td>
                    <td className={`${TD_CLS} tnum font-mono text-ink2`}>
                      <Value v={formatLinkMbps(c.wired_link_mbps)} />
                    </td>
                    <td className="tnum py-2 font-mono text-caption text-ink3">{c.mac}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>
      )}

      {grouped.other.length > 0 && (
        <Card title={t('Other ({n})', { n: grouped.other.length })} pad={false}>
          <StackedClients group="other" items={grouped.other} />
          <div className="hidden overflow-x-auto px-4 pb-3 sm:block">
            <table className="w-full text-body">
              <thead>
                <tr className="label border-b border-line/8 text-left">
                  <th className={TH_CLS}>{t('Hostname')}</th>
                  <th className={TH_CLS}>IP</th>
                  <th className="pb-1.5 font-semibold">MAC</th>
                </tr>
              </thead>
              <tbody>
                {grouped.other.map((c) => (
                  <tr key={c.mac} className="border-b border-line/6 last:border-0">
                    <td className={`${TD_CLS} font-medium text-ink`}>{hostname(c)}</td>
                    <td className={MONO_TD}>
                      <Value v={c.ip} />
                    </td>
                    <td className="tnum py-2 font-mono text-caption text-ink3">{c.mac}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>
      )}
    </div>
  )
}
