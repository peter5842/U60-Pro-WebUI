import { useState } from 'react'
import { api } from '../../data/api'
import type { PollResult } from '../../data/poll'
import { formatBytes, formatSpeed, formatUptime } from '../../format'
import type { ProxyMode, ProxyStatus } from '../../types'
import { Button, Segmented } from '../../ui/controls'
import { confirm, toastError } from '../../ui/feedback'
import { Card, Chip, InlineStatus, Skeleton, Stat } from '../../ui/primitives'
import CopyField from './CopyField'
import { currentRoute, MODE_HELP, MODE_OPTIONS, serviceState } from './proxyView'
import { t } from '../../i18n'

type Busy = 'start' | 'stop' | 'restart' | 'mode' | null

export default function OverviewTab({ status }: { status: PollResult<ProxyStatus> }) {
  const [busy, setBusy] = useState<Busy>(null)
  const [modeDraft, setModeDraft] = useState<ProxyMode | null>(null)
  const s = status.data

  async function service(action: 'start' | 'stop' | 'restart') {
    if (busy) return
    if (action === 'stop' && s?.tun) {
      const ok = await confirm({
        title: t('Stop the proxy?'),
        body: t('Transparent proxy (TUN) is on. Devices go back to the normal route.'),
        kind: 'connection',
        consequence: t('Connections through mihomo drop.'),
        recovery: t('Start it again here.'),
      })
      if (!ok) return
    }
    setBusy(action)
    try {
      status.mutate(await api.proxyService(action))
    } catch (e) {
      toastError(e, { start: t('Failed to start the proxy'), stop: t('Failed to stop the proxy'), restart: t('Failed to restart the proxy') }[action])
      status.refresh()
    } finally {
      setBusy(null)
    }
  }

  async function applyMode() {
    if (!modeDraft || busy) return
    setBusy('mode')
    try {
      status.mutate(await api.proxySettings({ mode: modeDraft }))
      setModeDraft(null)
    } catch (e) {
      toastError(e, t('Failed to change the mode'))
    } finally {
      setBusy(null)
    }
  }

  if (status.status === 'loading') return <Skeleton className="h-40" />
  if (!s) {
    return (
      <InlineStatus kind="error" action={{ label: 'Retry', onClick: status.refresh, loading: status.refreshing }}>
        {status.error ? t('Proxy status could not be read: {error}', { error: status.error }) : t('Proxy status could not be read.')}
      </InlineStatus>
    )
  }

  const state = serviceState(s)
  const traffic = s.traffic
  const mode = modeDraft ?? s.mode ?? 'rule'

  return (
    <>
      {status.status === 'stale' && (
        <InlineStatus kind="stale" action={{ label: 'Retry', onClick: status.refresh, loading: status.refreshing }}>
          {t('Showing the last status read. The latest refresh failed.')}
        </InlineStatus>
      )}
      {!s.installed && (
        <InlineStatus kind="error">
          {t('mihomo is not installed on the router. Install it from a computer with')}{' '}
          <code className="font-mono text-meta">scripts/deploy-mihomo.sh</code>
        </InlineStatus>
      )}
      {s.notice && <InlineStatus kind="warn">{s.notice}</InlineStatus>}
      {s.last_error && !s.running && <InlineStatus kind="error">{s.last_error}</InlineStatus>}

      <Card
        title={t('Service')}
        action={
          <div className="flex gap-2">
            {s.running ? (
              <>
                <Button size="sm" variant="ghost" onClick={() => void service('restart')} loading={busy === 'restart'} disabled={!!busy}>
                  {t('Restart')}
                </Button>
                <Button size="sm" variant="outline" onClick={() => void service('stop')} loading={busy === 'stop'} disabled={!!busy}>
                  {t('Stop')}
                </Button>
              </>
            ) : (
              <Button
                size="sm"
                variant="primary"
                onClick={() => void service('start')}
                loading={busy === 'start'}
                disabled={!!busy || !s.installed}
              >
                {t('Start')}
              </Button>
            )}
          </div>
        }
      >
        <div className="grid grid-cols-2 gap-x-4 gap-y-4 sm:grid-cols-4">
          <div className="min-w-0">
            <p className="label">{t('Status')}</p>
            <div className="mt-1.5 flex flex-wrap gap-1.5">
              <Chip tone={state.tone}>{state.label}</Chip>
              {s.tun_active && <Chip tone="accent">TUN</Chip>}
            </div>
          </div>
          <div className="col-span-2 min-w-0 sm:col-span-1">
            <p className="label">{t('Route')}</p>
            <p className="mt-1 break-words text-body font-semibold text-ink">{currentRoute(s) ?? '—'}</p>
            {s.mode && <p className="mt-0.5 text-meta text-ink3">{MODE_OPTIONS.find((o) => o.value === s.mode)?.label}</p>}
          </div>
          <Stat label={t('Download')} value={traffic?.down_rate !== undefined ? formatSpeed(traffic.down_rate) : '—'} sub={formatBytes(traffic?.down_total)} />
          <Stat label={t('Upload')} value={traffic?.up_rate !== undefined ? formatSpeed(traffic.up_rate) : '—'} sub={formatBytes(traffic?.up_total)} />
          <Stat label={t('Connections')} value={traffic?.connections ?? '—'} />
          <Stat label={t('Memory')} value={formatBytes(s.rss_bytes)} />
          <Stat label={t('Uptime')} value={s.running ? formatUptime(s.uptime_secs) : '—'} sub={s.restarts ? t('{n} auto-restarts', { n: s.restarts }) : undefined} />
          <Stat label={t('Core')} value={s.version ?? '—'} sub="mihomo" />
        </div>
      </Card>

      <Card title={t('Mode')}>
        <div className="space-y-3">
          <div className="flex flex-wrap items-center gap-2">
            <Segmented<ProxyMode>
              label={t('Proxy mode')}
              options={MODE_OPTIONS}
              value={mode}
              onChange={setModeDraft}
              disabled={!!busy}
            />
            {modeDraft && modeDraft !== s.mode && (
              <Button variant="primary" onClick={() => void applyMode()} loading={busy === 'mode'}>
                {t('Apply')}
              </Button>
            )}
          </div>
          <p className="text-meta text-ink2">{MODE_HELP[mode]}</p>
        </div>
      </Card>

      <Card title={t('Connect devices')}>
        {s.tun ? (
          <p className="text-body text-ink2">
            {s.tun_active
              ? t('Transparent proxy (TUN) is on: every device on this router’s Wi-Fi or USB is routed through mihomo automatically. No device setup is needed.')
              : t('Transparent proxy (TUN) is enabled but not active yet. Once mihomo runs, every device on this router’s Wi-Fi or USB is routed through it automatically.')}
          </p>
        ) : (
          <div className="space-y-4">
            <p className="text-meta text-ink2">
              {t('Point a device at the proxy, or turn on transparent proxy in Settings to cover every device.')}
            </p>
            {s.pac_url && (
              <CopyField
                label={t('Automatic proxy (PAC) URL')}
                value={s.pac_url}
                hint={t('Recommended: if mihomo stops, devices fall back to a direct connection.')}
              />
            )}
            {s.proxy_address && <CopyField label={t('Manual proxy (HTTP / SOCKS5)')} value={s.proxy_address} />}
            <ul className="list-disc space-y-1 pl-5 text-meta text-ink2">
              <li>{t('iPhone / iPad: Settings → Wi-Fi → ⓘ → Configure Proxy → Automatic → URL')}</li>
              <li>{t('macOS: System Settings → Network → Wi-Fi → Details → Proxies → Automatic proxy configuration')}</li>
              <li>{t('Windows: Settings → Network & internet → Proxy → Use setup script')}</li>
              <li>{t('Android: Wi-Fi → network → Edit → Proxy → Proxy Auto-Config')}</li>
            </ul>
          </div>
        )}
      </Card>
    </>
  )
}
