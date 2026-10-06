import { useEffect, useRef, useState } from 'react'
import { api } from '../../../data/api'
import { useResource } from '../../../data/poll'
import { t } from '../../../i18n'
import type { CarrierNetwork, CarrierSelection as Selection } from '../../../types'
import { Button } from '../../../ui/controls'
import { confirm, toast, toastError } from '../../../ui/feedback'
import { Card, Chip, InlineStatus, Row, Skeleton } from '../../../ui/primitives'

const POLL_MS = 2000
const SCAN_LIMIT_MS = 4 * 60_000
const REGISTER_LIMIT_MS = 2 * 60_000

const stateLabel: Record<CarrierNetwork['state'], string> = {
  available: t('Available'),
  current: t('In use'),
  forbidden: t('Forbidden'),
  unknown: t('Unknown'),
}

export default function CarrierSelection() {
  const res = useResource<Selection>('cell:carriers', api.carriers)
  const [phase, setPhase] = useState<'idle' | 'scanning' | 'registering' | 'auto'>('idle')
  const started = useRef(0)
  const s = res.data

  // Poll while a scan or registration runs on the modem.
  useEffect(() => {
    if (phase !== 'scanning' && phase !== 'registering') return
    const timer = setInterval(() => {
      void api.carriers().then(
        (next) => {
          res.mutate(next)
          const limit = phase === 'scanning' ? SCAN_LIMIT_MS : REGISTER_LIMIT_MS
          const timedOut = Date.now() - started.current > limit
          if (phase === 'scanning' && (next.scan !== 'scanning' || timedOut)) {
            setPhase('idle')
            if (next.scan === 'failed' || timedOut) toast(t('The network search failed'), 'err')
          }
          if (phase === 'registering' && (next.register === 'success' || next.register === 'failed' || timedOut)) {
            setPhase('idle')
            if (next.register === 'success') toast(t('Registered on the selected network'))
            else toast(t('Registration failed; the modem will search again automatically'), 'err')
          }
        },
        () => undefined,
      )
    }, POLL_MS)
    return () => clearInterval(timer)
  }, [phase, res])

  async function scan() {
    const ok = await confirm({
      title: t('Search for mobile networks?'),
      body: t('The modem lists every network it can hear. Mobile data may pause until the search ends, usually one to three minutes.'),
      kind: 'connection',
      confirmLabel: t('Search'),
    })
    if (!ok) return
    try {
      res.mutate(await api.carrierScan())
      started.current = Date.now()
      setPhase('scanning')
    } catch (e) {
      toastError(e, t('The network search could not start'))
    }
  }

  async function select(n: CarrierNetwork) {
    const ok = await confirm({
      title: t('Register on {name} ({rat})?', { name: n.name, rat: n.rat_label }),
      body: t('The modem stays on this network until you switch back to automatic. If it has no service here, mobile data stops.'),
      kind: 'connection',
      confirmLabel: t('Register'),
      details: [{ label: 'MCC/MNC', value: n.mccmnc }],
      recovery: t('Use "Back to automatic" here at any time.'),
    })
    if (!ok) return
    try {
      res.mutate(await api.carrierSelect(n.mccmnc, n.rat))
      started.current = Date.now()
      setPhase('registering')
    } catch (e) {
      toastError(e, t('Registration could not start'))
    }
  }

  async function backToAuto() {
    setPhase('auto')
    try {
      res.mutate(await api.carrierAuto())
      toast(t('Automatic network selection restored'))
    } catch (e) {
      toastError(e, t('Could not switch back to automatic'))
    } finally {
      setPhase('idle')
    }
  }

  if (res.status === 'loading') return <Skeleton className="h-28" />
  if (!s) {
    return (
      <Card title={t('Carrier selection')}>
        <InlineStatus kind="error" action={{ label: t('Retry'), onClick: res.refresh, loading: res.refreshing }}>
          {t('The carrier selection state could not be read.')}
        </InlineStatus>
      </Card>
    )
  }
  const busy = phase !== 'idle'
  return (
    <Card
      title={t('Carrier selection')}
      action={
        <Button size="sm" variant="outline" onClick={() => void scan()} loading={phase === 'scanning'} disabled={busy}>
          {t('Search networks')}
        </Button>
      }
    >
      <div className="space-y-3">
        <Row label={t('Current network')} value={s.current.name ?? '—'} />
        <Row
          label={t('Selection')}
          value={<Chip tone={s.select_mode === 'manual' ? 'warn' : 'ok'}>{s.select_mode === 'manual' ? t('Manual') : t('Automatic')}</Chip>}
        />
        {s.select_mode === 'manual' && (
          <Button variant="primary" size="sm" onClick={() => void backToAuto()} loading={phase === 'auto'} disabled={busy}>
            {t('Back to automatic')}
          </Button>
        )}
        {phase === 'scanning' && (
          <InlineStatus kind="info">{t('Searching… this takes one to three minutes. Mobile data may pause meanwhile.')}</InlineStatus>
        )}
        {phase === 'registering' && <InlineStatus kind="info">{t('Registering on the selected network…')}</InlineStatus>}
        {s.networks.length > 0 && phase !== 'scanning' && (
          <ul className="divide-y divide-line/6">
            {s.networks.map((n) => (
              <li key={`${n.mccmnc}-${n.rat}`} className="flex items-center justify-between gap-2 py-2">
                <div className="min-w-0">
                  <p className="flex flex-wrap items-center gap-2 text-body font-medium text-ink">
                    {n.name}
                    <Chip>{n.rat_label}</Chip>
                    <Chip tone={n.state === 'current' ? 'ok' : n.state === 'forbidden' ? 'danger' : 'default'}>{stateLabel[n.state]}</Chip>
                  </p>
                  <p className="tnum font-mono text-meta text-ink3">{n.mccmnc}</p>
                </div>
                <Button size="sm" variant="ghost" onClick={() => void select(n)} disabled={busy || n.state === 'forbidden' || n.state === 'current'}>
                  {t('Register')}
                </Button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </Card>
  )
}
