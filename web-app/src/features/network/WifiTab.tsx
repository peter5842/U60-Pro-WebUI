import { useId, useRef, useState, type ReactNode } from 'react'
import { api } from '../../data/api'
import { useResource } from '../../data/poll'
import { lang, t } from '../../i18n'
import type { WifiBand } from '../../types'
import { Button, Field, Input, Select, Toggle } from '../../ui/controls'
import { confirm, toastError, type ConfirmOptions } from '../../ui/feedback'
import { Card, Chip, InlineStatus, Loading, Skeleton, Unavailable } from '../../ui/primitives'
import GuestWifiCard from './GuestWifiCard'
import { getBandInsights } from './wifiAdvice'
import { bandSaveConfirm, masterOffConfirm, radioOffConfirm, syncConfirm } from './wifiConfirm'
import {
  buildBandPatch,
  buildSyncPatch,
  cancelDraft,
  draftFromBand,
  editDraft,
  formatBandwidthMode,
  hasErrors,
  initDraft,
  isDirty,
  isUncertainFailure,
  normalizeConfiguredChannel,
  parseTxPower,
  reconcileDraft,
  reloadDraft,
  startEditing,
  validateDraft,
  verifyApplied,
  type BandDraft,
  type BandSuffix,
  type WifiPatch,
} from './wifiDraft'

const CHANNELS_2G = ['auto', '1', '2', '3', '4', '5', '6', '7', '8', '9', '10', '11', '12', '13']
const CHANNELS_5G = [
  'auto', '36', '40', '44', '48', '52', '56', '60', '64', '100', '104', '108', '112', '116', '120',
  '124', '128', '132', '136', '140', '144', '149', '153', '157', '161', '165',
]

/** Submit one reviewed payload. Resolves true only when the device acknowledged it. */
type ApplyFn = (patch: WifiPatch, what: string, review?: ConfirmOptions) => Promise<boolean>

// ── Band card ─────────────────────────────────────────────────────────────────

function BandCard({
  label,
  other,
  band,
  suffix,
  masterEnabled,
  locked,
  apply,
}: {
  label: string
  other: string
  band: WifiBand
  suffix: BandSuffix
  masterEnabled: boolean
  locked: boolean
  apply: ApplyFn
}) {
  // The draft lives here, not in the status data: a Wi-Fi re-read (or a sibling band's save) hands
  // us a new `band` object, but only a real change to this band's settings reaches the draft (R03).
  const observed = draftFromBand(band)
  const [state, setState] = useState(() => initDraft(observed))
  const current = reconcileDraft(state, observed)
  if (current !== state) setState(current)
  const { editing, draft, base, conflict } = current
  const dirty = isDirty(current)

  const [saving, setSaving] = useState(false)
  const ids = useId()
  const hiddenId = `${ids}-hidden`
  const set = (patch: Partial<BandDraft>) => setState((s) => editDraft(s, patch))

  const errors = validateDraft(draft, base, band.security)
  const patch = buildBandPatch(suffix, draft, base)
  const canSave = dirty && !hasErrors(errors) && Object.keys(patch).length > 0 && !locked

  async function handleSave() {
    if (!canSave) return
    // Freeze what was reviewed: later edits or re-reads cannot change this payload.
    const frozen = buildBandPatch(suffix, draft, base)
    setSaving(true)
    try {
      const ok = await apply(frozen, t('{band} settings', { band: label }), bandSaveConfirm(label, other, frozen, band))
      if (ok) setState((s) => cancelDraft(s))
    } finally {
      setSaving(false)
    }
  }

  async function toggleRadio() {
    const turningOff = band.enabled
    const key = suffix === '2g' ? 'radio2_disabled' : 'radio5_disabled'
    const frozen = Object.freeze({ [key]: turningOff ? '1' : '0' })
    await apply(frozen, t('{band} radio', { band: label }), turningOff ? radioOffConfirm(label, other) : undefined)
  }

  const channels = [...(suffix === '2g' ? CHANNELS_2G : CHANNELS_5G)]
  if (draft.channel && !channels.includes(draft.channel)) channels.push(draft.channel)
  const htmodes = [...(band.bandwidthOptions ?? [])]
  for (const m of [base.htmode, draft.htmode]) if (m && !htmodes.includes(m)) htmodes.push(m)

  const insights = getBandInsights(suffix, band)
  const configuredChannel = normalizeConfiguredChannel(band.configuredChannel)
  const currentChannel = band.actualChannel ?? band.channel
  const currentWidth = band.actualBandwidth ?? band.bandwidth
  const txKnown = band.txpowerPercent != null
  const tx = parseTxPower(draft.txpower)

  return (
    <Card
      title={label}
      action={
        !editing ? (
          <Button size="sm" variant="ghost" aria-label={t('Edit {band} settings', { band: label })} onClick={() => setState(startEditing)}>
            {t('Edit')}
          </Button>
        ) : (
          <div className="flex gap-1.5">
            <Button size="sm" variant="ghost" aria-label={t('Cancel editing {band}', { band: label })} onClick={() => setState(cancelDraft)} disabled={saving}>
              {t('Cancel')}
            </Button>
            <Button size="sm" variant="primary" aria-label={t('Save {band} settings', { band: label })} onClick={handleSave} loading={saving} disabled={!canSave}>
              {t('Save')}
            </Button>
          </div>
        )
      }
    >
      <div className="space-y-3">
        <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1">
          <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5">
            <span
              aria-hidden="true"
              className={`h-2 w-2 shrink-0 rounded-full ${masterEnabled ? (band.enabled ? 'bg-ok' : 'bg-danger') : 'bg-warn'}`}
            />
            <span className="text-body text-ink2">
              {masterEnabled ? (band.enabled ? t('Enabled') : t('Disabled')) : t('Master off')}
            </span>
            {band.clients != null && (
              <span className="text-meta text-ink3">
                {band.clients === 1 ? t('{n} client', { n: band.clients }) : t('{n} clients', { n: band.clients })}
              </span>
            )}
          </div>
          <Toggle checked={band.enabled} onChange={toggleRadio} disabled={locked} label={t('{band} radio', { band: label })} />
        </div>
        {!masterEnabled && <p className="text-meta text-warn">{t('Global Wi-Fi is off. Band settings are still saved.')}</p>}

        {editing ? (
          <>
            {dirty && (
              <div>
                <Chip tone="warn">{t('Unsaved changes')}</Chip>
              </div>
            )}
            {conflict && (
              <InlineStatus kind="warn" action={{ label: t('Reload from device'), onClick: () => setState(reloadDraft) }}>
                {t("The device changed this band's settings while you were editing. Your edits are kept; reload to discard them.")}
              </InlineStatus>
            )}
            <Field label="SSID" error={errors.ssid}>
              <Input value={draft.ssid} onChange={(e) => set({ ssid: e.target.value })} autoComplete="off" />
            </Field>
            <Field label={t('Password')} hint={t('Leave unchanged to keep the current password')} error={errors.password}>
              <Input type="password" value={draft.password} onChange={(e) => set({ password: e.target.value })} autoComplete="new-password" />
            </Field>
            <div className="grid grid-cols-1 gap-2 border-t border-line/8 pt-3 sm:grid-cols-2">
              <Field label={t('Channel')}>
                <Select value={draft.channel} onChange={(e) => set({ channel: e.target.value })}>
                  {channels.map((c) => (
                    <option key={c} value={c}>
                      {c === 'auto' ? t('Auto') : c}
                    </option>
                  ))}
                </Select>
              </Field>
              <Field label={t('Bandwidth')}>
                <Select value={draft.htmode} onChange={(e) => set({ htmode: e.target.value })}>
                  {htmodes.map((m) => (
                    <option key={m} value={m}>
                      {formatBandwidthMode(m)}
                    </option>
                  ))}
                </Select>
              </Field>
              <Field
                label={t('TX power (%)')}
                hint={
                  txKnown
                    ? t('Percent of maximum power, 1–100. Currently {value}%. Clear the box to keep the current value.', { value: band.txpowerPercent ?? '' })
                    : t('The current value is unknown. Leave blank to keep it, or enter 1–100.')
                }
                error={tx.ok ? undefined : tx.error}
              >
                {(f) => (
                  <Input
                    id={f.id}
                    aria-describedby={f.describedBy}
                    aria-invalid={f.invalid || undefined}
                    inputMode="numeric"
                    autoComplete="off"
                    placeholder={t('Keep current')}
                    value={draft.txpower}
                    onChange={(e) => set({ txpower: e.target.value })}
                  />
                )}
              </Field>
              <div className="flex items-center gap-2 sm:items-end sm:pb-1.5">
                <Toggle checked={draft.hidden} onChange={(hidden) => set({ hidden })} labelledBy={hiddenId} />
                <span id={hiddenId} className="text-meta font-medium text-ink2">
                  {t('Hidden SSID')}
                </span>
              </div>
            </div>
          </>
        ) : (
          <>
            <div className="grid grid-cols-2 gap-x-3 gap-y-2">
              <Info label="SSID" value={band.ssid} strong />
              <Info label={t('Password')} value={band.password} mono />
              <Info label={t('Configured channel')} value={configuredChannel === 'auto' ? t('Auto') : configuredChannel} />
              <Info label={t('Current channel')} value={currentChannel != null ? String(currentChannel) : undefined} />
              <Info label={t('Configured width')} value={formatBandwidthMode(band.configuredBandwidth)} />
              <Info label={t('Current width')} value={currentWidth} />
              <Info label={t('Configured TX power')} value={txKnown ? `${band.txpowerPercent}%` : undefined} />
              <Info label={t('Security')} value={band.security} />
              <Info label={t('Hidden')} value={band.hidden ? t('Yes') : t('No')} />
            </div>
            {insights.length > 0 && (
              <div className="rounded-ctl bg-surface2/70 px-3 py-2">
                <p className="label mb-1">{t('Configuration notes')}</p>
                <div className="space-y-1">
                  {insights.map((insight) => (
                    <p key={insight} className="text-meta text-ink2">
                      {insight}
                    </p>
                  ))}
                </div>
              </div>
            )}
          </>
        )}
      </div>
    </Card>
  )
}

function Info({ label, value, strong = false, mono = false }: { label: string; value?: ReactNode; strong?: boolean; mono?: boolean }) {
  return (
    <div className="min-w-0">
      <p className="label">{label}</p>
      <p className={`break-words text-body ${strong ? 'font-semibold text-ink' : 'text-ink2'} ${mono ? 'font-mono text-meta' : ''}`}>
        {value == null || value === '' ? <Unavailable /> : value}
      </p>
    </div>
  )
}

// ── Tab ───────────────────────────────────────────────────────────────────────

interface Notice {
  kind: 'verifying' | 'verified' | 'mismatch' | 'unverified'
  text: string
  /** Read-only re-check; never repeats the change. */
  check?: () => void
}

export default function WifiTab() {
  const wifi = useResource('wifi:status', api.wifiStatus)
  const [pending, setPending] = useState(false)
  const [notice, setNotice] = useState<Notice | null>(null)
  const lock = useRef(false)
  const verifyId = useRef(0)

  /** Re-read the settings after an acknowledged (or uncertain) change and compare them with the request. */
  async function verify(patch: WifiPatch, what: string, acknowledged = true) {
    const id = ++verifyId.current
    setNotice({
      kind: 'verifying',
      text: acknowledged
        ? t("{what}: applied. Wi-Fi may be reconnecting. Verifying the device's settings…", { what })
        : t("{what}: checking the device's settings…", { what }),
    })
    try {
      const fresh = await api.wifiStatus()
      if (id !== verifyId.current) return
      wifi.mutate(fresh)
      const v = verifyApplied(patch, fresh)
      if (v.kind === 'mismatch') {
        setNotice({
          kind: 'mismatch',
          text: t('{what}: the device does not report the requested value yet ({fields}). Re-check in a moment.', {
            what,
            fields: v.fields.join(lang() === 'zh' ? '、' : ', '),
          }),
          check: () => void verify(patch, what, acknowledged),
        })
      } else {
        setNotice({
          kind: 'verified',
          text:
            v.kind === 'verified'
              ? t('{what}: the device now reports the new settings.', { what })
              : t('{what}: settings re-read from the device. A changed password cannot be read back to compare.', { what }),
        })
      }
    } catch {
      if (id !== verifyId.current) return
      setNotice({
        kind: 'unverified',
        text: t(
          "{what}: the change was sent but the device's settings could not be read back. They are unverified. Reconnect if Wi-Fi dropped, then check again.",
          { what },
        ),
        check: () => void verify(patch, what, acknowledged),
      })
    }
  }

  /**
   * The single path for every wireless mutation: review first (when the change can drop clients),
   * then submit exactly the reviewed payload once. A second call while one is open or in flight is
   * ignored, never queued or retried.
   */
  const apply: ApplyFn = async (patch, what, review) => {
    if (lock.current) return false
    lock.current = true
    try {
      if (review && !(await confirm(review))) return false
      setPending(true)
      try {
        await api.wifiSet({ ...patch })
      } catch (e) {
        if (isUncertainFailure(e)) {
          // No reply: the device may or may not have applied it. Never auto-retry; offer a read-only check.
          setNotice({
            kind: 'unverified',
            text: t(
              "{what}: no reply from the device. Wi-Fi may have restarted and the change may or may not have applied. Reconnect, then check the device's settings.",
              { what },
            ),
            check: () => void verify(patch, what, false),
          })
        } else {
          toastError(e, t('{what} failed', { what }))
        }
        return false
      } finally {
        setPending(false)
      }
      void verify(patch, what)
      return true
    } finally {
      lock.current = false
    }
  }

  const data = wifi.data
  if (!data) {
    if (wifi.status === 'error') {
      return (
        <InlineStatus kind="error" action={{ label: t('Retry'), onClick: wifi.refresh, loading: wifi.refreshing }}>
          {t('Could not read the Wi-Fi settings: {error}', { error: wifi.error ?? '' })}
        </InlineStatus>
      )
    }
    return (
      <Loading label={t('Loading Wi-Fi settings')} className="space-y-3">
        <Skeleton className="h-24" />
        <Skeleton className="h-72" />
      </Loading>
    )
  }

  async function toggleMaster() {
    const turningOff = data!.master_enabled
    const frozen = Object.freeze({ wifi_onoff: turningOff ? '0' : '1' })
    await apply(frozen, t('Global Wi-Fi'), turningOff ? masterOffConfirm() : undefined)
  }

  async function syncBands(source: BandSuffix) {
    const sourceBand = source === '2g' ? data!.band_2g : data!.band_5g
    const targetBand = source === '2g' ? data!.band_5g : data!.band_2g
    const sourceLabel = source === '2g' ? '2.4 GHz' : '5 GHz'
    const targetSuffix: BandSuffix = source === '2g' ? '5g' : '2g'
    const targetLabel = source === '2g' ? '5 GHz' : '2.4 GHz'
    const built = buildSyncPatch(sourceBand, targetSuffix)
    if ('error' in built) {
      toastError(new Error(t('Cannot sync from {band}: {error}', { band: sourceLabel, error: built.error })))
      return
    }
    await apply(
      built.patch,
      t('Copy {source} to {target}', { source: sourceLabel, target: targetLabel }),
      syncConfirm(sourceLabel, targetLabel, built.patch, targetBand, built.includePassword),
    )
  }

  const noticeKind = { verifying: 'info', verified: 'ok', mismatch: 'warn', unverified: 'warn' } as const

  return (
    <div className="space-y-3">
      {wifi.status === 'stale' && (
        <InlineStatus kind="stale" action={{ label: t('Retry'), onClick: wifi.refresh, loading: wifi.refreshing }}>
          {t('Showing the last Wi-Fi settings that loaded. The latest refresh failed: {error}', { error: wifi.error ?? '' })}
        </InlineStatus>
      )}
      {notice && (
        <InlineStatus
          kind={noticeKind[notice.kind]}
          action={notice.check ? { label: t('Check device'), onClick: notice.check } : undefined}
        >
          {notice.text}
        </InlineStatus>
      )}

      <Card title={t('Global Wi-Fi')}>
        <div className="flex items-center justify-between gap-3">
          <div className="min-w-0">
            <p className="text-body font-medium text-ink">{t('Master switch')}</p>
            <p className="mt-0.5 text-meta text-ink2">
              {!data.master_supported
                ? t('This firmware does not expose a reliable global Wi-Fi toggle.')
                : data.master_enabled
                  ? t('On — radios follow your per-band settings')
                  : t('Off — all Wi-Fi radios are globally disabled')}
            </p>
          </div>
          <Toggle
            checked={data.master_enabled}
            onChange={toggleMaster}
            disabled={pending || !data.master_supported}
            label={t('Master Wi-Fi switch')}
          />
        </div>
        {data.wifi6_supported && (
          <div className="mt-3 border-t border-line/8 pt-3">
            <Chip tone={data.wifi6_enabled ? 'ok' : 'default'}>{data.wifi6_enabled ? t('Wi-Fi 6 enabled') : t('Wi-Fi 6 disabled')}</Chip>
          </div>
        )}
        {data.wifi7_supported && (
          <div className="mt-3 border-t border-line/8 pt-3">
            <Chip tone="ok" wrap>
              {t('Wi-Fi 7 / 802.11be supported')}
            </Chip>
          </div>
        )}
      </Card>

      <Card title={t('Band sync')}>
        <p className="mb-2.5 text-meta text-ink2">{t('Copy SSID, password, security and hidden-state from one band to the other.')}</p>
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" onClick={() => syncBands('2g')} disabled={pending}>
            {t('Use {band} for both', { band: '2.4 GHz' })}
          </Button>
          <Button variant="outline" onClick={() => syncBands('5g')} disabled={pending}>
            {t('Use {band} for both', { band: '5 GHz' })}
          </Button>
        </div>
      </Card>

      <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
        <BandCard label="2.4 GHz" other="5 GHz" band={data.band_2g} suffix="2g" masterEnabled={data.master_enabled} locked={pending} apply={apply} />
        <BandCard label="5 GHz" other="2.4 GHz" band={data.band_5g} suffix="5g" masterEnabled={data.master_enabled} locked={pending} apply={apply} />
      </div>

      {data.guest ? (
        <GuestWifiCard guest={data.guest} locked={pending} apply={apply} />
      ) : (
        data.guest_ssid && (
          <Card title={t('Guest network')}>
            <p className="break-words text-body text-ink2">
              {t('SSID:')} <span className="font-semibold text-ink">{data.guest_ssid}</span>
            </p>
          </Card>
        )
      )}
    </div>
  )
}
