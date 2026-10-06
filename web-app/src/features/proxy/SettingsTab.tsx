import { useState } from 'react'
import { api } from '../../data/api'
import type { PollResult } from '../../data/poll'
import type { ProxyPreset, ProxyStatus } from '../../types'
import { Button, Field, Input, Segmented, Toggle } from '../../ui/controls'
import { confirm, toast, toastError } from '../../ui/feedback'
import { Card, Chip, InlineStatus, Row, Skeleton } from '../../ui/primitives'
import { PRESET_OPTIONS, validatePort } from './proxyView'

type Busy = 'preset' | 'tun' | 'port' | null

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
      toastError(e, 'Failed to save the setting')
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
            title: 'Turn on transparent proxy?',
            body:
              'Every device on this router’s Wi-Fi and USB is routed through mihomo without any device setup. ' +
              'The router’s own traffic and its management pages are not affected.',
            kind: 'connection',
            consequence: s?.running ? 'mihomo restarts; open connections drop for a few seconds.' : undefined,
            recovery: 'Turn it off here, or reboot the router — nothing is written to the firmware.',
          }
        : {
            title: 'Turn off transparent proxy?',
            body: 'Devices go back to the normal route unless they use the proxy address or PAC URL.',
            kind: 'connection',
            consequence: s?.running ? 'mihomo restarts; open connections drop for a few seconds.' : undefined,
          },
    )
    if (ok) await save('tun', { tun: on }, on ? 'Transparent proxy on' : 'Transparent proxy off')
  }

  async function applyPort() {
    const parsed = validatePort(portDraft ?? '')
    if (!parsed.ok) {
      setPortError(parsed.error)
      return
    }
    setPortError(undefined)
    if (await save('port', { mixed_port: parsed.port }, `Proxy port set to ${parsed.port}`)) setPortDraft(null)
  }

  if (status.status === 'loading') return <Skeleton className="h-40" />
  if (!s) {
    return (
      <InlineStatus kind="error" action={{ label: 'Retry', onClick: status.refresh, loading: status.refreshing }}>
        Proxy settings could not be read{status.error ? `: ${status.error}` : '.'}
      </InlineStatus>
    )
  }

  const preset = presetDraft ?? s.preset ?? 'bypass_cn'
  const presetHelp = PRESET_OPTIONS.find((o) => o.value === preset)?.help

  return (
    <>
      <Card title="Routing">
        <div className="space-y-3">
          <Segmented<ProxyPreset>
            label="Routing preset"
            options={PRESET_OPTIONS.map(({ value, label }) => ({ value, label }))}
            value={preset}
            onChange={setPresetDraft}
            disabled={!!busy}
            wrap
          />
          <p className="text-meta text-ink2">{presetHelp} Applies in Rule mode.</p>
          {presetDraft && presetDraft !== s.preset && (
            <Button
              variant="primary"
              onClick={() => void save('preset', { preset: presetDraft }, 'Routing updated').then((ok) => ok && setPresetDraft(null))}
              loading={busy === 'preset'}
            >
              Apply routing
            </Button>
          )}
        </div>
      </Card>

      <Card
        title="Transparent proxy (TUN)"
        action={<Chip tone={s.tun_active ? 'ok' : 'default'}>{s.tun_active ? 'Active' : s.tun ? 'Enabled' : 'Off'}</Chip>}
      >
        <div className="flex items-start justify-between gap-4">
          <p className="text-meta text-ink2">
            Captures traffic from devices on the LAN (Wi-Fi and USB) so they need no proxy settings. Domains are
            recognised from TLS/HTTP, DNS is left to the router. The firewall accepts it needs exist only while mihomo
            runs; a reboot always restores the stock route.
          </p>
          <Toggle checked={s.tun} onChange={(v) => void setTun(v)} disabled={!!busy || !s.installed} label="Transparent proxy" />
        </div>
      </Card>

      <Card title="Proxy port">
        <div className="flex flex-wrap items-end gap-2">
          <div className="w-48">
            <Field label="HTTP / SOCKS5 port" error={portError}>
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
              Apply
            </Button>
          )}
        </div>
        <p className="mt-2 text-meta text-ink2">Listens on the LAN address only ({s.lan_ip ?? 'router'}), never on the mobile network.</p>
      </Card>

      <Card title="About">
        <Row label="Core" value={s.version ? `mihomo ${s.version}` : '—'} mono />
        <Row label="Starts with the router" value={s.enabled ? 'Yes' : 'No — start it on Overview'} />
        <Row label="Subscriptions" value={s.subscriptions} mono />
        <Row label="Controller" value="127.0.0.1:9097 (router only)" mono />
      </Card>
    </>
  )
}
