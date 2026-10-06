/* eslint-disable react-refresh/only-export-components */
import { useCallback, useEffect, useId, useRef, useState, type KeyboardEvent } from 'react'
import { IAlert, ICheck, IX } from '../icons'
import { Button } from './controls'
import { t } from '../i18n'

// ── Toasts ────────────────────────────────────────────────────────────────────

export interface ToastItem {
  id: number
  text: string
  kind: 'ok' | 'err'
  /** How many identical consecutive messages were folded into this toast (≥ 2 shows "×n"). */
  count?: number
}

/** Success toasts clear themselves; errors stay until dismissed (design.md § Feedback). */
const OK_TOAST_MS = 5000
/** Visible toasts per kind; the oldest is dropped beyond this. */
const MAX_PER_KIND = 3

let toastId = 0
let pushToast: ((t: ToastItem) => void) | null = null

/**
 * Fire a toast from anywhere (imperative). Use only when the result is not otherwise visible;
 * persistent facts (failed reads, stale data, "verifying") belong in an inline `InlineStatus`.
 */
export function toast(text: string, kind: 'ok' | 'err' = 'ok') {
  pushToast?.({ id: ++toastId, text, kind })
}

export function toastError(e: unknown, fallback = t('Something went wrong')) {
  toast(e instanceof Error ? e.message : fallback, 'err')
}

/**
 * Always-mounted live-region host. The polite `role="status"` region carries success toasts; the
 * assertive `role="alert"` region carries errors. Both exist, empty, from first render so that
 * additions are announced. Errors persist until dismissed. An identical message that is already
 * showing is folded into it (a "×n" count, hidden from assistive tech) and is not announced again.
 */
export function Toaster() {
  const [items, setItems] = useState<ToastItem[]>([])
  const itemsRef = useRef<ToastItem[]>([])
  const timers = useRef(new Map<number, ReturnType<typeof setTimeout>>())

  const commit = useCallback((next: ToastItem[]) => {
    itemsRef.current = next
    setItems(next)
  }, [])

  const dismiss = useCallback(
    (id: number) => {
      const t = timers.current.get(id)
      if (t) clearTimeout(t)
      timers.current.delete(id)
      commit(itemsRef.current.filter((x) => x.id !== id))
    },
    [commit],
  )

  useEffect(() => {
    const running = timers.current
    const arm = (t: ToastItem) => {
      const prev = running.get(t.id)
      if (prev) clearTimeout(prev)
      if (t.kind === 'ok') running.set(t.id, setTimeout(() => dismiss(t.id), OK_TOAST_MS))
    }
    pushToast = (t) => {
      const cur = itemsRef.current
      const dup = cur.find((x) => x.kind === t.kind && x.text === t.text)
      if (dup) {
        const bumped = { ...dup, count: (dup.count ?? 1) + 1 }
        commit(cur.map((x) => (x.id === dup.id ? bumped : x)))
        arm(bumped)
        return
      }
      let next = [...cur, t]
      const sameKind = next.filter((x) => x.kind === t.kind)
      if (sameKind.length > MAX_PER_KIND) {
        const drop = sameKind[0]
        const timer = running.get(drop.id)
        if (timer) clearTimeout(timer)
        running.delete(drop.id)
        next = next.filter((x) => x.id !== drop.id)
      }
      commit(next)
      arm(t)
    }
    return () => {
      pushToast = null
      running.forEach((t) => clearTimeout(t))
      running.clear()
    }
  }, [commit, dismiss])

  const render = (list: ToastItem[]) =>
    list.map((item) => (
      <div
        key={item.id}
        data-toast={item.kind}
        className={`pointer-events-auto mt-1.5 flex max-w-sm items-center gap-2 rounded-ctl border py-1 pl-3 pr-1 text-body font-medium shadow-sm ${
          item.kind === 'ok' ? 'border-ok/25 bg-surface text-ink' : 'border-danger/30 bg-surface text-danger'
        }`}
      >
        {item.kind === 'ok' ? <ICheck size={15} className="shrink-0 text-ok" /> : <IAlert size={15} className="shrink-0" />}
        <span className="min-w-0 py-1 text-ink">{item.text}</span>
        {(item.count ?? 1) > 1 && (
          <span aria-hidden="true" className="tnum shrink-0 font-mono text-caption text-ink3">
            ×{item.count}
          </span>
        )}
        <button
          type="button"
          onClick={() => dismiss(item.id)}
          aria-label={t('Dismiss notification')}
          className="flex h-8 w-8 shrink-0 items-center justify-center rounded-ctl text-ink2 transition-colors hover:bg-surface2 hover:text-ink coarse:h-11 coarse:w-11"
        >
          <IX size={14} />
        </button>
      </div>
    ))

  return (
    <div className="pointer-events-none fixed inset-x-0 bottom-[calc(4.5rem+env(safe-area-inset-bottom))] z-50 flex flex-col items-center px-4 lg:bottom-6">
      <div role="status" aria-live="polite" aria-atomic="false" data-toast-region="polite" className="flex w-full flex-col items-center">
        {render(items.filter((t) => t.kind === 'ok'))}
      </div>
      <div role="alert" aria-live="assertive" aria-atomic="false" data-toast-region="assertive" className="flex w-full flex-col items-center">
        {render(items.filter((t) => t.kind === 'err'))}
      </div>
    </div>
  )
}

// ── Confirm dialog (promise-based) ────────────────────────────────────────────

export interface ConfirmOptions {
  title: string
  body?: string
  confirmLabel?: string
  /** Shorthand for `kind: 'danger'`. */
  danger?: boolean
  /**
   * `danger`: irreversible/destructive (solid danger Confirm). `connection`: drops or may drop a
   * connection (primary Confirm). Both focus Cancel first; `default` focuses Confirm.
   */
  kind?: 'default' | 'danger' | 'connection'
  /** Compact mono key/value list: the operation, the affected radio/profile/cell. Never secrets. */
  details?: { label: string; value: string }[]
  /** What will be interrupted, e.g. "Devices on this band will disconnect." */
  consequence?: string
  /** The practical way back, e.g. "Reconnect to the 2.4 GHz network." */
  recovery?: string
}

interface PendingConfirm {
  resolve: (ok: boolean) => void
  opener: HTMLElement | null
}

let openConfirm: ((opts: ConfirmOptions, resolve: (ok: boolean) => void, opener: HTMLElement | null) => void) | null = null

/**
 * Imperative confirmation dialog. Resolves true only when Confirm is pressed; Escape, Cancel,
 * backdrop click and host unmount resolve false.
 *
 * Overlap policy (bounded): only one confirmation can be open. A `confirm()` issued while another is
 * open resolves `false` immediately and does NOT replace the open dialog, so the caller of the second
 * call must treat it as "not confirmed". The caller should freeze the payload it is confirming before
 * calling; the dialog never reads live state.
 */
export function confirm(opts: ConfirmOptions): Promise<boolean> {
  return new Promise((resolve) => {
    if (!openConfirm) {
      resolve(false)
      return
    }
    const active = document.activeElement
    openConfirm(opts, resolve, active instanceof HTMLElement && active !== document.body ? active : null)
  })
}

export function ConfirmHost() {
  const [opts, setOpts] = useState<ConfirmOptions | null>(null)
  const pending = useRef<PendingConfirm | null>(null)
  const dialogRef = useRef<HTMLDialogElement>(null)
  const downOnBackdrop = useRef(false)
  const uid = useId()
  const titleId = `${uid}-title`
  const descId = `${uid}-desc`

  /** Settle the open confirmation exactly once, close the dialog, then restore focus. */
  const settle = useCallback((ok: boolean) => {
    const p = pending.current
    if (!p) return
    pending.current = null
    p.resolve(ok)
    const d = dialogRef.current
    // Close first: while the modal is open everything behind it is inert and cannot take focus.
    if (d?.open) d.close()
    setOpts(null)
    const main = document.querySelector<HTMLElement>('main')
    const opener = p.opener
    if (opener?.isConnected) opener.focus()
    if (!opener?.isConnected || document.activeElement !== opener) (main ?? document.body).focus()
  }, [])

  useEffect(() => {
    openConfirm = (o, resolve, opener) => {
      if (pending.current) {
        resolve(false)
        return
      }
      pending.current = { resolve, opener }
      setOpts(o)
    }
    return () => {
      openConfirm = null
      const p = pending.current
      if (p) {
        pending.current = null
        p.resolve(false)
      }
    }
  }, [])

  useEffect(() => {
    if (!opts) return
    const d = dialogRef.current
    if (!d) return
    if (!d.open) d.showModal()
    const kind = opts.kind ?? (opts.danger ? 'danger' : 'default')
    d.querySelector<HTMLElement>(kind === 'default' ? '[data-confirm]' : '[data-cancel]')?.focus()
  }, [opts])

  if (!opts) return null

  const kind = opts.kind ?? (opts.danger ? 'danger' : 'default')
  const hasDesc = !!(opts.body || opts.details?.length || opts.consequence || opts.recovery)

  // Native modal dialogs usually wrap Tab, but browsers differ (some leave to browser chrome).
  const onKeyDown = (e: KeyboardEvent<HTMLDialogElement>) => {
    if (e.key !== 'Tab') return
    const f = Array.from(e.currentTarget.querySelectorAll<HTMLElement>('button:not([disabled])'))
    if (f.length === 0) return
    const first = f[0]
    const last = f[f.length - 1]
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault()
      last.focus()
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault()
      first.focus()
    }
  }

  return (
    <dialog
      ref={dialogRef}
      aria-labelledby={titleId}
      aria-describedby={hasDesc ? descId : undefined}
      onCancel={(e) => {
        e.preventDefault()
        settle(false)
      }}
      onKeyDown={onKeyDown}
      onPointerDown={(e) => {
        downOnBackdrop.current = e.target === e.currentTarget
      }}
      onClick={(e) => {
        // The dialog element itself is the full-viewport scrim, so a click whose target is the
        // dialog (and that also began there) is a backdrop click; clicks inside the panel are not.
        if (e.target === e.currentTarget && downOnBackdrop.current) settle(false)
        downOnBackdrop.current = false
      }}
      className="fixed inset-0 m-0 h-full max-h-none w-full max-w-none items-end justify-center overflow-y-auto border-0 bg-scrim/45 text-ink open:flex sm:items-center"
      style={{
        padding:
          'max(1rem, env(safe-area-inset-top)) max(1rem, env(safe-area-inset-right)) max(1rem, env(safe-area-inset-bottom)) max(1rem, env(safe-area-inset-left))',
      }}
    >
      <div className="w-full max-w-sm rounded-panel border border-line/12 bg-surface p-4">
        <h2 id={titleId} className="font-display text-sm font-semibold text-ink">
          {opts.title}
        </h2>
        {hasDesc && (
          <div id={descId} className="mt-1.5 space-y-2.5 text-body leading-relaxed text-ink2">
            {opts.body && <p>{opts.body}</p>}
            {opts.details && opts.details.length > 0 && (
              <dl className="space-y-1 rounded-ctl border border-line/8 bg-surface2 px-3 py-2 font-mono text-meta">
                {opts.details.map((d) => (
                  <div key={d.label} className="flex justify-between gap-3">
                    <dt className="shrink-0 text-ink3">{d.label}</dt>
                    <dd className="min-w-0 break-words text-right text-ink">{d.value}</dd>
                  </div>
                ))}
              </dl>
            )}
            {opts.consequence && (
              <p>
                <span className="font-semibold text-ink">{t('Effect:')} </span>
                {opts.consequence}
              </p>
            )}
            {opts.recovery && (
              <p>
                <span className="font-semibold text-ink">{t('Recovery:')} </span>
                {opts.recovery}
              </p>
            )}
          </div>
        )}
        <div className="mt-4 flex justify-end gap-2">
          <Button variant="ghost" data-cancel="" onClick={() => settle(false)}>
            {t('Cancel')}
          </Button>
          <Button variant={kind === 'danger' ? 'danger' : 'primary'} data-confirm="" onClick={() => settle(true)}>
            {opts.confirmLabel ?? t('Confirm')}
          </Button>
        </div>
      </div>
    </dialog>
  )
}
