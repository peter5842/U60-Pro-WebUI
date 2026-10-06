import { useRef, useState } from 'react'
import { api } from '../../data/api'
import { useResource, type PollResult } from '../../data/poll'
import { t } from '../../i18n'
import type { ApnModeState, ApnProfile } from '../../types'
import { Button, Field, Input, Segmented, Select } from '../../ui/controls'
import { confirm, toastError } from '../../ui/feedback'
import { Card, Chip, Empty, InlineStatus, Skeleton } from '../../ui/primitives'
import {
  activationConfirm,
  canApplyMode,
  modeChangeConfirm,
  modeState,
  modeWire,
  readBackOutcome,
  shownMode,
  type ApnMode,
  type ObservedApnMode,
} from './apnState'

const PDP_LABELS: Record<number, string> = { 1: 'IPv4', 2: 'IPv6', 3: 'IPv4v6' }
const AUTH_LABELS: Record<number, string> = { 0: t('None'), 1: 'PAP', 2: 'CHAP', 3: 'PAP/CHAP' }

const EMPTY_FORM = { name: '', apn: '', user: '', pass: '', auth: 0, pdp: 3 }

type Op = 'mode' | 'activate' | 'delete' | 'add'

/**
 * One owner for APN mode and profiles: both observations, the "an operation is running" lock that
 * serialises the two panels, and one coordinated read-back after every accepted change.
 */
function useApn() {
  const mode = useResource<ApnModeState>('apn-mode', api.apnModeGet)
  const profiles = useResource<ApnProfile[]>('apn-profiles', api.apnProfiles)
  const [op, setOp] = useState<Op | null>(null)
  // A ref guards against a second click landing before React re-renders the disabled state.
  const opRef = useRef<Op | null>(null)
  const [unverified, setUnverified] = useState(false)
  const [notice, setNotice] = useState<string | null>(null)
  const [readingBack, setReadingBack] = useState(false)

  /**
   * Re-read mode and profiles together and publish whatever succeeded. Returns true if both did.
   * `afterAccepted` (default) flags an unverified state when a read fails; after a rejected change
   * there is nothing accepted to verify, so a failed read-back is not reported as such.
   */
  async function readBack(afterAccepted = true): Promise<boolean> {
    setReadingBack(true)
    try {
      const [m, p] = await Promise.allSettled([api.apnModeGet(), api.apnProfiles()])
      const out = readBackOutcome(m, p)
      if (out.mode) mode.mutate(out.mode)
      if (out.profiles) profiles.mutate(out.profiles)
      setUnverified(afterAccepted && out.failed)
      return !out.failed
    } finally {
      setReadingBack(false)
    }
  }

  /** Run one device operation at a time; later calls while one is pending are ignored. */
  async function run(kind: Op, fn: () => Promise<void>) {
    if (opRef.current) return
    opRef.current = kind
    setOp(kind)
    setNotice(null)
    setUnverified(false)
    try {
      await fn()
    } finally {
      opRef.current = null
      setOp(null)
    }
  }

  return { mode, profiles, op, run, readBack, readingBack, unverified, notice, setNotice }
}

type ApnOwner = ReturnType<typeof useApn>

// ── APN mode ──────────────────────────────────────────────────────────────────

function ApnMode({ apn }: { apn: ApnOwner }) {
  const { mode, op } = apn
  const [draft, setDraft] = useState<ApnMode | null>(null)
  const observed: ObservedApnMode = mode.data?.mode ?? 'unknown'
  const shown = shownMode(draft, observed)
  const locked = op !== null

  async function applyMode() {
    // Freeze what was reviewed; the dialog and the request both use exactly this.
    const target = draft
    if (!target || !canApplyMode(target, observed) || locked) return
    const ok = await confirm(modeChangeConfirm(target, observed))
    if (!ok) return
    await apn.run('mode', async () => {
      try {
        await api.apnModeSet({ apn_mode: modeWire(target) })
      } catch (e) {
        toastError(e, t('Failed to change APN mode'))
        return
      }
      // Accepted by the router: show it now, then verify with a read-back.
      mode.mutate(modeState(target))
      setDraft(null)
      const verified = await apn.readBack()
      if (verified) {
        apn.setNotice(
          target === 'auto'
            ? t('APN mode is now automatic. Mobile data may reconnect briefly.')
            : t('APN mode is now manual. Mobile data may reconnect briefly.'),
        )
      }
    })
  }

  return (
    <Card title={t('APN mode')}>
      <p className="mb-3 text-meta text-ink2">
        {t('Automatic selects the APN from your SIM. Switch to manual to use a custom profile.')}
      </p>
      {mode.status === 'error' && (
        <InlineStatus
          kind="error"
          className="mb-3"
          action={{ label: t('Retry'), onClick: mode.refresh, loading: mode.refreshing }}
        >
          {mode.error ? t('APN mode could not be read: {error}', { error: mode.error }) : t('APN mode could not be read.')}
        </InlineStatus>
      )}
      {mode.status === 'stale' && (
        <InlineStatus
          kind="stale"
          className="mb-3"
          action={{ label: t('Retry'), onClick: mode.refresh, loading: mode.refreshing }}
        >
          {t('Showing the last APN mode read. The latest refresh failed.')}
        </InlineStatus>
      )}
      {mode.status === 'ready' && observed === 'unknown' && (
        <InlineStatus kind="warn" className="mb-3" live={false}>
          {t('The router reported an APN mode this dashboard does not recognise. Choose a mode and apply it to set it.')}
        </InlineStatus>
      )}
      {mode.status === 'loading' ? (
        <Skeleton className="h-9 w-48" />
      ) : (
        <div className="flex flex-wrap items-center gap-3">
          <Segmented<ObservedApnMode>
            label={t('APN mode')}
            value={shown}
            disabled={locked}
            options={[
              { value: 'auto', label: t('Automatic') },
              { value: 'manual', label: t('Manual') },
            ]}
            onChange={(v) => {
              // Selection only edits the draft. Arrow keys must never reach the router.
              if (v === 'auto' || v === 'manual') setDraft(v === observed ? null : v)
            }}
          />
          <Button
            variant="primary"
            onClick={applyMode}
            loading={op === 'mode'}
            disabled={locked || !canApplyMode(draft, observed)}
          >
            {t('Apply')}
          </Button>
          {draft && (
            <Button variant="ghost" disabled={locked} onClick={() => setDraft(null)}>
              {t('Cancel')}
            </Button>
          )}
        </div>
      )}
    </Card>
  )
}

// ── Profiles ──────────────────────────────────────────────────────────────────

function ProfileList({ apn, profiles }: { apn: ApnOwner; profiles: ApnProfile[] }) {
  const { mode, op } = apn
  const locked = op !== null
  const observed: ObservedApnMode = mode.data?.mode ?? 'unknown'

  async function activateProfile(p: ApnProfile) {
    if (locked) return
    // Freeze the reviewed profile before the dialog opens.
    const target = { profileId: p.profileId, profilename: p.profilename, wanapn: p.wanapn }
    const ok = await confirm(activationConfirm(target, observed))
    if (!ok) return
    await apn.run('activate', async () => {
      try {
        await api.apnActivate({ profileId: target.profileId })
      } catch (e) {
        toastError(e, t('Failed to activate APN profile'))
        // The agent rolls the mode back on failure; show whatever the router now reports.
        await apn.readBack(false)
        return
      }
      // The agent sets manual mode before enabling the profile, so the accepted state is manual.
      mode.mutate(modeState('manual'))
      const verified = await apn.readBack()
      if (verified) apn.setNotice(t('Activated "{name}". Mobile data may reconnect briefly.', { name: target.profilename }))
    })
  }

  async function deleteProfile(p: ApnProfile) {
    if (locked) return
    const target = { profileId: p.profileId, profilename: p.profilename }
    const ok = await confirm({ title: t('Delete APN profile "{name}"?', { name: target.profilename }), confirmLabel: t('Delete'), danger: true })
    if (!ok) return
    await apn.run('delete', async () => {
      try {
        await api.apnDelete({ profileId: target.profileId })
      } catch (e) {
        toastError(e, t('Failed to delete APN profile'))
        return
      }
      await apn.readBack()
    })
  }

  return (
    <div className="space-y-2">
      {profiles.map((p) => (
        <div
          key={p.profileId}
          className={`flex flex-wrap items-center justify-between gap-2 rounded-ctl border px-3 py-2 ${
            p.isEnable ? 'border-accent/30 bg-accent/4' : 'border-line/8'
          }`}
        >
          <div className="min-w-0">
            <p className="flex items-center gap-2 text-body font-semibold text-ink">
              <span className="truncate">{p.profilename}</span>
              {p.isEnable && (
                <Chip tone={observed === 'manual' ? 'ok' : 'default'}>
                  {observed === 'manual' ? t('Active') : t('Selected')}
                </Chip>
              )}
            </p>
            <p className="tnum font-mono mt-0.5 truncate text-meta text-ink2">
              {p.wanapn} — {(p.pdpType !== null && PDP_LABELS[p.pdpType]) || '?'} / {(p.pppAuthMode !== null && AUTH_LABELS[p.pppAuthMode]) || '?'}
              {p.username ? ` — ${p.username}` : ''}
            </p>
          </div>
          <div className="flex shrink-0 gap-1.5">
            {!p.isEnable && (
              <Button size="sm" variant="primary" disabled={locked} onClick={() => activateProfile(p)} aria-label={t('Activate {name}', { name: p.profilename })}>
                {t('Activate')}
              </Button>
            )}
            <Button
              size="sm"
              variant="ghost"
              disabled={p.isEnable || locked}
              onClick={() => deleteProfile(p)}
              aria-label={t('Delete {name}', { name: p.profilename })}
              title={p.isEnable ? t('Switch to another APN before deleting this profile') : undefined}
            >
              {t('Delete')}
            </Button>
          </div>
        </div>
      ))}
    </div>
  )
}

function AddProfile({ apn }: { apn: ApnOwner }) {
  const { op } = apn
  const [adding, setAdding] = useState(false)
  const [form, setForm] = useState(EMPTY_FORM)
  const locked = op !== null

  async function addProfile() {
    if (locked) return
    // Freeze the draft being submitted.
    const draft = form
    await apn.run('add', async () => {
      try {
        await api.apnAdd({
          profilename: draft.name,
          wanapn: draft.apn,
          username: draft.user,
          password: draft.pass,
          pppAuthMode: draft.auth,
          pdpType: draft.pdp,
        })
      } catch (e) {
        // Keep the open form and every field so the user can correct and retry.
        toastError(e, t('Failed to add profile'))
        return
      }
      setAdding(false)
      setForm(EMPTY_FORM)
      await apn.readBack()
    })
  }

  if (!adding) {
    return (
      <Button variant="primary" onClick={() => setAdding(true)} disabled={locked}>
        {t('Add APN profile')}
      </Button>
    )
  }
  return (
    <Card title={t('Add APN profile')}>
      <div className="grid grid-cols-1 gap-2.5 lg:grid-cols-2">
        <Field label={t('Profile name')}>
          <Input value={form.name} onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))} placeholder={t('My Carrier')} />
        </Field>
        <Field label="APN">
          <Input value={form.apn} onChange={(e) => setForm((f) => ({ ...f, apn: e.target.value }))} placeholder="internet" />
        </Field>
        <Field label={t('Username')}>
          <Input value={form.user} onChange={(e) => setForm((f) => ({ ...f, user: e.target.value }))} placeholder={t('(optional)')} />
        </Field>
        <Field label={t('Password')}>
          <Input type="password" autoComplete="new-password" value={form.pass} onChange={(e) => setForm((f) => ({ ...f, pass: e.target.value }))} placeholder={t('(optional)')} />
        </Field>
        <Field label={t('Authentication')}>
          <Select value={form.auth} onChange={(e) => setForm((f) => ({ ...f, auth: parseInt(e.target.value) }))}>
            <option value={0}>{t('None')}</option>
            <option value={1}>PAP</option>
            <option value={2}>CHAP</option>
            <option value={3}>PAP/CHAP</option>
          </Select>
        </Field>
        <Field label={t('PDP type')}>
          <Select value={form.pdp} onChange={(e) => setForm((f) => ({ ...f, pdp: parseInt(e.target.value) }))}>
            <option value={3}>IPv4v6</option>
            <option value={1}>IPv4</option>
            <option value={2}>IPv6</option>
          </Select>
        </Field>
      </div>
      <div className="mt-3 flex gap-2">
        <Button variant="primary" onClick={addProfile} loading={op === 'add'} disabled={locked || !form.name || !form.apn}>
          {t('Add profile')}
        </Button>
        <Button
          variant="ghost"
          disabled={locked}
          onClick={() => {
            // Cancelling discards the draft, including the password.
            setAdding(false)
            setForm(EMPTY_FORM)
          }}
        >
          {t('Cancel')}
        </Button>
      </div>
    </Card>
  )
}

function Profiles({ apn }: { apn: ApnOwner }) {
  const profiles: PollResult<ApnProfile[]> = apn.profiles
  return (
    <>
      <Card title={t('APN profiles')}>
        {profiles.status === 'loading' ? (
          <Skeleton className="h-20" />
        ) : profiles.status === 'error' || !profiles.data ? (
          <InlineStatus
            kind="error"
            action={{ label: t('Retry'), onClick: profiles.refresh, loading: profiles.refreshing }}
          >
            {profiles.error ? t('APN profiles could not be read: {error}', { error: profiles.error }) : t('APN profiles could not be read.')}
          </InlineStatus>
        ) : (
          <div className="space-y-2">
            {profiles.status === 'stale' && (
              <InlineStatus
                kind="stale"
                action={{ label: t('Retry'), onClick: profiles.refresh, loading: profiles.refreshing }}
              >
                {t('Showing the last profiles read. The latest refresh failed.')}
              </InlineStatus>
            )}
            {profiles.data.length === 0 ? (
              <Empty title={t('No manual APN profiles')} body={t('Add the exact settings supplied by your carrier.')} />
            ) : (
              <ProfileList apn={apn} profiles={profiles.data} />
            )}
          </div>
        )}
      </Card>

      <AddProfile apn={apn} />
    </>
  )
}

export default function ApnTab() {
  const apn = useApn()
  return (
    <div className="space-y-3">
      {apn.unverified && (
        <InlineStatus
          kind="warn"
          action={{ label: t('Re-read APN state'), onClick: () => void apn.readBack(), loading: apn.readingBack }}
        >
          {t('The router accepted the change, but the current APN state could not be confirmed. What is shown may be out of date.')}
        </InlineStatus>
      )}
      {apn.notice && !apn.unverified && <InlineStatus kind="ok">{apn.notice}</InlineStatus>}
      <ApnMode apn={apn} />
      <Profiles apn={apn} />
    </div>
  )
}
