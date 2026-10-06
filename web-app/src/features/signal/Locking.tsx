import { useCallback, useRef, useState } from 'react'
import { useHome } from '../../app/HomeContext'
import { api } from '../../data/api'
import { useResource } from '../../data/poll'
import { t } from '../../i18n'
import type { BandLockState } from '../../types'
import { Button } from '../../ui/controls'
import { confirm, toast, toastError } from '../../ui/feedback'
import { Card, InlineStatus, Loading, Skeleton } from '../../ui/primitives'
import { BandLock } from './locking/BandLock'
import { CellLock } from './locking/CellLock'
import { bandResetConfirm, cellLockConfirm, cellResetConfirm, type CellTuple } from './locking/confirmations'
import { bandSpec, lteObservation, modeSpec, nrSaObservation } from './locking/draft'
import { NetworkMode } from './locking/NetworkMode'
import type { Ops } from './locking/ops'
import { ServingCells } from './locking/ServingCells'
import { useDraft } from './locking/useDraft'

function describeLockState(state: BandLockState): string {
  if (state.kind === 'locked') return state.bands.join(', ')
  return state.kind === 'automatic' ? t('(automatic)') : t('(unknown)')
}

// ── Group ─────────────────────────────────────────────────────────────────────

export default function Locking() {
  // Shares the home poll rather than fetching /api/network/signal separately;
  // `refresh` re-reads that batch after a change so the read-back arrives promptly.
  const { data: home, status: homeStatus, refresh } = useHome()
  const signal = home?.signal ?? null
  const caps = useResource('modem-capabilities', api.modemCapabilities)
  const capabilities = caps.data

  // Observations come from the heartbeat; drafts are owned here so one reset can re-baseline both locks.
  // The SA control reads SA state only (never the NSA lock).
  const [lte, dispatchLte] = useDraft(bandSpec, lteObservation(signal), signal)
  const [nr, dispatchNr] = useDraft(bandSpec, nrSaObservation(signal), signal)
  const [mode, dispatchMode] = useDraft(modeSpec, signal?.net_select, signal)

  // One device operation at a time; the ref stops a repeated click from starting a second one.
  const [busy, setBusy] = useState(false)
  const busyRef = useRef(false)
  const run = useCallback(async (fn: () => Promise<void>) => {
    if (busyRef.current) return
    busyRef.current = true
    setBusy(true)
    try {
      await fn()
    } finally {
      busyRef.current = false
      setBusy(false)
    }
  }, [])
  const ops: Ops = { busy, run }

  /** Confirm, then submit exactly the reviewed tuple once (request semantics unchanged). */
  async function lockCell(cell: CellTuple, successText: string) {
    const frozen: CellTuple = { ...cell }
    await run(async () => {
      const ok = await confirm(cellLockConfirm(frozen))
      if (!ok) return
      try {
        if (frozen.tech === 'nr') await api.cellLockNr(frozen.pci, frozen.earfcn, frozen.band ?? '')
        else await api.cellLockLte(frozen.pci, frozen.earfcn)
        toast(successText)
        refresh()
      } catch (e) {
        toastError(e, t('Cell lock failed'))
      }
    })
  }

  const lockServingCell = (cell: CellTuple) => lockCell(cell, t('Locked to {tech} cell PCI {pci}', { tech: cell.tech === 'nr' ? 'NR' : 'LTE', pci: cell.pci }))

  async function resetBands() {
    await run(async () => {
      const ok = await confirm(bandResetConfirm())
      if (!ok) return
      dispatchLte({ type: 'submit', snapshot: [] })
      dispatchNr({ type: 'submit', snapshot: [] })
      try {
        await api.bandLockReset()
        dispatchLte({ type: 'applied', snapshot: [] })
        dispatchNr({ type: 'applied', snapshot: [] })
        toast(t('Band reset request accepted'))
        refresh()
      } catch (e) {
        dispatchLte({ type: 'failed' })
        dispatchNr({ type: 'failed' })
        toastError(e, t('Reset failed'))
      }
    })
  }

  async function resetCells() {
    await run(async () => {
      const ok = await confirm(cellResetConfirm())
      if (!ok) return
      try {
        await api.cellLockReset()
        toast(t('Cell lock reset request accepted'))
        refresh()
      } catch (e) {
        toastError(e, t('Reset failed'))
      }
    })
  }

  if (!signal) {
    return (
      <Loading label={t('Loading radio state')} className="space-y-3">
        <Skeleton className="h-40" />
        <Skeleton className="h-56" />
      </Loading>
    )
  }

  return (
    <div className="space-y-3">
      {homeStatus === 'stale' && (
        <InlineStatus kind="stale" action={{ label: t('Retry'), onClick: refresh }}>
          {t('Radio state could not be refreshed; showing the last values received.')}
        </InlineStatus>
      )}

      {capabilities ? (
        <>
          {caps.status === 'stale' && (
            <InlineStatus kind="stale" action={{ label: t('Retry'), onClick: caps.refresh, loading: caps.refreshing }}>
              {t('Firmware capabilities could not be refreshed; using the last values received.')}
            </InlineStatus>
          )}
          <NetworkMode modes={capabilities.network_modes} state={mode} dispatch={dispatchMode} ops={ops} onApplied={refresh} />
        </>
      ) : caps.status === 'error' ? (
        <Card title={t('Network mode and bands')}>
          <InlineStatus kind="error" action={{ label: t('Retry'), onClick: caps.refresh, loading: caps.refreshing }}>
            {t('Firmware capability data could not be read{detail}. Radio mode and band changes are unavailable until it loads.', {
              detail: caps.error ? ` (${caps.error})` : '',
            })}
          </InlineStatus>
        </Card>
      ) : (
        <Loading label={t('Loading firmware capabilities')}>
          <Skeleton className="h-40" />
        </Loading>
      )}

      <ServingCells signal={signal} ops={ops} onLock={lockServingCell} />

      {capabilities && (
        <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
          <BandLock
            title={t('NR 5G band lock')}
            description={t('Allowed NR bands. Works in 5G SA mode only — firmware does not support NSA band locking.')}
            supported={capabilities.nr_sa_bands}
            type="nr"
            state={nr}
            dispatch={dispatchNr}
            ops={ops}
            onApplied={refresh}
          />
          <BandLock
            title={t('LTE band lock')}
            description={t('Allowed LTE bands.')}
            supported={capabilities.lte_bands}
            type="lte"
            state={lte}
            dispatch={dispatchLte}
            ops={ops}
            onApplied={refresh}
          />
        </div>
      )}

      <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
        <CellLock type="nr" ops={ops} onLock={lockCell} />
        <CellLock type="lte" ops={ops} onLock={lockCell} />
      </div>

      <Card title={t('Reset locks')}>
        <p className="mb-3 text-meta text-ink2">
          {t('Remove all band and cell locks; the modem returns to automatic selection.')}
        </p>
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" onClick={resetBands} disabled={busy}>
            {t('Reset bands to automatic')}
          </Button>
          <Button variant="outline" onClick={resetCells} disabled={busy}>
            {t('Reset cell locks')}
          </Button>
        </div>
      </Card>

      <Card title={t('Diagnostics')}>
        <div className="tnum space-y-1 break-all font-mono text-caption text-ink3">
          <p>
            {t('LTE lock (raw):')} <span className="text-ink">{signal.raw_lte_band_lock || t('(empty)')}</span>
          </p>
          <p>
            {t('NR lock (raw):')} <span className="text-ink">{signal.raw_nr_band_lock || t('(empty)')}</span>
          </p>
          <p>
            {t('LTE lock (parsed):')} <span className="text-ink">{describeLockState(signal.lte_band_lock_state)}</span>
          </p>
          <p>
            {t('NR SA lock (parsed):')} <span className="text-ink">{describeLockState(signal.nr_sa_band_lock_state)}</span>
          </p>
          <p>
            {t('Network mode:')} <span className="text-ink">{signal.net_select || t('(unknown)')}</span>
          </p>
        </div>
      </Card>
    </div>
  )
}
