import { useRef, useState } from 'react'
import { api } from '../../data/api'
import { t } from '../../i18n'
import { Button } from '../../ui/controls'
import { confirm, toast, toastError } from '../../ui/feedback'
import { Card, Chip, InlineStatus } from '../../ui/primitives'
import { SECTION_LABELS, backupFileName, parseBackup, type ParsedBackup } from './backupView'

type Outcome = { status: string; message?: string }

export default function BackupCard() {
  const [busy, setBusy] = useState<'download' | 'restore' | null>(null)
  const [chosen, setChosen] = useState<Extract<ParsedBackup, { ok: true }> | null>(null)
  const [selected, setSelected] = useState<string[]>([])
  const [fileError, setFileError] = useState<string | null>(null)
  const [results, setResults] = useState<Record<string, Outcome> | null>(null)
  const input = useRef<HTMLInputElement>(null)

  async function download() {
    setBusy('download')
    try {
      const doc = await api.backup()
      const blob = new Blob([JSON.stringify(doc, null, 2)], { type: 'application/json' })
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = backupFileName(typeof doc.created === 'string' ? doc.created : undefined)
      a.click()
      setTimeout(() => URL.revokeObjectURL(url), 1000)
      toast(t('Backup downloaded'))
    } catch (e) {
      toastError(e, t('The backup could not be made'))
    } finally {
      setBusy(null)
    }
  }

  async function pick(file: File | undefined) {
    setResults(null)
    setFileError(null)
    setChosen(null)
    if (!file) return
    const parsed = parseBackup(await file.text())
    if (!parsed.ok) {
      setFileError(parsed.error)
      return
    }
    setChosen(parsed)
    setSelected(parsed.sections)
  }

  async function restore() {
    if (!chosen || selected.length === 0) return
    const ok = await confirm({
      title: t('Restore {n} settings groups?', { n: selected.length }),
      body: t('Current settings in these groups are replaced; rules and fixed addresses are added if missing. This can take a minute.'),
      kind: 'connection',
      confirmLabel: t('Restore'),
      details: selected.map((s) => ({ label: SECTION_LABELS[s] ?? s, value: '✓' })),
    })
    if (!ok) return
    setBusy('restore')
    try {
      const reply = await api.restore({ ...chosen.doc, only: selected })
      setResults((reply.results ?? {}) as Record<string, Outcome>)
      toast(t('Restore finished'))
    } catch (e) {
      toastError(e, t('Restore failed'))
    } finally {
      setBusy(null)
    }
  }

  return (
    <Card title={t('Backup and restore')}>
      <div className="space-y-3">
        <p className="text-meta text-ink2">
          {t('Saves this dashboard’s settings to a file: proxy subscriptions, SMS forwarding, schedules, port rules, fixed addresses, block list and device names. Useful after a factory reset or firmware update.')}
        </p>
        <InlineStatus kind="warn" live={false}>
          {t('The file contains your subscription links and push keys. Keep it private.')}
        </InlineStatus>
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" onClick={() => void download()} loading={busy === 'download'} disabled={busy !== null}>
            {t('Download backup')}
          </Button>
          <Button variant="ghost" onClick={() => input.current?.click()} disabled={busy !== null}>
            {t('Restore from a file…')}
          </Button>
          <input
            ref={input}
            type="file"
            accept="application/json,.json"
            className="hidden"
            aria-label={t('Backup file')}
            onChange={(e) => {
              void pick(e.target.files?.[0])
              e.target.value = ''
            }}
          />
        </div>
        {fileError && <InlineStatus kind="error">{fileError}</InlineStatus>}
        {chosen && !results && (
          <div className="space-y-2 rounded-ctl border border-line/8 p-3">
            <p className="text-meta text-ink2">
              {t('Backup from {date}', { date: chosen.created ?? t('an unknown date') })}
              {chosen.firmware ? ` · ${chosen.firmware}` : ''}
            </p>
            <ul className="space-y-1">
              {chosen.sections.map((s) => (
                <li key={s}>
                  <label className="flex items-center gap-2 text-body text-ink">
                    <input
                      type="checkbox"
                      checked={selected.includes(s)}
                      onChange={(e) => setSelected(e.target.checked ? [...selected, s] : selected.filter((x) => x !== s))}
                    />
                    {SECTION_LABELS[s] ?? s}
                  </label>
                </li>
              ))}
            </ul>
            <div className="flex gap-2">
              <Button variant="primary" onClick={() => void restore()} loading={busy === 'restore'} disabled={busy !== null || selected.length === 0}>
                {t('Restore')}
              </Button>
              <Button variant="ghost" onClick={() => setChosen(null)} disabled={busy !== null}>
                {t('Cancel')}
              </Button>
            </div>
          </div>
        )}
        {results && (
          <ul className="divide-y divide-line/6">
            {Object.entries(results)
              .filter(([, r]) => r.status !== 'skipped' && r.status !== 'missing')
              .map(([s, r]) => (
                <li key={s} className="flex flex-wrap items-center justify-between gap-2 py-1.5">
                  <span className="text-body text-ink">{SECTION_LABELS[s] ?? s}</span>
                  <span className="flex min-w-0 items-center gap-2">
                    {r.message && <span className="truncate text-meta text-ink3">{r.message}</span>}
                    <Chip tone={r.status === 'ok' ? 'ok' : r.status === 'partial' ? 'warn' : 'danger'}>
                      {r.status === 'ok' ? t('Restored') : r.status === 'partial' ? t('Partly') : t('Failed')}
                    </Chip>
                  </span>
                </li>
              ))}
          </ul>
        )}
      </div>
    </Card>
  )
}
