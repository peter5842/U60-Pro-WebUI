import { useRef, useState } from 'react'
import { api } from '../../data/api'
import { useResource } from '../../data/poll'
import { t } from '../../i18n'
import type { SmsCapabilities, SmsMessage } from '../../types'
import { IMessage, IPlus } from '../../icons'
import { Button, Field, Input, Segmented } from '../../ui/controls'
import { toast, toastError, confirm } from '../../ui/feedback'
import { Card, Empty, InlineStatus, Skeleton } from '../../ui/primitives'
import { findMessage, inBox, markRead, reconcileSelection, removeMessage, restoreUnread, unreadCount, type Box } from './smsCollection'
import SmsForwardCard from './SmsForwardCard'

type SmsList = { messages: SmsMessage[]; dropped: number }

function formatDate(d?: string) {
  if (!d) return ''
  try {
    const stock = d.match(/^(\d{2});(\d{2});(\d{2});(\d{2});(\d{2});(\d{2})/)
    const value = stock
      ? new Date(2000 + Number(stock[1]), Number(stock[2]) - 1, Number(stock[3]), Number(stock[4]), Number(stock[5]), Number(stock[6]))
      : new Date(d)
    return Number.isNaN(value.getTime()) ? d : value.toLocaleString()
  } catch {
    return d
  }
}

export default function SmsTab() {
  const caps = useResource<SmsCapabilities>('sms-capabilities', api.smsCapabilities)
  const ready = !!caps.data?.available && !!caps.data.ready
  // The whole collection is stored once. `mutate` publishes an acknowledged change and discards any
  // older in-flight list read, so a pre-delete / pre-mark-read response can never overwrite it.
  const list = useResource<SmsList>('sms-list', api.smsListChecked, ready)

  const [box, setBox] = useState<Box>('inbox')
  const [selectedId, setSelectedId] = useState<number | null>(null)
  const [composing, setComposing] = useState(false)
  const [to, setTo] = useState('')
  const [text, setText] = useState('')
  const [sending, setSending] = useState(false)
  const [markFailed, setMarkFailed] = useState<string | null>(null)

  const all = list.data?.messages ?? null
  // Always the latest collection, including a value published but not yet rendered.
  const latest = useRef<SmsList | null>(null)
  const rendered = useRef<SmsList | null>(null)
  if (rendered.current !== list.data) {
    // Only a genuinely new published value replaces the ref; a render that still carries the
    // previous value must not undo a change committed but not yet rendered.
    rendered.current = list.data
    latest.current = list.data
  }

  function commit(next: SmsMessage[]) {
    const value = { messages: next, dropped: latest.current?.dropped ?? 0 }
    latest.current = value
    list.mutate(value)
  }

  // Selection follows the message id and is dropped when that message leaves the collection.
  const reconciled = all ? reconcileSelection(all, selectedId) : selectedId
  if (reconciled !== selectedId) setSelectedId(reconciled)

  const messages = all ? inBox(all, box) : []
  const selected = all ? findMessage(messages, selectedId) : null
  const unread = all ? unreadCount(all) : 0

  async function markMessageRead(id: number) {
    const current = latest.current
    if (!current) return
    setMarkFailed(null)
    commit(markRead(current.messages, id))
    try {
      await api.smsRead([id])
      // Acknowledged: re-assert so a list read started during the request cannot restore "unread".
      if (latest.current) commit(markRead(latest.current.messages, id))
    } catch (e) {
      if (latest.current) commit(restoreUnread(latest.current.messages, id))
      setMarkFailed(
        e instanceof Error && e.message
          ? t('The router did not mark that message as read ({error}). It is shown as unread again.', { error: e.message })
          : t('The router did not mark that message as read. It is shown as unread again.'),
      )
    }
  }

  async function deleteMsg(id: number) {
    const ok = await confirm({ title: t('Delete this message?'), confirmLabel: t('Delete'), danger: true })
    if (!ok) return
    try {
      await api.smsDelete([id])
      // The row disappearing is the confirmation; no success toast.
      if (latest.current) commit(removeMessage(latest.current.messages, id))
      setSelectedId((cur) => (cur === id ? null : cur))
    } catch (e) {
      toastError(e, t('Delete failed'))
    }
  }

  function openMsg(m: SmsMessage) {
    setSelectedId(m.id)
    if (m.tag === 1) void markMessageRead(m.id)
  }

  async function send(e: React.FormEvent) {
    e.preventDefault()
    // Freeze what is being sent.
    const payload = { to, text }
    setSending(true)
    try {
      await api.smsSend(payload.to, payload.text)
      toast(t('Message sent'))
      setTo('')
      setText('')
      setComposing(false)
      // Sent messages live in the same collection; re-read it whichever box is showing.
      list.refresh()
    } catch (err) {
      toastError(err, t('Failed to send'))
    } finally {
      setSending(false)
    }
  }

  if (caps.status === 'loading') return <Skeleton className="h-64" />
  if (caps.status === 'error' || !caps.data) {
    return (
      <InlineStatus kind="error" action={{ label: t('Retry'), onClick: caps.refresh, loading: caps.refreshing }}>
        {caps.error ? t('SMS availability could not be checked: {error}', { error: caps.error }) : t('SMS availability could not be checked.')}
      </InlineStatus>
    )
  }
  if (!ready) {
    return (
      <Card title={t('SMS unavailable')}>
        <Empty
          icon={<IMessage size={26} />}
          title={t('Firmware WMS is not ready')}
          body={caps.data.reason ?? t('The agent could not verify the SMS service, so listing, sending, and deletion are disabled.')}
        />
        <div className="flex justify-center">
          <Button size="sm" variant="ghost" onClick={caps.refresh} loading={caps.refreshing}>
            {t('Check again')}
          </Button>
        </div>
      </Card>
    )
  }

  return (
    <div className="space-y-3">
      <SmsForwardCard />
      <div className="flex flex-wrap items-center justify-between gap-2">
        <Segmented<Box>
          label={t('Message box')}
          options={[
            { value: 'inbox', label: unread > 0 ? t('Inbox ({n})', { n: unread }) : t('Inbox') },
            { value: 'sent', label: t('Sent') },
          ]}
          value={box}
          onChange={(v) => {
            setBox(v)
            setSelectedId(null)
          }}
        />
        <div className="flex items-center gap-2">
          <Button variant="ghost" onClick={list.refresh} loading={list.refreshing} aria-label={t('Refresh messages')}>
            {t('Refresh')}
          </Button>
          <Button
            variant="primary"
            onClick={() => {
              setComposing(true)
              setSelectedId(null)
            }}
          >
            <IPlus size={14} /> {t('New')}
          </Button>
        </div>
      </div>

      {list.status === 'stale' && (
        <InlineStatus kind="stale" action={{ label: t('Retry'), onClick: list.refresh, loading: list.refreshing }}>
          {list.error
            ? t('Showing the last messages loaded. Refreshing failed: {error}', { error: list.error })
            : t('Showing the last messages loaded. Refreshing failed.')}
        </InlineStatus>
      )}
      {markFailed && (
        <InlineStatus kind="error" action={{ label: t('Dismiss'), onClick: () => setMarkFailed(null) }}>
          {markFailed}
        </InlineStatus>
      )}
      {list.data && list.data.dropped > 0 && (
        <InlineStatus kind="warn" live={false}>
          {list.data.dropped === 1
            ? t('{n} message from the router could not be read and is not shown.', { n: list.data.dropped })
            : t('{n} messages from the router could not be read and are not shown.', { n: list.data.dropped })}
        </InlineStatus>
      )}

      {composing && (
        <Card title={t('New message')}>
          <form onSubmit={send} className="space-y-2.5">
            <Field label={t('To')}>
              <Input value={to} onChange={(e) => setTo(e.target.value)} required placeholder="+61400000000" inputMode="tel" />
            </Field>
            <Field label={t('Message')}>
              <textarea
                value={text}
                onChange={(e) => setText(e.target.value)}
                required
                rows={4}
                className="w-full resize-none rounded-ctl border border-line/12 bg-surface2/50 px-3 py-2 text-body text-ink outline-none transition-colors placeholder:text-ink3 focus:border-accent/60"
                placeholder={t('Type a message…')}
              />
            </Field>
            <div className="flex items-center gap-2">
              <Button type="submit" variant="primary" loading={sending} disabled={!to || !text}>
                {t('Send')}
              </Button>
              <Button type="button" variant="ghost" onClick={() => setComposing(false)}>
                {t('Cancel')}
              </Button>
              <span className="tnum font-mono ml-auto text-caption text-ink3">{text.length}/160</span>
            </div>
          </form>
        </Card>
      )}

      <div className="grid grid-cols-1 gap-3 lg:grid-cols-3">
        <Card className="lg:col-span-1" title={box === 'inbox' ? t('Inbox') : t('Sent')} pad={false}>
          {list.status === 'loading' ? (
            <div className="space-y-2 p-4">
              <Skeleton className="h-14" />
              <Skeleton className="h-14" />
              <Skeleton className="h-14" />
            </div>
          ) : list.status === 'error' || !all ? (
            <div className="p-4">
              <InlineStatus kind="error" action={{ label: t('Retry'), onClick: list.refresh, loading: list.refreshing }}>
                {list.error ? t('Messages could not be loaded: {error}', { error: list.error }) : t('Messages could not be loaded.')}
              </InlineStatus>
            </div>
          ) : messages.length === 0 ? (
            <Empty icon={<IMessage size={26} />} title={t('No messages')} />
          ) : (
            <ul className="max-h-[32rem] divide-y divide-line/6 overflow-y-auto">
              {messages.map((m) => (
                <li key={m.id}>
                  <button
                    onClick={() => openMsg(m)}
                    aria-current={selected?.id === m.id ? 'true' : undefined}
                    className={`block w-full px-4 py-2.5 text-left transition-colors hover:bg-surface2/60 ${
                      selected?.id === m.id ? 'bg-accent/8' : ''
                    }`}
                  >
                    <div className="flex items-start justify-between gap-2">
                      <p className={`truncate text-body ${m.tag === 1 ? 'font-bold text-ink' : 'font-medium text-ink2'}`}>
                        {m.number || '—'}
                      </p>
                      {m.tag === 1 && (
                        <>
                          <span aria-hidden="true" className="mt-1 h-2 w-2 shrink-0 rounded-full bg-accent" />
                          <span className="sr-only">{t('Unread')}</span>
                        </>
                      )}
                    </div>
                    <p className="mt-0.5 truncate text-meta text-ink3">{m.content}</p>
                    <p className="tnum font-mono mt-0.5 text-caption text-ink3">{formatDate(m.date)}</p>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </Card>

        <Card className="lg:col-span-2" title={selected ? t('Message') : t('Select a message')}>
          {selected ? (
            <div className="space-y-3">
              <div className="flex items-start justify-between gap-2">
                <div className="min-w-0">
                  <p className="truncate text-body font-semibold text-ink">
                    {box === 'inbox'
                      ? t('From: {number}', { number: selected.number || '\u2014' })
                      : t('To: {number}', { number: selected.number || '\u2014' })}
                  </p>
                  <p className="tnum font-mono mt-0.5 text-caption text-ink3">{formatDate(selected.date)}</p>
                </div>
                <Button size="sm" variant="ghost" onClick={() => deleteMsg(selected.id)}>
                  {t('Delete')}
                </Button>
              </div>
              <div className="rounded-ctl bg-surface2/70 p-3.5">
                <p className="whitespace-pre-wrap break-words text-body leading-relaxed text-ink">{selected.content}</p>
              </div>
              {box === 'inbox' && (
                <Button
                  variant="outline"
                  onClick={() => {
                    setComposing(true)
                    setTo(selected.number)
                    setSelectedId(null)
                  }}
                >
                  {t('Reply')}
                </Button>
              )}
            </div>
          ) : (
            <Empty icon={<IMessage size={26} />} title={t('No message selected')} />
          )}
        </Card>
      </div>
    </div>
  )
}
