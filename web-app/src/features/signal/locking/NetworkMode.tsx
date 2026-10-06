import type { Dispatch } from 'react'
import { api } from '../../../data/api'
import { t } from '../../../i18n'
import type { ModemCapabilities } from '../../../types'
import { Button, Segmented } from '../../../ui/controls'
import { confirm, toast, toastError } from '../../../ui/feedback'
import { Card, InlineStatus } from '../../../ui/primitives'
import { hasConflict, isDirty, modeSpec, type DraftAction, type DraftState } from './draft'
import { networkModeConfirm } from './confirmations'
import type { Ops } from './ops'

type ModeState = DraftState<string | undefined, string>

/**
 * Network mode is a named radio group with a draft and an explicit Apply: arrow keys move the
 * selection but never submit. Apply confirms (R11), then submits the frozen choice once.
 */
export function NetworkMode({
  modes,
  state,
  dispatch,
  ops,
  onApplied,
}: {
  modes: ModemCapabilities['network_modes']
  state: ModeState
  dispatch: Dispatch<DraftAction<string | undefined, string>>
  ops: Ops
  onApplied: () => void
}) {
  const current = state.observed
  const currentLabel = current === undefined ? undefined : (modes.find((m) => m.value === current)?.label ?? current)
  const dirty = isDirty(modeSpec, state)
  const conflict = hasConflict(modeSpec, state)
  const locked = ops.busy || state.pending !== null

  async function apply() {
    const next = modes.find((m) => m.value === state.draft)
    if (!next || !dirty || locked) return
    const snapshot = next.value // frozen before the confirmation opens
    await ops.run(async () => {
      const ok = await confirm(networkModeConfirm(next, { value: current, label: currentLabel }))
      if (!ok) return
      dispatch({ type: 'submit', snapshot })
      try {
        await api.networkModeSet(snapshot)
        dispatch({ type: 'applied', snapshot })
        toast(t('Network mode request accepted — the modem is reconnecting'))
        onApplied()
      } catch (e) {
        dispatch({ type: 'failed' })
        toastError(e, t('Failed to set network mode'))
      }
    })
  }

  return (
    <Card title={t('Network mode')}>
      <p className="mb-3 text-meta text-ink2">
        {t('Preferred network technology. The modem reconnects after a change.')}
      </p>
      <p className="mb-2 text-meta text-ink2">
        {t('Current mode:')} <span className="font-semibold text-ink">{currentLabel ?? t('Unknown (not reported)')}</span>
      </p>
      {modes.length === 0 ? (
        <InlineStatus kind="warn" live={false}>
          {t('The firmware did not report any selectable network modes.')}
        </InlineStatus>
      ) : (
        <>
          <Segmented
            wrap
            label={t('Network mode')}
            options={modes.map((m) => ({ value: m.value, label: m.label }))}
            value={state.draft}
            onChange={(v) => dispatch({ type: 'edit', update: () => v })}
            disabled={locked}
          />
          {state.verifying && (
            <InlineStatus kind="info" className="mt-3">
              {t('Request accepted — waiting for the modem to report the new mode.')}
            </InlineStatus>
          )}
          {state.mismatch && (
            <InlineStatus kind="warn" className="mt-3">
              {currentLabel !== undefined
                ? t('The modem reports {mode} rather than the mode you applied.', { mode: currentLabel })
                : t('The modem reports no network mode rather than the mode you applied.')}
            </InlineStatus>
          )}
          {conflict && (
            <InlineStatus kind="warn" className="mt-3" action={{ label: t('Use modem value'), onClick: () => dispatch({ type: 'cancel' }) }}>
              {currentLabel !== undefined
                ? t('The modem now reports {mode}. Your selection is kept.', { mode: currentLabel })
                : t('The modem now reports a different mode. Your selection is kept.')}
            </InlineStatus>
          )}
          <div className="mt-3 flex flex-wrap items-center gap-2">
            <Button variant="primary" onClick={apply} loading={state.pending !== null} disabled={!dirty || state.draft === '' || locked}>
              {t('Apply')}
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
