import { useState } from 'react'
import { api } from '../../data/api'
import type { PollResult } from '../../data/poll'
import type { ProxyPreset, ProxyStatus } from '../../types'
import { Button, Field, Input, Segmented, Toggle } from '../../ui/controls'
import { confirm, toast, toastError } from '../../ui/feedback'
import { Card, Chip, InlineStatus, Row, Skeleton } from '../../ui/primitives'
import CopyField from './CopyField'
import { PRESET_OPTIONS, validatePort } from './proxyView'
import { t } from '../../i18n'

type Busy = 'preset' | 'tun' | 'bypass' | 'port' | null

export default function SettingsTab({ status }: { status: PollResult<ProxyStatus> }) {
  const [busy, setBusy] = useState<Busy>(null)
  const [presetDraft, setPresetDraft] = useState<ProxyPreset | null>(null)
  const [portDraft, setPortDraft] = useState<string | null>(null)
  const [portError, setPortError] = useState<string | undefined>()
  const s = status.data

  async function save(kind: Exclude<Busy, null>, body: Parameters<typeof api.proxySettings>[0], done: string) {
    setBusy(kind)
    try {
      status.mutate(await api.proxySettings(body))
      toast(done)
      return true
    } catch (e) {
      toastError(e, t('Failed to save the setting'))
      status.refresh()
      return false
    } finally {
      setBusy(null)
    }
  }

  async function setTun(on: boolean) {
    const ok = await confirm(
      on
        ? {
            title: t('Turn on transparent proxy?'),
            body: t('Every device on this router’s Wi-Fi and USB is routed through mihomo without any device setup. The router’s own traffic and its management pages are not affected.'),
            kind: 'connection',
            consequence: s?.running ? t('mihomo restarts; open connections drop for a few seconds.') : undefined,
            recovery: t('Turn it off here at any time. If forwarding fails, the router turns TUN off by itself; its own pages stay reachable.'),
          }
        : {
            title: t('Turn off transparent proxy?'),
            body: t('Devices go back to the normal route unless they use the proxy address or PAC URL.'),
            kind: 'connection',
            consequence: s?.running ? t('mihomo restarts; open connections drop for a few seconds.') : undefined,
          },
    )
    if (ok) await save('tun', { tun: on }, on ? t('Transparent proxy on') : t('Transparent proxy off'))
  }

  async function applyPort() {
    const parsed = validatePort(portDraft ?? '')
    if (!parsed.ok) {
      setPortError(parsed.error)
      return
    }
    setPortError(undefined)
    if (await save('port', { mixed_port: parsed.port }, t('Proxy port set to {port}', { port: parsed.port }))) setPortDraft(null)
  }

  if (status.status === 'loading') return <Skeleton className="h-40" />
  if (!s) {
    return (
      <InlineStatus kind="error" action={{ label: 'Retry', onClick: status.refresh, loading: status.refreshing }}>
        {status.error ? t('Proxy settings could not be read: {error}', { error: status.error }) : t('Proxy settings could not be read.')}
      </InlineStatus>
    )
  }

  const preset = presetDraft ?? s.preset ?? 'bypass_cn'
  const presetHelp = PRESET_OPTIONS.find((o) => o.value === preset)?.help

  return (
    <>
      <Card title={t('Routing')}>
        {s.profile ? (
          <InlineStatus kind="info" live={false}>
            {t('Rules and groups come from the subscription {name}. The presets below apply only to the managed rules.', { name: s.profile.name })}
          </InlineStatus>
        ) : null}
        <div className={`space-y-3 ${s.profile ? 'mt-3 opacity-60' : ''}`}>
          <Segmented<ProxyPreset>
            label={t('Routing preset')}
            options={PRESET_OPTIONS.map(({ value, label }) => ({ value, label }))}
            value={preset}
            onChange={setPresetDraft}
            disabled={!!busy || !!s.profile}
            wrap
          />
          <p className="text-meta text-ink2">{presetHelp} {t('Applies in Rule mode.')}</p>
          {presetDraft && presetDraft !== s.preset && (
            <Button
              variant="primary"
              onClick={() => void save('preset', { preset: presetDraft }, t('Routing updated')).then((ok) => ok && setPresetDraft(null))}
              loading={busy === 'preset'}
            >
              {t('Apply routing')}
            </Button>
          )}
        </div>
      </Card>

      <Card
        title={t('Transparent proxy (TUN)')}
        action={<Chip tone={s.tun_active ? 'ok' : 'default'}>{s.tun_active ? t('Active') : s.tun ? t('Enabled') : t('Off')}</Chip>}
      >
        <div className="flex items-start justify-between gap-4">
          <p className="text-meta text-ink2">
            {t('Captures traffic from devices on the LAN (Wi-Fi and USB) so they need no proxy settings. Domains are recognised from TLS/HTTP, DNS is left to the router. While on, it comes back automatically after a reboot. Its firewall rules are added at runtime and never written to the firmware; turning this off restores the stock route immediately.')}
          </p>
          <Toggle checked={s.tun} onChange={(v) => void setTun(v)} disabled={!!busy || !s.installed} label={t('Transparent proxy')} />
        </div>
        {s.tun && (
          <div className="mt-3 border-t border-line/8 pt-3">
            <Row label={t('Starts at boot')} value={s.enabled ? t('Yes') : t('No — start it on Overview')} />
          </div>
        )}
        <div className="mt-3 flex items-start justify-between gap-4 border-t border-line/8 pt-3">
          <div className="min-w-0">
            <p className="flex flex-wrap items-center gap-2 text-body font-semibold text-ink">
              {t('Mainland traffic bypasses TUN')}
              {s.tun_active && s.cn_bypass && (
                <Chip tone={s.cn_bypass_active ? 'ok' : 'warn'}>{s.cn_bypass_active ? t('Active') : t('Not applied')}</Chip>
              )}
            </p>
            <p className="text-meta text-ink2">
              {s.cn_bypass_available
                ? t('Connections to mainland China IP addresses skip mihomo and use the router’s hardware path: faster and lighter on the CPU and battery. Rules that send a mainland IP through a proxy no longer apply to TUN traffic.')
                : t('The mainland IP list is not installed on the router; run scripts/deploy-mihomo.sh once.')}
            </p>
          </div>
          <Toggle
            checked={s.cn_bypass}
            onChange={(v) => void save('bypass', { cn_bypass: v }, v ? t('Mainland bypass on') : t('Mainland bypass off'))}
            disabled={!!busy || !s.cn_bypass_available}
            label={t('Mainland traffic bypasses TUN')}
          />
        </div>
      </Card>

      <Card title={t('Web panel (metacubexd)')}>
        {s.panel?.installed && s.panel.url ? (
          <div className="space-y-3">
            <p className="text-meta text-ink2">
              {t('The full mihomo panel: live connections, logs, rules and rule-set updates. It talks to mihomo directly, so every device on the LAN that opens it can control the proxy.')}
            </p>
            <div className="flex flex-wrap items-center gap-2">
              <a
                href={s.panel.url}
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center rounded-ctl bg-accent px-3 py-1.5 text-body font-semibold text-white hover:opacity-90"
              >
                {t('Open metacubexd')}
              </a>
              <span className="tnum font-mono text-meta text-ink2">{s.panel.url}</span>
            </div>
            {s.panel.secret ? (
              <CopyField label={t('Panel secret')} value={s.panel.secret} hint={t('From the subscription config. metacubexd asks for it the first time.')} />
            ) : (
              <Row label={t('Panel secret')} value={t('None — the config sets no secret')} />
            )}
            {!s.running && <InlineStatus kind="info" live={false}>{t('The panel works while the proxy is running.')}</InlineStatus>}
          </div>
        ) : (
          <InlineStatus kind="info" live={false}>
            {t('metacubexd is not installed on the router; run scripts/deploy-mihomo.sh once.')}
          </InlineStatus>
        )}
      </Card>

      <Card title={t('Proxy port')}>
        <div className="flex flex-wrap items-end gap-2">
          <div className="w-48">
            <Field label={t('HTTP / SOCKS5 port')} error={portError}>
              <Input
                type="number"
                inputMode="numeric"
                min={1024}
                max={65535}
                value={portDraft ?? String(s.mixed_port ?? '')}
                onChange={(e) => {
                  setPortDraft(e.target.value)
                  setPortError(undefined)
                }}
              />
            </Field>
          </div>
          {portDraft !== null && portDraft !== String(s.mixed_port ?? '') && (
            <Button variant="primary" onClick={() => void applyPort()} loading={busy === 'port'}>
              {t('Apply')}
            </Button>
          )}
        </div>
        <p className="mt-2 text-meta text-ink2">
          {t('Listens on the LAN address only ({address}), never on the mobile network.', { address: s.lan_ip ?? t('router') })}
        </p>
      </Card>

      <Card title={t('About')}>
        <Row label={t('Core')} value={s.version ? `mihomo ${s.version}` : '—'} mono />
        <Row label={t('Starts with the router')} value={s.enabled ? t('Yes') : t('No — start it on Overview')} />
        <Row label={t('Subscriptions')} value={s.subscriptions} mono />
        <Row label={t('Controller')} value={t('127.0.0.1:9097 (router only)')} mono />
      </Card>
    </>
  )
}
