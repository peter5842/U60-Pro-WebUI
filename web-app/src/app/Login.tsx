import { useId, useState } from 'react'
import { login, setToken } from '../data/client'
import { Mark } from '../ui/Mark'
import { Button } from '../ui/controls'
import { Segmented } from '../ui/controls'
import { t } from '../i18n'

function isMobilePinClient() {
  const ua = navigator.userAgent
  return (
    /Android|webOS|iPhone|iPad|iPod|BlackBerry|IEMobile|Opera Mini|Mobile/i.test(ua) ||
    (navigator.maxTouchPoints > 1 && /Macintosh/i.test(ua))
  )
}

export default function Login({ onAuthed }: { onAuthed: () => void }) {
  const mobile = isMobilePinClient()
  const [mode, setMode] = useState<'pin' | 'password'>(mobile ? 'pin' : 'password')
  const [pw, setPw] = useState('')
  const [pin, setPin] = useState('')
  const [err, setErr] = useState('')
  const [busy, setBusy] = useState(false)
  const pinId = useId()
  const pwId = useId()

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    setBusy(true)
    setErr('')
    try {
      const { token } = await login(mode === 'pin' ? { pin } : { password: pw })
      setToken(token)
      onAuthed()
    } catch (error) {
      setErr(error instanceof Error ? error.message : t('Sign in failed'))
    } finally {
      setBusy(false)
    }
  }

  const canSubmit = mode === 'pin' ? pin.length === 6 : pw.length > 0

  return (
    <div className="flex min-h-full items-center justify-center bg-bg p-6">
      <div className="w-full max-w-sm">
        <div className="mb-6 flex flex-col items-center text-center">
          <Mark size={36} className="mb-3 text-ink" />
          <h1 className="font-display text-xl font-semibold tracking-[-0.015em] text-ink">ZTE U60 Pro</h1>
          <p className="mt-0.5 text-body text-ink2">{t('Sign in to the dashboard')}</p>
        </div>

        <form
          onSubmit={submit}
          className="space-y-4 rounded-panel border border-line/8 bg-surface p-5"
        >
          {mobile && (
            <div className="flex justify-center">
              <Segmented
                options={[
                  { value: 'pin', label: 'PIN' },
                  { value: 'password', label: t('Password') },
                ]}
                value={mode}
                label={t('Sign-in method')}
                onChange={(m) => {
                  setMode(m)
                  setErr('')
                }}
              />
            </div>
          )}

          {mode === 'pin' ? (
            <div>
              <label htmlFor={pinId} className="label mb-1 block">
                PIN
              </label>
              <input
                id={pinId}
                type="password"
                inputMode="numeric"
                pattern="[0-9]*"
                maxLength={6}
                value={pin}
                onChange={(e) => setPin(e.target.value.replace(/\D/g, '').slice(0, 6))}
                className="tnum font-mono h-12 w-full rounded-ctl border border-line/12 bg-surface2/50 pl-[0.4em] text-center text-2xl font-medium tracking-[0.4em] text-ink transition-colors placeholder:text-ink3 focus:border-accent"
                placeholder="••••••"
                autoFocus
                autoComplete="one-time-code"
                enterKeyHint="done"
              />
            </div>
          ) : (
            <div>
              <label htmlFor={pwId} className="label mb-1 block">
                {t('Agent password')}
              </label>
              <input
                id={pwId}
                type="password"
                value={pw}
                onChange={(e) => setPw(e.target.value)}
                className="h-11 w-full rounded-ctl border border-line/12 bg-surface2/50 px-3.5 text-sm text-ink transition-colors placeholder:text-ink3 focus:border-accent"
                placeholder={t('Agent password')}
                autoFocus
                autoComplete="current-password"
              />
            </div>
          )}

          {err && <p className="text-xs font-medium text-danger">{err}</p>}

          <Button type="submit" variant="primary" loading={busy} disabled={!canSubmit} className="w-full !h-11">
            {t('Sign in')}
          </Button>
        </form>
      </div>
    </div>
  )
}
