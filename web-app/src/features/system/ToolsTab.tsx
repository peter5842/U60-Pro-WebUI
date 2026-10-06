import { useCallback, useEffect, useRef, useState } from 'react'
import { api } from '../../data/api'
import { usePoll, useResource } from '../../data/poll'
import { formatBytes, formatDuration } from '../../format'
import { t } from '../../i18n'
import type { LoggerDownload, LoggerStatus, ProcessListResult } from '../../types'
import { IInfo, IRefresh } from '../../icons'
import { Button, Field, Input, Select } from '../../ui/controls'
import { confirm, toast, toastError } from '../../ui/feedback'
import { Card, Empty, InlineStatus, Meter } from '../../ui/primitives'

function downloadCsv(csv: string, prefix: string) {
  const blob = new Blob([csv], { type: 'text/csv' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = `${prefix}_${new Date().toISOString().slice(0, 19).replace(/:/g, '-')}.csv`
  a.click()
  URL.revokeObjectURL(url)
}

const DURATION_OPTS = [
  [300, t('5 minutes')],
  [900, t('15 minutes')],
  [1800, t('30 minutes')],
  [3600, t('1 hour')],
  [7200, t('2 hours')],
  [14400, t('4 hours')],
  [28800, t('8 hours')],
  [43200, t('12 hours')],
  [86400, t('24 hours')],
] as const

const INTERVAL_OPTS = [
  [1, t('1 second')],
  [3, t('3 seconds')],
  [5, t('5 seconds')],
  [10, t('10 seconds')],
  [30, t('30 seconds')],
  [60, t('1 minute')],
] as const

// ── Logger card (shared by signal + connection loggers) ───────────────────────

function LoggerCard({
  pollKey,
  title,
  description,
  countLabel,
  statusFn,
  startFn,
  stopFn,
  downloadFn,
  filePrefix,
}: {
  pollKey: string
  title: string
  description: string
  countLabel: string
  statusFn: () => Promise<LoggerStatus>
  startFn: (duration: number, interval: number) => Promise<unknown>
  stopFn: () => Promise<unknown>
  downloadFn: () => Promise<LoggerDownload>
  filePrefix: string
}) {
  const [duration, setDuration] = useState(3600)
  const [interval, setInterval_] = useState(3)
  const [busy, setBusy] = useState(false)
  // Only worth watching closely while a run is in flight; idle is the common case.
  const [live, setLive] = useState(false)
  const { data: status, refresh } = usePoll<LoggerStatus>(pollKey, statusFn, live ? 3000 : 15000)

  const isRunning = status?.running ?? false
  useEffect(() => setLive(isRunning), [isRunning])

  async function handleStart() {
    setBusy(true)
    try {
      await startFn(duration, interval)
      refresh()
    } catch (e) {
      toastError(e, t('Failed to start logger'))
    } finally {
      setBusy(false)
    }
  }

  async function handleStop() {
    try {
      await stopFn()
      refresh()
    } catch (e) {
      toastError(e, t('Failed to stop logger'))
    }
  }

  async function handleDownload() {
    try {
      const data = await downloadFn()
      downloadCsv(data.csv, filePrefix)
    } catch (e) {
      toastError(e, t('No data to download'))
    }
  }

  const progress = status && status.duration_secs > 0 ? (status.elapsed_secs / status.duration_secs) * 100 : 0

  return (
    <Card title={title}>
      <p className="mb-3 text-meta text-ink2">{t('{description} Logs stop at 8 MiB and flush at least every 30 seconds.', { description })}</p>
      {status?.last_error && <p role="alert" className="mb-3 text-meta text-danger">{status.last_error}</p>}

      {!isRunning && (
        <div className="mb-3 grid grid-cols-2 gap-2">
          <Field label={t('Duration')}>
            <Select value={duration} onChange={(e) => setDuration(Number(e.target.value))}>
              {DURATION_OPTS.map(([v, l]) => (
                <option key={v} value={v}>
                  {l}
                </option>
              ))}
            </Select>
          </Field>
          <Field label={t('Interval')}>
            <Select value={interval} onChange={(e) => setInterval_(Number(e.target.value))}>
              {INTERVAL_OPTS.map(([v, l]) => (
                <option key={v} value={v}>
                  {l}
                </option>
              ))}
            </Select>
          </Field>
        </div>
      )}

      <div className="flex flex-wrap gap-2">
        {!isRunning ? (
          <Button variant="primary" onClick={handleStart} loading={busy}>
            {t('Start')}
          </Button>
        ) : (
          <Button variant="danger" onClick={handleStop}>
            {t('Stop')}
          </Button>
        )}
        <Button variant="outline" onClick={handleDownload}>
          {t('Download CSV')}
        </Button>
      </div>

      {status && (
        <div className="mt-3 grid grid-cols-3 gap-3 border-t border-line/8 pt-3">
          <div>
            <p className="label">{t('Status')}</p>
            <p className={`mt-0.5 text-body font-bold ${isRunning ? 'text-ok' : 'text-ink3'}`}>
              {isRunning ? t('Running') : t('Stopped')}
            </p>
          </div>
          <div>
            <p className="label">{countLabel}</p>
            <p className="tnum font-mono mt-0.5 text-body font-bold text-ink">{status.samples ?? status.events ?? 0}</p>
          </div>
          <div>
            <p className="label">{t('Elapsed')}</p>
            <p className="tnum font-mono mt-0.5 text-body font-medium text-ink2">
              {formatDuration(status.elapsed_secs)} / {formatDuration(status.duration_secs)}
            </p>
          </div>
          {isRunning && <Meter pct={progress} className="col-span-3" />}
        </div>
      )}
    </Card>
  )
}

// ── AT console ────────────────────────────────────────────────────────────────

function AtConsole() {
  const [command, setCommand] = useState('')
  const [timeout, setTimeout_] = useState(2)
  const [history, setHistory] = useState<{ cmd: string; response: string; error?: boolean }[]>([])
  const [busy, setBusy] = useState(false)
  const portRes = useResource('tools-at-port', api.atPort)
  const port = portRes.data ? portRes.data.port : undefined
  const outputRef = useRef<HTMLDivElement>(null)

  async function handleSend() {
    if (!command.trim() || busy) return
    const cmd = command.trim()
    setCommand('')
    setBusy(true)
    try {
      const data = await api.atSend(cmd, timeout)
      setHistory((h) => [...h, { cmd, response: data.response }])
    } catch (e) {
      setHistory((h) => [...h, { cmd, response: (e as Error).message, error: true }])
    }
    setBusy(false)
    setTimeout(() => outputRef.current?.scrollTo(0, outputRef.current.scrollHeight), 50)
  }

  return (
    <Card
      title={
        <span className="inline-flex items-center gap-1.5">
          {t('AT console')}
          <span
            className="inline-flex cursor-help text-warn"
            title={t('AT commands talk directly to the modem. Incorrect write commands can disable connectivity or leave persistent modem settings behind.')}
            aria-label={t('AT command safety information')}
          >
            <IInfo size={14} />
          </span>
        </span>
      }
    >
      <div className="mb-3 rounded-ctl border border-warn/30 bg-warn/10 px-3 py-2 text-meta text-warn">
        <strong>{t('Safety warning:')}</strong> {t('AT commands bypass the normal settings APIs and talk directly to the modem. Only use documented read-only queries; commands that write, reset, reboot, or alter radio state can interrupt service or persist after the agent exits.')}
      </div>
      <p className="mb-3 text-meta text-ink2">
        {t('The agent accepts only its read-only command allowlist.')}
        {port !== undefined && (
          <span className={port ? 'text-ok' : 'text-warn'}>{' '}{port ? t('Port: {port}', { port }) : t('No AT port detected.')}</span>
        )}
      </p>
      {portRes.status === 'error' && (
        <InlineStatus kind="stale" className="mb-3" action={{ label: t('Retry'), onClick: portRes.refresh, loading: portRes.refreshing }}>
          {t('AT port status could not be read.')}
        </InlineStatus>
      )}

      <div
        ref={outputRef}
        className="mb-3 h-72 overflow-y-auto rounded-ctl border border-line/8 bg-surface2/50 p-3 font-mono text-meta"
      >
        {history.length === 0 && (
          <p className="text-ink3">{t('No commands sent yet. Try: AT, ATI, AT+COPS?, AT+CSQ, AT+CGDCONT?')}</p>
        )}
        {history.map((h, i) => (
          <div key={i} className="mb-2">
            <p className="text-accent">{'> '} {h.cmd}</p>
            <p className={`whitespace-pre-wrap break-words ${h.error ? 'text-danger' : 'text-ok'}`}>{h.response}</p>
          </div>
        ))}
        {busy && <p className="animate-pulse text-ink3">{t('Waiting for response…')}</p>}
      </div>

      <div className="flex flex-wrap items-end gap-2">
        <div className="min-w-0 flex-1 basis-full sm:basis-48">
          <Field label={t('AT command')}>
            <Input
              type="text"
              value={command}
              onChange={(e) => setCommand(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && !e.shiftKey) {
                  e.preventDefault()
                  handleSend()
                }
              }}
              placeholder="AT+COPS?"
              className="font-mono"
              autoComplete="off"
            />
          </Field>
        </div>
        <div className="w-36 shrink-0">
          <Field label={t('Timeout (seconds)')}>
            <Select value={timeout} onChange={(e) => setTimeout_(Number(e.target.value))}>
              <option value={2}>2</option>
              <option value={5}>5</option>
              <option value={10}>10</option>
              <option value={30}>30</option>
            </Select>
          </Field>
        </div>
        <Button variant="primary" onClick={handleSend} disabled={!command.trim()} loading={busy}>
          {t('Send')}
        </Button>
      </div>
    </Card>
  )
}

// ── Processes ─────────────────────────────────────────────────────────────────

function Processes() {
  const [data, setData] = useState<ProcessListResult | null>(null)
  const [busy, setBusy] = useState(false)
  const [killing, setKilling] = useState(false)

  const load = useCallback(async () => {
    setBusy(true)
    try {
      setData(await api.top())
    } catch (e) {
      toastError(e, t('Failed to load processes'))
    } finally {
      setBusy(false)
    }
  }, [])

  async function killBloat() {
    if (!data) return
    const ok = await confirm({
      title:
        data.bloat_count === 1
          ? t('Stop {n} optional service?', { n: data.bloat_count })
          : t('Stop {n} optional services?', { n: data.bloat_count }),
      body: t(
        'Frees roughly {size} of RAM. The agent re-checks the live firmware boot barrier, excludes every protected daemon, and sends only a graceful termination signal. The firmware may restart some services.',
        { size: formatBytes(data.bloat_rss_kb * 1024) },
      ),
      confirmLabel: t('Stop services'),
      danger: true,
    })
    if (!ok) return
    setKilling(true)
    try {
      const result = await api.killBloat()
      toast(
        result.killed.length > 0
          ? result.killed.length === 1
            ? t('Stopped {n} optional service, freed {size}', { n: result.killed.length, size: formatBytes(result.freed_rss_kb * 1024) })
            : t('Stopped {n} optional services, freed {size}', { n: result.killed.length, size: formatBytes(result.freed_rss_kb * 1024) })
          : t('No optional services were running'),
      )
      await load()
    } catch (e) {
      toastError(e, t('Failed to stop optional services'))
    } finally {
      setKilling(false)
    }
  }

  const procs = data?.processes.slice(0, 15) ?? []

  return (
    <Card
      title={t('Top processes')}
      action={
        <Button size="sm" variant="ghost" onClick={load} loading={busy}>
          <IRefresh size={13} /> {data ? t('Refresh') : t('Load')}
        </Button>
      }
      pad={false}
    >
      {data == null ? (
        <div className="p-4">
          <Empty title={t('Load on demand')} body={t('Reading /proc for all processes is expensive — load when needed.')} />
        </div>
      ) : (
        <>
          <div className="px-4 pb-3">
            <div className="flex flex-wrap items-center justify-between gap-2 rounded-ctl bg-surface2/70 px-3 py-2.5">
              <div className="min-w-0">
                <p className="label">{t('Optional services')}</p>
                <p className="tnum font-mono mt-0.5 text-body text-ink2">
                  {t('{bloat} of {total} processes', { bloat: data.bloat_count, total: data.total_count })} ·{' '}
                  <span className="font-semibold text-ink">{formatBytes(data.bloat_rss_kb * 1024)}</span> RAM ·{' '}
                  {data.bloat_cpu_pct.toFixed(1)}% CPU
                </p>
              </div>
              <Button
                size="sm"
                variant="danger"
                onClick={killBloat}
                loading={killing}
                disabled={data.bloat_count === 0}
              >
                {t('Stop optional services')}
              </Button>
            </div>
          </div>

          <div className="overflow-x-auto px-4 pb-3">
          <table className="w-full text-meta">
            <thead>
              <tr className="label border-b border-line/8 text-left">
                <th className="pb-1.5 pr-3 font-semibold">PID</th>
                <th className="pb-1.5 pr-3 font-semibold">{t('Name')}</th>
                <th className="pb-1.5 pr-3 text-right font-semibold">CPU%</th>
                <th className="pb-1.5 text-right font-semibold">{t('Mem')}</th>
              </tr>
            </thead>
            <tbody>
              {procs.map((p) => (
                <tr key={p.pid} className="border-b border-line/6 last:border-0">
                  <td className="tnum font-mono py-1 pr-3 text-ink3">{p.pid}</td>
                  <td className="max-w-[180px] truncate py-1 pr-3 font-medium text-ink">
                    {p.name}
                    {p.is_bloat && <span className="ml-1.5 text-caption font-semibold text-warn">{t('bloat')}</span>}
                  </td>
                  <td className="tnum font-mono py-1 pr-3 text-right text-ink2">{p.cpu_pct.toFixed(1)}</td>
                  <td className="tnum font-mono py-1 text-right text-ink2">{formatBytes(p.rss_kb * 1024)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          </div>
        </>
      )}
    </Card>
  )
}

// ── Tab ───────────────────────────────────────────────────────────────────────

export default function ToolsTab() {
  return (
    <div className="space-y-3">
      <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
        <LoggerCard
          pollKey="logger-signal"
          title={t('Signal logger')}
          description={t('Logs signal metrics (RSRP, RSRQ, SINR, RSSI, bands, CA) to CSV. Maximum 24 hours.')}
          countLabel={t('Samples')}
          statusFn={api.loggerSignalStatus}
          startFn={api.loggerSignalStart}
          stopFn={api.loggerSignalStop}
          downloadFn={api.loggerSignalDownload}
          filePrefix="signal_log"
        />
        <LoggerCard
          pollKey="logger-connection"
          title={t('Connection event logger')}
          description={t('Logs connection events: cell handovers, band changes, NR connect/disconnect, PCI changes. Maximum 24 hours.')}
          countLabel={t('Events')}
          statusFn={api.loggerConnectionStatus}
          startFn={api.loggerConnectionStart}
          stopFn={api.loggerConnectionStop}
          downloadFn={api.loggerConnectionDownload}
          filePrefix="connection_log"
        />
      </div>

      <AtConsole />
      <Processes />
    </div>
  )
}
