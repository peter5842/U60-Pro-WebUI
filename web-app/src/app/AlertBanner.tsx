import { useState } from 'react'
import { IX } from '../icons'
import { dismiss, endEpisodes, isDismissed, type Alert, type Dismissals } from './alerts'
import { useAlertConditions } from './HomeContext'
import { t } from '../i18n'

const episodeKey = (a: Alert) => `${a.id}:${a.level}`

/**
 * Alerts from the home poll (zero extra requests). Dismissing hides a
 * condition for its current episode; it shows again after an observed recovery
 * or if it escalates. Only new episodes and escalations are announced: the
 * live regions carry stable titles, never the ages and readings that change
 * with every heartbeat.
 */
export function AlertBanner() {
  const { active, resolved } = useAlertConditions()
  const [dismissals, setDismissals] = useState<Dismissals>(() => new Map())
  const [announced, setAnnounced] = useState({ keys: '', polite: '', assertive: '' })

  const current = endEpisodes(dismissals, resolved)
  if (current !== dismissals) setDismissals(current)

  const visible = active.filter((a) => !isDismissed(current, a))
  const hidden = active.length - visible.length

  const keys = visible.map(episodeKey).join('|')
  if (announced.keys !== keys) {
    const before = new Set(announced.keys.split('|'))
    const fresh = visible.filter((a) => !before.has(episodeKey(a)))
    const titles = (level: Alert['level']) => fresh.filter((a) => a.level === level).map((a) => a.title).join('. ')
    setAnnounced({ keys, polite: titles('warning'), assertive: titles('error') })
  }

  return (
    <>
      <div className="sr-only" role="status" aria-live="polite">{announced.polite}</div>
      <div className="sr-only" role="alert" aria-live="assertive">{announced.assertive}</div>
      {(visible.length > 0 || hidden > 0) && (
        <div className="mb-4 space-y-1.5">
          {visible.map((a) => (
            <div
              key={a.id}
              className={`flex items-center gap-2.5 rounded-ctl border px-3 py-2 text-body font-medium ${
                a.level === 'error'
                  ? 'border-danger/25 bg-danger/8 text-danger'
                  : 'border-warn/25 bg-warn/8 text-warn'
              }`}
            >
              <span className="min-w-0 flex-1">
                {a.title}
                {a.detail && <span className="font-normal">. {a.detail}</span>}
              </span>
              <button
                type="button"
                onClick={() => setDismissals((prev) => dismiss(prev, a))}
                className="-m-1.5 inline-flex shrink-0 items-center justify-center rounded-ctl p-1.5 opacity-60 transition-opacity hover:opacity-100 coarse:-my-3 coarse:h-11 coarse:w-11"
                aria-label={`Dismiss: ${a.title}`}
              >
                <IX size={14} />
              </button>
            </div>
          ))}
          {hidden > 0 && (
            <button
              type="button"
              onClick={() => setDismissals(new Map())}
              className="whitespace-nowrap text-meta font-semibold text-ink3 hover:text-ink coarse:min-h-11"
            >
              {hidden === 1 ? t('Show 1 dismissed alert') : t('Show {n} dismissed alerts', { n: hidden })}
            </button>
          )}
        </div>
      )}
    </>
  )
}
