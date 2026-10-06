import { useState } from 'react'
import { t } from '../../i18n'
import type { GuestWifi } from '../../types'
import type { ConfirmOptions } from '../../ui/feedback'
import { Button, Field, Input, Select, Toggle } from '../../ui/controls'
import { Card, Chip, Row } from '../../ui/primitives'
import {
  GUEST_SECURITY,
  GUEST_TIMES,
  buildGuestPatch,
  draftFromGuest,
  leftLabel,
  securityLabel,
  timeLimitLabel,
  validateGuest,
  type GuestDraft,
} from './guestView'
import type { WifiPatch } from './wifiDraft'

type ApplyFn = (patch: WifiPatch, what: string, review?: ConfirmOptions) => Promise<boolean>

export default function GuestWifiCard({ guest, locked, apply }: { guest: GuestWifi; locked: boolean; apply: ApplyFn }) {
  const [draft, setDraft] = useState<GuestDraft | null>(null)
  const [showErrors, setShowErrors] = useState(false)
  const on = guest.enabled_2g || guest.enabled_5g

  if (!draft) {
    return (
      <Card
        title={t('Guest network')}
        action={
          <Button size="sm" variant="outline" onClick={() => setDraft(draftFromGuest(guest))} disabled={locked}>
            {t('Edit')}
          </Button>
        }
      >
        <Row label={t('Status')} value={<Chip tone={on ? 'ok' : 'default'}>{on ? t('On') : t('Off')}</Chip>} />
        <Row label="SSID" value={guest.ssid ?? '—'} />
        <Row label={t('Security')} value={securityLabel(guest.security ?? 'none')} />
        <Row
          label={t('Time limit')}
          value={
            guest.active_minutes != null
              ? on && guest.left_secs
                ? `${timeLimitLabel(guest.active_minutes)} · ${leftLabel(guest.left_secs)}`
                : timeLimitLabel(guest.active_minutes)
              : '—'
          }
        />
        <p className="mt-2 text-meta text-ink3">
          {t('A separate network for visitors on both bands. With a time limit the router turns it off again by itself.')}
        </p>
      </Card>
    )
  }

  const errors = validateGuest(draft, guest)
  const patch = buildGuestPatch(draft, guest)
  const changed = Object.keys(patch).length > 0
  const set = (p: Partial<GuestDraft>) => setDraft({ ...draft, ...p })

  async function save() {
    if (!draft) return
    if (Object.keys(errors).length > 0) {
      setShowErrors(true)
      return
    }
    const restarts = Object.keys(patch).some((k) => !['guest_disabled_2g', 'guest_disabled_5g', 'guest_active_time'].includes(k))
    const ok = await apply(patch, t('Guest network'), {
      title: t('Apply guest network changes?'),
      kind: 'connection',
      confirmLabel: t('Apply changes'),
      details: [
        { label: t('Status'), value: draft.enabled ? t('On') : t('Off') },
        { label: 'SSID', value: draft.ssid },
        { label: t('Security'), value: securityLabel(draft.security) },
        { label: t('Time limit'), value: timeLimitLabel(draft.minutes) },
      ],
      consequence: restarts
        ? t('Wi-Fi restarts to apply this. Every Wi-Fi device, possibly this browser, briefly disconnects. Mobile data is not changed.')
        : t('Devices on the guest network disconnect when it turns off. The main Wi-Fi is not changed.'),
      recovery: t('Reconnect to Wi-Fi or use USB-C, then reopen this dashboard.'),
    })
    if (ok) {
      setDraft(null)
      setShowErrors(false)
    }
  }

  return (
    <Card title={t('Guest network')}>
      <div className="space-y-3">
        <div className="flex items-center justify-between gap-3">
          <span className="text-body text-ink">{t('Guest network on')}</span>
          <Toggle checked={draft.enabled} onChange={(v) => set({ enabled: v })} disabled={locked} label={t('Guest network on')} />
        </div>
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <Field label={t('Network name (SSID)')} error={showErrors ? errors.ssid : undefined}>
            <Input value={draft.ssid} onChange={(e) => set({ ssid: e.target.value })} disabled={locked} />
          </Field>
          <Field label={t('Security')}>
            <Select value={draft.security} onChange={(e) => set({ security: e.target.value })} disabled={locked}>
              {GUEST_SECURITY.map((s) => (
                <option key={s} value={s}>
                  {securityLabel(s)}
                </option>
              ))}
            </Select>
          </Field>
          {draft.security !== 'none' && (
            <Field
              label={t('Password')}
              hint={guest.has_key && guest.security !== 'none' ? t('Leave empty to keep the current password.') : undefined}
              error={showErrors ? errors.password : undefined}
            >
              <Input type="password" autoComplete="new-password" value={draft.password} onChange={(e) => set({ password: e.target.value })} disabled={locked} />
            </Field>
          )}
          <Field label={t('Time limit')} error={errors.minutes}>
            <Select value={draft.minutes} onChange={(e) => set({ minutes: Number(e.target.value) })} disabled={locked}>
              {GUEST_TIMES.map((m) => (
                <option key={m} value={m}>
                  {timeLimitLabel(m)}
                </option>
              ))}
            </Select>
          </Field>
        </div>
        <div className="flex items-center justify-between gap-3">
          <span className="text-body text-ink">{t('Hide the network name')}</span>
          <Toggle checked={draft.hidden} onChange={(v) => set({ hidden: v })} disabled={locked} label={t('Hide the network name')} />
        </div>
        <div className="flex gap-2">
          <Button variant="primary" onClick={() => void save()} loading={locked} disabled={!changed}>
            {t('Apply changes')}
          </Button>
          <Button
            variant="ghost"
            onClick={() => {
              setDraft(null)
              setShowErrors(false)
            }}
            disabled={locked}
          >
            {t('Cancel')}
          </Button>
        </div>
      </div>
    </Card>
  )
}
