// USB mode view logic (PLAN2 R10/R11): which modes may be offered, what a switch request looks
// like, and how a *scheduled* switch is verified. Pure: no React, no network, no clock reads.
//
// Three facts are kept apart and never merged:
//   active mode      what the gadget is presenting now (status.active_mode)
//   scheduled mode   what an accepted PUT /api/usb/mode asked for but status has not confirmed yet
//   boot default     what the agent re-applies after boot (status.default_mode)

import { t } from '../../i18n'
import type { UsbMode, UsbModeResult, UsbStatus } from '../../types'

/** The only modes the dashboard can ever request. `debug` is deliberately absent: the agent rejects it. */
export type UsbModeKey = UsbMode

export const USB_MODE_KEYS: readonly UsbModeKey[] = ['rndis', 'ecm', 'ncm']

export const USB_MODE_INFO: Record<UsbModeKey, { label: string; description: string }> = {
  rndis: {
    label: 'RNDIS',
    description: t("Microsoft's USB networking. Native on Windows; needs unmaintained drivers on macOS."),
  },
  ecm: {
    label: 'ECM',
    description: t('CDC-ECM USB Ethernet. Driver-free on macOS, Linux and modern Windows. Best supported mode.'),
  },
  ncm: {
    label: 'NCM',
    description: t(
      'CDC-NCM USB Ethernet. Higher throughput in theory. Experimental: ZTE does not wire ncm.0 into the normal USB switch.',
    ),
  },
}

export function isUsbModeKey(v: unknown): v is UsbModeKey {
  return typeof v === 'string' && (USB_MODE_KEYS as readonly string[]).includes(v)
}

export function usbModeLabel(mode: UsbModeKey | null | undefined): string {
  return mode ? USB_MODE_INFO[mode].label : t('unknown')
}

// ── Capabilities ──────────────────────────────────────────────────────────────

export interface ModeAvailability {
  mode: UsbModeKey
  label: string
  description: string
  supported: boolean
  experimental: boolean
  /** Why the mode cannot be chosen (set exactly when `supported` is false). */
  reason?: string
  /** Where the answer came from. `none`: no status, so nothing is offered. */
  source: 'capabilities' | 'supported_modes' | 'none'
}

/**
 * Which modes the UI may offer. `mode_capabilities` is the authority: a mode it lists as unsupported
 * stays unsupported whatever `supported_modes` says, and a mode it does not list is not offered.
 * Only when no capability list exists (older agent, or none of its entries was readable) does the
 * documented fallback apply: the `supported_modes` list. With no status at all nothing is offered.
 */
export function usbModeAvailability(status: UsbStatus | null): ModeAvailability[] {
  const caps = status?.mode_capabilities && status.mode_capabilities.length > 0 ? status.mode_capabilities : undefined
  return USB_MODE_KEYS.map((mode) => {
    const info = USB_MODE_INFO[mode]
    const base = { mode, label: info.label, description: info.description }
    if (!status) {
      return { ...base, supported: false, experimental: mode === 'ncm', source: 'none' as const, reason: t('USB status is unavailable.') }
    }
    if (caps) {
      const cap = caps.find((c) => c.mode === mode)
      if (!cap) {
        return {
          ...base,
          supported: false,
          experimental: mode === 'ncm',
          source: 'capabilities' as const,
          reason: t('{mode} is not offered by this firmware (the agent does not list it).', { mode: info.label }),
        }
      }
      return {
        ...base,
        supported: cap.supported,
        experimental: cap.experimental,
        source: 'capabilities' as const,
        ...(cap.supported ? {} : { reason: t('{mode} is not available on this firmware (the agent reports it unsupported).', { mode: info.label }) }),
      }
    }
    const supported = status.supported_modes.includes(mode)
    return {
      ...base,
      supported,
      experimental: status.experimental_modes?.includes(mode) ?? mode === 'ncm',
      source: 'supported_modes' as const,
      ...(supported ? {} : { reason: t("{mode} is not in the agent's supported modes.", { mode: info.label }) }),
    }
  })
}

/** The persistent boot default, kept separate from the active mode. `null` = not reported. */
export function usbBootDefault(status: UsbStatus | null): UsbModeKey | null {
  if (!status) return null
  if (status.default_mode) return status.default_mode
  if (status.ncm_persist_on_boot === true) return 'ncm'
  if (status.ncm_persist_on_boot === false) return 'ecm'
  return null
}

// ── Switch plan (what to confirm, what to send) ───────────────────────────────

export interface SwitchConfirm {
  title: string
  body: string
  confirmLabel: string
  kind: 'connection'
  details: { label: string; value: string }[]
  consequence: string
  recovery: string
}

export type SwitchPlan =
  | {
      ok: true
      mode: UsbModeKey
      /** Extra request fields. NCM needs the agent's experimental acknowledgement. */
      options?: { confirm_experimental: true }
      /** NCM -> ECM: the agent schedules this as a rollback rather than calling ubus. */
      rollbackFromNcm: boolean
      confirm: SwitchConfirm
    }
  | { ok: false; reason: string }

/**
 * Freeze a USB switch before asking for confirmation. Returns the exact request to send and the
 * dialog text, or a refusal. Anything that is not a supported, different mode is refused, so a
 * request for `debug` (or any unknown string) can never be built.
 */
export function planUsbSwitch(status: UsbStatus | null, target: unknown): SwitchPlan {
  if (!status) return { ok: false, reason: t('USB status is unavailable.') }
  if (!isUsbModeKey(target)) return { ok: false, reason: t('That USB mode is not available.') }
  const availability = usbModeAvailability(status).find((a) => a.mode === target)
  if (!availability?.supported) return { ok: false, reason: availability?.reason ?? t('That USB mode is not available.') }
  if (status.active_mode === target) return { ok: false, reason: t('{mode} is already the active mode.', { mode: usbModeLabel(target) }) }

  const from = status.active_mode
  const rollbackFromNcm = target === 'ecm' && from === 'ncm'
  const label = usbModeLabel(target)
  const details = [
    { label: t('Operation'), value: rollbackFromNcm ? t('Roll back to ECM') : t('Switch USB mode') },
    { label: t('Active now'), value: usbModeLabel(from) },
    { label: t('Requested'), value: availability.experimental ? t('{mode} (experimental)', { mode: label }) : label },
  ]
  const consequence = t(
    'The USB link disconnects and re-enumerates. A computer connected by USB loses its network link until the new mode is up, and may need a different driver. If you reach this dashboard over USB, you will lose it for that time. Devices on Wi-Fi are not expected to be affected.',
  )

  let recovery: string
  if (target === 'ncm') {
    recovery = t(
      'Keep a Wi-Fi path to the dashboard open. This is an experimental mode: if USB does not come back, reconnect over Wi-Fi and switch back to ECM.',
    )
  } else if (rollbackFromNcm) {
    recovery = t(
      'The agent switches the gadget back to ECM with its own preflight and rollback. Reconnect the USB cable or interface if the link does not return.',
    )
  } else {
    recovery = t('Reconnect USB after the switch. If the link does not return, use the dashboard over Wi-Fi and choose {mode} again.', {
      mode: usbModeLabel(from),
    })
  }

  return {
    ok: true,
    mode: target,
    ...(target === 'ncm' ? { options: { confirm_experimental: true as const } } : {}),
    rollbackFromNcm,
    confirm: {
      title: rollbackFromNcm ? t('Roll back to ECM?') : t('Switch USB to {mode}?', { mode: label }),
      body: rollbackFromNcm
        ? t('The device is in experimental NCM. This returns it to the standard ECM tethering mode.')
        : availability.experimental
          ? t('{mode} is experimental and not part of the stock USB switch.', { mode: label })
          : t('This changes the USB tethering mode from {from} to {to}.', { from: usbModeLabel(from), to: label }),
      confirmLabel: rollbackFromNcm ? t('Roll back') : t('Switch'),
      kind: 'connection',
      details,
      consequence,
      recovery,
    },
  }
}

// ── Verification of a scheduled / accepted switch ─────────────────────────────

/** Bounded recheck policy. A secondary one-shot resource reads status; it never resends the switch. */
export const USB_RECHECK_MS = 3000
export const USB_VERIFY_TIMEOUT_MS = 45_000

/** One status read made while verifying. */
export type UsbProbe =
  | { kind: 'status'; status: UsbStatus }
  /** The management path did not answer (network failure or timeout): the USB link may be re-enumerating. */
  | { kind: 'unreachable'; message: string }
  /** The agent answered with an error. Explicit, so verification stops. */
  | { kind: 'error'; message: string }

/** Agent errors carry an HTTP status; a transport failure does not. */
export function classifyUsbReadError(e: unknown): Exclude<UsbProbe, { kind: 'status' }> {
  const message = e instanceof Error && e.message ? e.message : t('Status check failed')
  const status = (e as { status?: unknown } | null)?.status
  return typeof status === 'number' ? { kind: 'error', message } : { kind: 'unreachable', message }
}

export interface PendingSwitch {
  requested: UsbModeKey
  from: UsbModeKey | null
  /**
   * `scheduled`: the agent said it will switch shortly. `accepted`: ubus took the request (ECM/RNDIS).
   * `unknown`: no reply arrived (the connection dropped), so whether it was accepted is not known.
   */
  via: 'scheduled' | 'accepted' | 'unknown'
  startedAt: number
  delayMs: number | null
  /** The agent's own wording, shown as-is. */
  rollback?: string
}

/** A request whose reply never arrived: verify from status, never resend. */
export function pendingFromUnknown(submitted: UsbModeKey, from: UsbModeKey | null, now: number): PendingSwitch {
  return { requested: submitted, from, via: 'unknown', startedAt: now, delayMs: null }
}

/** Describe an accepted request. `submitted` is the frozen mode we sent; the reply only refines it. */
export function pendingFromResult(
  result: UsbModeResult,
  submitted: UsbModeKey,
  from: UsbModeKey | null,
  now: number,
): PendingSwitch {
  if (result.state === 'scheduled') {
    return {
      requested: result.mode ?? submitted,
      from,
      via: 'scheduled',
      startedAt: now,
      delayMs: result.delayMs,
      ...(result.rollback !== undefined ? { rollback: result.rollback } : {}),
    }
  }
  return { requested: submitted, from, via: 'accepted', startedAt: now, delayMs: null }
}

export type SwitchVerdict =
  | { state: 'waiting' }
  | { state: 'reconnecting' }
  | { state: 'verified' }
  | { state: 'error'; message: string }
  | { state: 'timeout'; unreachable: boolean; lastActive: UsbModeKey | null }

export const isTerminalVerdict = (v: SwitchVerdict) => v.state === 'verified' || v.state === 'error' || v.state === 'timeout'

/**
 * Where a switch stands, given the latest probe. Order matters: a confirming status always wins
 * (even after the deadline); an explicit agent error stops verification; then the deadline; then
 * an unreachable management path reads as reconnecting; anything else is still waiting.
 */
export function evaluateSwitch(pending: PendingSwitch, probe: UsbProbe | null, now: number): SwitchVerdict {
  if (probe?.kind === 'status' && probe.status.active_mode === pending.requested) return { state: 'verified' }
  if (probe?.kind === 'error') return { state: 'error', message: probe.message }
  if (now - pending.startedAt >= USB_VERIFY_TIMEOUT_MS) {
    return {
      state: 'timeout',
      unreachable: probe?.kind === 'unreachable',
      lastActive: probe?.kind === 'status' ? probe.status.active_mode : null,
    }
  }
  if (probe?.kind === 'unreachable') return { state: 'reconnecting' }
  return { state: 'waiting' }
}

/** What to show for the scheduled-mode line. `null`: nothing is pending (or it is already verified). */
export function scheduledMode(pending: PendingSwitch | null, verdict: SwitchVerdict | null): UsbModeKey | null {
  if (!pending || !verdict || verdict.state === 'verified') return null
  return pending.requested
}

export interface VerdictNote {
  kind: 'info' | 'ok' | 'warn' | 'error'
  text: string
}

/** User-facing note for a verdict. Never claims completion or a reboot requirement the agent did not state. */
export function describeVerdict(pending: PendingSwitch, verdict: SwitchVerdict, activeNow: UsbModeKey | null): VerdictNote {
  const target = usbModeLabel(pending.requested)
  const secs = Math.round(USB_VERIFY_TIMEOUT_MS / 1000)
  switch (verdict.state) {
    case 'verified':
      return { kind: 'ok', text: t('Verified: the active USB mode is now {target}.', { target }) }
    case 'reconnecting':
      return {
        kind: 'warn',
        text: t(
          'Reconnect, verifying. The dashboard cannot reach the device right now, which is expected while USB re-enumerates. Still checking whether {target} is active. The request will not be sent again.',
          { target },
        ),
      }
    case 'error':
      return {
        kind: 'error',
        text: t('Not verified: the status check failed ({message}). The switch to {target} was not repeated.', { message: verdict.message, target }),
      }
    case 'timeout':
      return {
        kind: 'warn',
        text: verdict.unreachable
          ? t('Not verified: the device did not answer within {secs} s, so it is unknown whether {target} took effect. The request was not repeated.', { secs, target })
          : t('Not verified: after {secs} s the active mode still reads {current}, not {target}. The request was not repeated.', {
              secs,
              current: usbModeLabel(verdict.lastActive ?? activeNow),
              target,
            }),
      }
    case 'waiting':
      return {
        kind: 'info',
        text:
          pending.via === 'scheduled'
            ? t('{target} scheduled. The active mode is still {active} until the switch happens. Checking every {every} s.', {
                target,
                active: usbModeLabel(activeNow),
                every: USB_RECHECK_MS / 1000,
              })
            : pending.via === 'accepted'
              ? t('{target} requested and accepted by the firmware. Checking whether it becomes the active mode (every {every} s).', {
                  target,
                  every: USB_RECHECK_MS / 1000,
                })
              : t(
                  'No reply to the {target} request arrived, so it is not known whether it was accepted. Checking whether {target} becomes active. The request will not be sent again.',
                  { target },
                ),
      }
  }
}
