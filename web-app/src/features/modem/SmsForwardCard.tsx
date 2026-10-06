import { useState } from 'react'
import { api } from '../../data/api'
import { SMS_CHANNELS } from '../../data/cellular'
import { useResource } from '../../data/poll'
import { t } from '../../i18n'
import type { SmsForward, SmsForwardChannel } from '../../types'
import { Button, Field, Input, Select, Toggle } from '../../ui/controls'
import { toast, toastError } from '../../ui/feedback'
import { Card, InlineStatus, Row, Skeleton } from '../../ui/primitives'

const CHANNEL_INFO: Record<SmsForwardChannel, { label: string; target: string; placeholder: string }> = {
  bark: { label: 'Bark', target: t('Bark key or URL'), placeholder: 'https://api.day.app/…' },
  serverchan: { label: 'Server酱', target: t('SendKey'), placeholder: 'SCT…' },
  wecom: { label: t('WeCom group bot'), target: t('Webhook URL'), placeholder: 'https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=…' },
  telegram: { label: 'Telegram', target: t('Bot token'), placeholder: '123456:ABC…' },
  webhook: { label: t('Custom webhook'), target: t('URL (receives JSON)'), placeholder: 'https://…' },
}

export default function SmsForwardCard() {
  const res = useResource<SmsForward>('sms-forward', api.smsForward)
  const [draft, setDraft] = useState<{ channel: SmsForwardChannel; target: string; chat_id: string; via_proxy: boolean } | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  const f = res.data

  async function save(body: Record<string, unknown>, done: string) {
    setBusy('save')
    try {
      res.mutate(await api.smsForwardSet(body))
      toast(done)
      return true
    } catch (e) {
      toastError(e, t('Failed to save SMS forwarding'))
      return false
    } finally {
      setBusy(null)
    }
  }

  async function test() {
    setBusy('test')
    try {
      await api.smsForwardTest()
      toast(t('Test message sent'))
    } catch (e) {
      toastError(e, t('The test message was not delivered'))
    } finally {
      setBusy(null)
      res.refresh()
    }
  }

  if (res.status === 'loading') return <Skeleton className="h-24" />
  if (!f) {
    return (
      <InlineStatus kind="error" action={{ label: t('Retry'), onClick: res.refresh, loading: res.refreshing }}>
        {t('SMS forwarding settings could not be read.')}
      </InlineStatus>
    )
  }

  return (
    <Card title={t('SMS forwarding')}>
      <div className="space-y-3">
        <div className="flex items-start justify-between gap-4">
          <p className="text-meta text-ink2">
            {t('New text messages are pushed to your phone or chat within about 20 seconds. Messages already on the SIM are not sent. The key stays on the router and is never shown again.')}
          </p>
          <Toggle
            checked={f.enabled}
            disabled={busy !== null || (!f.configured && !f.enabled)}
            label={t('SMS forwarding')}
            onChange={(v) => void save({ enabled: v }, v ? t('SMS forwarding on') : t('SMS forwarding off'))}
          />
        </div>
        {f.configured && !draft && (
          <div>
            <Row label={t('Send to')} value={`${CHANNEL_INFO[f.channel].label}${f.target_hint ? ` · ${f.target_hint}` : ''}`} />
            <Row label={t('Forwarded')} value={f.last_sent ? t('{n} messages, last {time}', { n: f.forwarded, time: f.last_sent }) : String(f.forwarded)} />
          </div>
        )}
        {f.last_error && !draft && <InlineStatus kind="warn">{t('Last attempt failed: {error}', { error: f.last_error })}</InlineStatus>}
        {draft ? (
          <div className="space-y-2.5 rounded-ctl border border-line/8 p-3">
            <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
              <Field label={t('Service')}>
                <Select value={draft.channel} onChange={(e) => setDraft({ ...draft, channel: e.target.value as SmsForwardChannel })}>
                  {SMS_CHANNELS.map((c) => (
                    <option key={c} value={c}>
                      {CHANNEL_INFO[c].label}
                    </option>
                  ))}
                </Select>
              </Field>
              <Field label={CHANNEL_INFO[draft.channel].target}>
                <Input type="password" autoComplete="off" value={draft.target} placeholder={CHANNEL_INFO[draft.channel].placeholder} onChange={(e) => setDraft({ ...draft, target: e.target.value })} />
              </Field>
              {draft.channel === 'telegram' && (
                <Field label={t('Chat id')}>
                  <Input value={draft.chat_id} placeholder="-100…" onChange={(e) => setDraft({ ...draft, chat_id: e.target.value })} />
                </Field>
              )}
            </div>
            <div className="flex items-center justify-between gap-3">
              <span className="text-body text-ink">{t('Send through the proxy (needed for Telegram on mainland networks)')}</span>
              <Toggle checked={draft.via_proxy} onChange={(v) => setDraft({ ...draft, via_proxy: v })} label={t('Send through the proxy (needed for Telegram on mainland networks)')} />
            </div>
            <div className="flex gap-2">
              <Button
                variant="primary"
                loading={busy === 'save'}
                disabled={busy !== null || draft.target.trim() === ''}
                onClick={() =>
                  void save({ channel: draft.channel, target: draft.target.trim(), chat_id: draft.chat_id.trim(), via_proxy: draft.via_proxy }, t('Saved')).then(
                    (ok) => ok && setDraft(null),
                  )
                }
              >
                {t('Save')}
              </Button>
              <Button variant="ghost" onClick={() => setDraft(null)} disabled={busy !== null}>
                {t('Cancel')}
              </Button>
            </div>
          </div>
        ) : (
          <div className="flex flex-wrap gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => setDraft({ channel: f.channel, target: '', chat_id: f.chat_id ?? '', via_proxy: f.via_proxy })}
              disabled={busy !== null}
            >
              {f.configured ? t('Change destination') : t('Set up')}
            </Button>
            {f.configured && (
              <Button variant="ghost" size="sm" onClick={() => void test()} loading={busy === 'test'} disabled={busy !== null}>
                {t('Send a test')}
              </Button>
            )}
          </div>
        )}
      </div>
    </Card>
  )
}
