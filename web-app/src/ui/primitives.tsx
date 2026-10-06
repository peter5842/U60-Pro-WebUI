import type { ReactNode } from 'react'
import { IAlert, ICheck, IClock, IInfo } from '../icons'
import { t } from '../i18n'

// ── Card ──────────────────────────────────────────────────────────────────────

export function Card({
  title,
  action,
  children,
  className = '',
  pad = true,
}: {
  title?: ReactNode
  action?: ReactNode
  children: ReactNode
  className?: string
  pad?: boolean
}) {
  return (
    <section className={`rounded-panel border border-line/8 bg-surface ${className}`}>
      {title != null && (
        <header className="flex items-center justify-between gap-2 border-b border-line/8 px-4 py-2.5">
          <h2 className="font-display text-sm font-semibold tracking-[-0.01em] text-ink">{title}</h2>
          {action}
        </header>
      )}
      <div className={pad ? 'p-4' : ''}>{children}</div>
    </section>
  )
}

// ── Stat / Row / Chip ─────────────────────────────────────────────────────────

export function Stat({
  label,
  value,
  sub,
  tone = 'text-ink',
}: {
  label: string
  value: ReactNode
  sub?: ReactNode
  tone?: string
}) {
  return (
    <div className="min-w-0">
      <p className="label">{label}</p>
      <p className={`tnum mt-1 truncate font-mono text-xl font-medium ${tone}`}>{value}</p>
      {sub != null && <p className="mt-0.5 truncate text-meta text-ink3">{sub}</p>}
    </div>
  )
}

export function Row({
  label,
  value,
  mono = false,
  wrap = false,
}: {
  label: string
  value: ReactNode
  mono?: boolean
  wrap?: boolean
}) {
  return (
    <div className="flex items-baseline justify-between gap-3 py-0.5 text-body">
      <span className="shrink-0 text-ink2">{label}</span>
      <span
        className={`min-w-0 text-right text-ink ${mono ? 'tnum font-mono text-meta' : 'font-medium'} ${
          wrap ? 'break-all' : 'truncate'
        }`}
      >
        {value}
      </span>
    </div>
  )
}

export type ChipTone = 'default' | 'lte' | 'nr' | 'ok' | 'warn' | 'danger' | 'accent'

const CHIP_TONES: Record<ChipTone, string> = {
  default: 'border-line/12 bg-transparent text-ink2',
  lte: 'border-accent/30 bg-accent/8 text-accent',
  nr: 'border-nr/30 bg-nr/8 text-nr',
  ok: 'border-ok/25 bg-ok/10 text-ok',
  warn: 'border-warn/25 bg-warn/10 text-warn',
  danger: 'border-danger/25 bg-danger/10 text-danger',
  accent: 'border-accent/30 bg-accent/8 text-accent',
}

/** `wrap` lets a long sentence-like chip break onto several lines instead of overflowing a narrow row. */
export function Chip({ children, tone = 'default', wrap = false }: { children: ReactNode; tone?: ChipTone; wrap?: boolean }) {
  return (
    <span
      className={`inline-flex items-center gap-1 rounded-chip border px-1.5 py-px font-mono text-caption font-medium ${
        wrap ? 'max-w-full whitespace-normal text-left' : 'whitespace-nowrap'
      } ${CHIP_TONES[tone]}`}
    >
      {children}
    </span>
  )
}

// ── Loading / empty states ────────────────────────────────────────────────────

export function Spinner({ size = 16, className = '' }: { size?: number; className?: string }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      className={`animate-spin ${className}`}
      aria-hidden="true"
    >
      <circle cx={12} cy={12} r={9} stroke="currentColor" strokeOpacity={0.2} strokeWidth={2.5} />
      <path d="M21 12a9 9 0 0 0-9-9" stroke="currentColor" strokeWidth={2.5} strokeLinecap="round" />
    </svg>
  )
}

/** Decorative loading block. Wrap groups of skeletons in `Loading` so assistive tech gets one label. */
export function Skeleton({ className = '' }: { className?: string }) {
  return <div aria-hidden="true" className={`animate-pulse rounded-ctl bg-surface2 ${className}`} />
}

/**
 * Loading region: announces `label` once (polite `role="status"`, `aria-busy`) while its children
 * (typically `Skeleton`s) stay hidden from assistive tech.
 */
export function Loading({
  label = t('Loading'),
  children,
  className = '',
}: {
  label?: string
  children?: ReactNode
  className?: string
}) {
  return (
    <div role="status" aria-busy="true" className={className}>
      <span className="sr-only">{label}</span>
      <div aria-hidden="true">{children}</div>
    </div>
  )
}

/**
 * Em dash for a value that is not available, with a screen-reader label so it is not read as
 * "em dash" or skipped. Use instead of printing '—' or 'N/A' directly.
 */
export function Unavailable({ label = t('Unavailable') }: { label?: string }) {
  return (
    <>
      <span aria-hidden="true">—</span>
      <span className="sr-only">{label}</span>
    </>
  )
}

// ── Inline status (persistent feedback) ───────────────────────────────────────

export type InlineStatusKind = 'info' | 'ok' | 'warn' | 'error' | 'stale'

const STATUS_STYLES: Record<InlineStatusKind, { box: string; icon: string }> = {
  info: { box: 'border-line/12 bg-surface2', icon: 'text-accent' },
  ok: { box: 'border-ok/25 bg-ok/10', icon: 'text-ok' },
  warn: { box: 'border-warn/25 bg-warn/10', icon: 'text-warn' },
  error: { box: 'border-danger/25 bg-danger/10', icon: 'text-danger' },
  stale: { box: 'border-line/12 bg-surface2', icon: 'text-ink3' },
}

/**
 * Persistent inline feedback that stays on the page: failed reads, stale data, "accepted, verifying".
 * Use this instead of a toast when the user needs the message after 5 s (design.md § Feedback).
 *
 * Live-region policy: `error` is `role="alert"` (assertive); every other kind is `role="status"`
 * (polite). Both announce when the note appears or its text changes, so do not also fire a toast
 * for the same fact. Pass `live={false}` for content that is present on first render and should
 * not be announced. `action` renders a named retry/refresh button (disabled and `aria-busy` while
 * `loading`); the text children are the message.
 */
export function InlineStatus({
  kind,
  children,
  action,
  live = true,
  className = '',
}: {
  kind: InlineStatusKind
  children: ReactNode
  action?: { label: string; onClick: () => void; loading?: boolean }
  /** Set false to suppress the live-region role (static, already-visible content). */
  live?: boolean
  className?: string
}) {
  const style = STATUS_STYLES[kind]
  const Icon = kind === 'ok' ? ICheck : kind === 'warn' || kind === 'error' ? IAlert : kind === 'stale' ? IClock : IInfo
  const role = !live ? undefined : kind === 'error' ? 'alert' : 'status'
  return (
    <div
      role={role}
      data-kind={kind}
      className={`flex flex-wrap items-center gap-x-3 gap-y-2 rounded-ctl border px-3 py-2 text-body text-ink ${style.box} ${className}`}
    >
      <Icon size={15} className={`shrink-0 ${style.icon}`} aria-hidden="true" />
      <span className="min-w-0 flex-1 basis-40">{children}</span>
      {action && (
        <button
          type="button"
          onClick={action.onClick}
          disabled={action.loading}
          aria-busy={action.loading || undefined}
          className="inline-flex h-8 shrink-0 items-center justify-center gap-1.5 whitespace-nowrap rounded-ctl border border-line/15 bg-surface px-3 text-meta font-semibold text-ink transition-colors hover:bg-surface2 coarse:min-h-11 coarse:min-w-11 disabled:pointer-events-none disabled:opacity-45"
        >
          {action.loading && <Spinner size={13} />}
          {action.label}
        </button>
      )}
    </div>
  )
}

export function Empty({ icon, title, body }: { icon?: ReactNode; title: string; body?: string }) {
  return (
    <div className="flex flex-col items-center justify-center gap-1.5 py-10 text-center">
      {icon && <div className="mb-1 text-ink3">{icon}</div>}
      <p className="text-sm font-medium text-ink2">{title}</p>
      {body && <p className="max-w-xs text-xs text-ink3">{body}</p>}
    </div>
  )
}

// ── Progress / meters ─────────────────────────────────────────────────────────

export function Meter({
  pct,
  tone = 'bg-accent',
  className = '',
}: {
  pct: number
  tone?: string
  className?: string
}) {
  const clamped = Math.max(0, Math.min(100, pct))
  return (
    <div className={`h-1.5 overflow-hidden rounded-full bg-surface2 ${className}`}>
      <div
        className={`h-full rounded-full transition-[width] duration-500 ${tone}`}
        style={{ width: `${clamped}%` }}
      />
    </div>
  )
}

/** Signal-quality style bars (1–5). */
export function SignalBars({ bars, large = false }: { bars?: number; large?: boolean }) {
  // Unknown bars draw an empty meter labelled unavailable, never "0 of 5".
  const n = bars ?? 0
  const color = n >= 4 ? 'bg-ok' : n >= 2 ? 'bg-warn' : 'bg-danger'
  const heights = large ? [10, 16, 22, 28, 34] : [4, 7, 10, 13, 16]
  const width = large ? 'w-1.5' : 'w-1'
  return (
    <div
      className="flex items-end gap-[3px]"
      role="img"
      aria-label={bars == null ? t('Signal bars unavailable') : t('{n} of 5 bars', { n })}
    >
      {heights.map((h, i) => (
        <div
          key={i}
          className={`${width} rounded-[2px] ${i < n ? color : 'bg-line/15'}`}
          style={{ height: `${h}px` }}
        />
      ))}
    </div>
  )
}
