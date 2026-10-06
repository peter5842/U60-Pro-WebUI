import type { Dispatch } from 'react'
import { api } from '../../../data/api'
import { normaliseBands } from '../../../data/bands'
import { t } from '../../../i18n'
import type { BandLockState } from '../../../types'
import { Button, ToggleChip } from '../../../ui/controls'
import { confirm, toastError } from '../../../ui/feedback'
import { Card, InlineStatus } from '../../../ui/primitives'
import { bandLockConfirm, bandName, describeObserved, type Rat } from './confirmations'
import { bandSpec, hasConflict, isDirty, toggleBand, type DraftAction, type DraftState } from './draft'
import type { Ops } from './ops'

type BandState = DraftState<BandLockState, number[]>

/**
 * Band lock editor. Observed lock, editable draft and in-flight snapshot live in the reducer
 * (`draft.ts`); this component only renders them and runs the confirm-then-submit sequence.
 */
export function BandLock({
  title,
  description,
  supported,
  type,
  state,
  dispatch,
  ops,
  onApplied,
}: {
  title: string
  description: string
  /** Bands from the firmware capability list. Never guessed. */
  supported: number[]
  type: Rat
  state: BandState
  dispatch: Dispatch<DraftAction<BandLockState, number[]>>
  ops: Ops
  onApplied: () => void
}) {
  const label = type === 'nr' ? 'NR' : 'LTE'
  const dirty = isDirty(bandSpec, state)
  const conflict = hasConflict(bandSpec, state)
  const locked = ops.busy || state.pending !== null
  // Show every selected band, even one the capability list lacks, so the visible selection is the submitted one.
  const shown = normaliseBands([...supported, ...state.draft])
  const observedText = describeObserved(type, state.observed, supported)

  async function apply() {
    const snapshot = normaliseBands(state.draft) // frozen before the confirmation opens
    if (snapshot.length === 0 || !dirty || locked) return
    await ops.run(async () => {
      const ok = await confirm(bandLockConfirm(type, snapshot))
      if (!ok) return
      dispatch({ type: 'submit', snapshot })
      try {
        if (type === 'nr') await api.bandLockNr(snapshot.join(','))
        else await api.bandLockLte(snapshot)
        dispatch({ type: 'applied', snapshot })
        onApplied()
      } catch (e) {
        dispatch({ type: 'failed' })
        toastError(e, t('Band lock failed'))
      }
    })
  }

  return (
    <Card title={title}>
      <p className="mb-2.5 text-meta text-ink2">{description}</p>
      <p className="mb-3 text-meta text-ink2">
        {t('Current lock:')} <span className="font-semibold text-ink">{observedText}</span>
      </p>
      {shown.length === 0 ? (
        <InlineStatus kind="warn" live={false}>
          {t('The firmware did not report any supported {label} bands, so band locking is unavailable.', { label })}
        </InlineStatus>
      ) : (
        <>
          <div className="mb-2 flex flex-wrap gap-2">
            <Button size="sm" variant="ghost" onClick={() => dispatch({ type: 'edit', update: () => normaliseBands(supported) })} disabled={locked}>
              {t('Select all')}
            </Button>
            <Button size="sm" variant="ghost" onClick={() => dispatch({ type: 'edit', update: () => [] })} disabled={locked || state.draft.length === 0}>
              {t('Clear')}
            </Button>
          </div>
          <div role="group" aria-label={t('{label} bands', { label })} className="flex flex-wrap gap-1.5">
            {shown.map((b) => (
              <ToggleChip
                key={b}
                tone={type === 'nr' ? 'nr' : 'accent'}
                pressed={state.draft.includes(b)}
                disabled={locked}
                onClick={() => dispatch({ type: 'edit', update: (d) => toggleBand(d, b) })}
              >
                {bandName(type, b)}
              </ToggleChip>
            ))}
          </div>
          <div className="mt-3 space-y-2">
            {state.verifying && (
              <InlineStatus kind="info">{t('Request accepted — waiting for the modem to report the new lock.')}</InlineStatus>
            )}
            {state.mismatch && (
              <InlineStatus kind="warn">
                {t('The modem reports “{observed}” rather than the lock you applied. Check the band list and try again.', { observed: observedText })}
              </InlineStatus>
            )}
            {conflict && (
              <InlineStatus kind="warn" action={{ label: t('Use modem lock'), onClick: () => dispatch({ type: 'cancel' }) }}>
                {t('The modem’s lock changed to “{observed}” while you were editing. Your selection is kept.', { observed: observedText })}
              </InlineStatus>
            )}
          </div>
          <div className="mt-3 flex flex-wrap items-center gap-2">
            <Button variant="primary" onClick={apply} loading={state.pending !== null} disabled={!dirty || state.draft.length === 0 || locked}>
              {state.draft.length === 1
                ? t('Lock {n} band', { n: state.draft.length })
                : t('Lock {n} bands', { n: state.draft.length })}
            </Button>
            <Button variant="ghost" onClick={() => dispatch({ type: 'cancel' })} disabled={!dirty || locked}>
              {t('Cancel')}
            </Button>
          </div>
        </>
      )}
    </Card>
  )
}
