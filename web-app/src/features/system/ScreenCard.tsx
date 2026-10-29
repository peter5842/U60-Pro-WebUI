import { useState } from 'react'
import { api } from '../../data/api'
import { SCREEN_PAGES } from '../../data/screen'
import { usePoll } from '../../data/poll'
import { t } from '../../i18n'
import type { ScreenAutoOpen, ScreenSettings, ScreenStatus, ScreenTheme } from '../../types'
import { Button, Field, Segmented, Select, Toggle } from '../../ui/controls'
import { toastError } from '../../ui/feedback'
import { Card, Chip, type ChipTone, InlineStatus, Row, Skeleton } from '../../ui/primitives'

const AUTO_OPTIONS: { value: ScreenAutoOpen; label: string }[] = [
  { value: 'off', label: t('Off') },
  { value: 'boot', label: t('At boot') },
  { value: 'wake', label: t('Every wake') },
]

const THEME_OPTIONS: { value: ScreenTheme; label: string }[] = [
  { value: 'green', label: t('Green') },
  { value: 'amber', label: t('Amber') },
  { value: 'cyan', label: t('Cyan') },
]

const IDLE_ACTIONS: { value: ScreenSettings['idle_action']; label: string }[] = [
  { value: 'stock', label: t('Back to stock UI') },
  { value: 'sleep', label: t('Screen off') },
]

const IDLE_OPTS = [
  [60, t('1 minute')],
  [120, t('2 minutes')],
  [300, t('5 minutes')],
  [600, t('10 minutes')],
  [1800, t('30 minutes')],
] as const

const BRIGHTNESS_OPTS = [64, 128, 160, 204, 255] as const

function stateView(s: ScreenStatus): { label: string; tone: ChipTone } {
  switch (s.state) {
    case 'active':
      return { label: t('On the LCD'), tone: 'ok' }
    case 'starting':
    case 'restoring':
      return { label: t('Switching…'), tone: 'accent' }
    case 'refused':
      return { label: t('Not started'), tone: 'warn' }
    case 'error':
      return { label: t('Failed, stock UI restored'), tone: 'danger' }
    case 'stale':
      return { label: t('Recovering the stock UI'), tone: 'warn' }
    case 'idle':
      return { label: t('Stock UI'), tone: 'default' }
    default:
      return { label: t('Unknown'), tone: 'default' }
  }
}

export default function ScreenCard() {
  const [busy, setBusy] = useState<'open' | 'close' | 'save' | null>(null)
  const [draft, setDraft] = useState<ScreenSettings | null>(null)
  const res = usePoll<ScreenStatus>('system:screen', api.screen, busy ? 2000 : 10_000)
  const s = res.data

  if (res.status === 'loading') return <Skeleton className="h-24" />
  if (!s) {
    return (
      <InlineStatus kind="error" action={{ label: t('Retry'), onClick: res.refresh, loading: res.refreshing }}>
        {t('The geek screen status could not be read.')}
      </InlineStatus>
    )
  }

  const cur = draft ?? s.settings
  const dirty = draft !== null && JSON.stringify(draft) !== JSON.stringify(s.settings)
  const active = s.state === 'active' || s.state === 'starting'
  const view = stateView(s)
  const set = (patch: Partial<ScreenSettings>) => setDraft({ ...cur, ...patch })

  async function run(kind: 'open' | 'close') {
    setBusy(kind)
    try {
      res.mutate(kind === 'open' ? await api.screenOpen() : await api.screenClose())
    } catch (err) {
      toastError(err, kind === 'open' ? t('The geek screen did not start') : t('Could not return to the stock UI'))
      res.refresh()
    } finally {
      setBusy(null)
    }
  }

  async function save() {
    if (!draft) return
    setBusy('save')
    try {
      res.mutate(await api.screenSettingsSet(draft))
      setDraft(null)
    } catch (err) {
      toastError(err, t('Failed to save the screen settings'))
    } finally {
      setBusy(null)
    }
  }

  return (
    <Card title={t('Geek screen')} action={<Chip tone={view.tone}>{view.label}</Chip>}>
      <div className="space-y-3">
        <p className="text-meta text-ink2">
          {t('A terminal-style status screen for the device LCD: system, power, cellular, network, clients and the mihomo proxy, with touch. Leaving it, an error or a crash always hands the screen back to the stock UI.')}
        </p>
        {!s.installed && (
          <InlineStatus kind="info" live={false}>
            {t('zte-screen is not installed on the device. Deploy it with ./deploy-screen.sh.')}
          </InlineStatus>
        )}
        {s.state === 'refused' && s.reason && (
          <InlineStatus kind="warn" live={false}>
            {t('Last attempt was refused: {reason}', { reason: s.reason })}
          </InlineStatus>
        )}
        {s.installed && (
          <div className="flex flex-wrap items-center gap-2">
            {active ? (
              <Button variant="outline" onClick={() => void run('close')} loading={busy === 'close'} disabled={!!busy}>
                {t('Back to stock UI')}
              </Button>
            ) : (
              <Button variant={dirty ? 'outline' : 'primary'} onClick={() => void run('open')} loading={busy === 'open'} disabled={!!busy}>
                {t('Show on device')}
              </Button>
            )}
            {s.version && <span className="font-mono text-meta text-ink3">zte-screen {s.version}</span>}
          </div>
        )}

        {s.installed && (
          <div className="space-y-3 border-t border-line/8 pt-3">
            <div className="space-y-1.5">
              <p className="label">{t('Open automatically')}</p>
              <Segmented<ScreenAutoOpen>
                label={t('Open automatically')}
                options={AUTO_OPTIONS}
                value={cur.auto_open}
                onChange={(v) => set({ auto_open: v })}
                disabled={!!busy}
                wrap
              />
              <p className="text-meta text-ink3">
                {cur.auto_open === 'wake'
                  ? t('Whenever the stock UI lights the screen, the geek screen takes over after a moment. Tap :q or hold the power key to use the stock UI; it stays until the screen sleeps again.')
                  : cur.auto_open === 'boot'
                    ? t('Once per boot, a few seconds after the stock UI is up.')
                    : t('Only from this page or with a two-finger hold.')}
              </p>
            </div>
            <div className="flex items-start justify-between gap-4">
              <p className="text-meta text-ink2">
                {t('Two-finger hold: hold two fingers still on the lit stock UI for 2 seconds to open the geek screen.')}
              </p>
              <Toggle checked={cur.gesture} onChange={(v) => set({ gesture: v })} disabled={!!busy} label={t('Two-finger hold')} />
            </div>
            <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
              <Field label={t('Idle time')}>
                <Select value={cur.idle_secs} onChange={(e) => set({ idle_secs: Number(e.target.value) })} disabled={!!busy}>
                  {IDLE_OPTS.map(([v, l]) => (
                    <option key={v} value={v}>
                      {l}
                    </option>
                  ))}
                </Select>
              </Field>
              <Field label={t('Brightness')}>
                <Select value={cur.brightness} onChange={(e) => set({ brightness: Number(e.target.value) })} disabled={!!busy}>
                  {!BRIGHTNESS_OPTS.includes(cur.brightness as (typeof BRIGHTNESS_OPTS)[number]) && (
                    <option value={cur.brightness}>{Math.round((cur.brightness * 100) / 255)}%</option>
                  )}
                  {BRIGHTNESS_OPTS.map((v) => (
                    <option key={v} value={v}>
                      {Math.round((v * 100) / 255)}%
                    </option>
                  ))}
                </Select>
              </Field>
              <Field label={t('Start page')}>
                <Select value={cur.start_page} onChange={(e) => set({ start_page: e.target.value as ScreenSettings['start_page'] })} disabled={!!busy}>
                  {SCREEN_PAGES.map((p) => (
                    <option key={p} value={p}>
                      {p.toUpperCase()}
                    </option>
                  ))}
                </Select>
              </Field>
              <div className="flex items-end justify-between gap-4 pb-1.5">
                <p className="text-meta text-ink2">{t('CRT scanlines')}</p>
                <Toggle checked={cur.scanlines} onChange={(v) => set({ scanlines: v })} disabled={!!busy} label={t('CRT scanlines')} />
              </div>
            </div>
            <div className="space-y-1.5">
              <p className="label">{t('When idle')}</p>
              <Segmented<ScreenSettings['idle_action']>
                label={t('When idle')}
                options={IDLE_ACTIONS}
                value={cur.idle_action}
                onChange={(v) => set({ idle_action: v })}
                disabled={!!busy}
                wrap
              />
            </div>
            <div className="space-y-1.5">
              <p className="label">{t('Theme')}</p>
              <Segmented<ScreenTheme> label={t('Theme')} options={THEME_OPTIONS} value={cur.theme} onChange={(v) => set({ theme: v })} disabled={!!busy} wrap />
            </div>
            <Row label={t('Two-finger hold listener')} value={s.gesture_listener ? t('On') : t('Off')} />
            {dirty && (
              <div className="flex flex-wrap items-center gap-2">
                <Button variant="primary" onClick={() => void save()} loading={busy === 'save'}>
                  {t('Save')}
                </Button>
                <Button variant="ghost" onClick={() => setDraft(null)} disabled={!!busy}>
                  {t('Cancel')}
                </Button>
              </div>
            )}
          </div>
        )}
      </div>
    </Card>
  )
}
