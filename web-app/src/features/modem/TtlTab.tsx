import { useState } from 'react'
import { api } from '../../data/api'
import { useResource } from '../../data/poll'
import { t } from '../../i18n'
import type { TtlStatus } from '../../types'
import { Button, Field, Input } from '../../ui/controls'
import { toastError } from '../../ui/feedback'
import { Card, Chip, InlineStatus, Skeleton } from '../../ui/primitives'
import { parseTtl, ttlFamilies, ttlInputValue, ttlState } from './ttlView'

export default function TtlTab() {
  const status = useResource<TtlStatus>('ttl-status', api.ttlStatus)
  const [draft, setDraft] = useState<string | null>(null)
  const [fieldError, setFieldError] = useState<string | undefined>()
  const [busy, setBusy] = useState(false)
  // The router accepted a change but the status could not be re-read afterwards.
  const [unverified, setUnverified] = useState(false)

  const state = ttlState(status.data)
  const ttlText = ttlInputValue(draft, status.data)

  /** Re-read after an accepted change; a failed read-back is reported, never hidden. */
  async function readBack() {
    try {
      status.mutate(await api.ttlStatus())
      setUnverified(false)
    } catch {
      setUnverified(true)
    }
  }

  async function applyTtl() {
    if (busy) return
    const parsed = parseTtl(ttlText)
    if (!parsed.ok) {
      setFieldError(parsed.error)
      return
    }
    setFieldError(undefined)
    setBusy(true)
    try {
      await api.ttlSet(parsed.ttl)
      setDraft(null)
      await readBack()
    } catch (e) {
      toastError(e, t('Failed to set TTL'))
    } finally {
      setBusy(false)
    }
  }

  async function clearTtl() {
    if (busy) return
    setBusy(true)
    try {
      await api.ttlClear()
      setFieldError(undefined)
      await readBack()
    } catch (e) {
      toastError(e, t('Failed to clear TTL'))
    } finally {
      setBusy(false)
    }
  }

  const input = (placeholder?: string) => (
    <div className="w-28">
      <Field label={t('TTL value')} hint={t('1 to 255')} error={fieldError}>
        {(ids) => (
          <Input
            id={ids.id}
            type="number"
            inputMode="numeric"
            min={1}
            max={255}
            step={1}
            value={ttlText}
            placeholder={placeholder}
            onChange={(e) => {
              setDraft(e.target.value)
              setFieldError(undefined)
            }}
            aria-describedby={ids.describedBy}
            aria-invalid={ids.invalid || undefined}
          />
        )}
      </Field>
    </div>
  )

  let body
  if (status.status === 'loading') {
    body = <Skeleton className="h-16" />
  } else if (!status.data) {
    body = (
      <InlineStatus kind="error" action={{ label: t('Retry'), onClick: status.refresh, loading: status.refreshing }}>
        {status.error ? t('TTL status could not be read: {error}', { error: status.error }) : t('TTL status could not be read.')}
      </InlineStatus>
    )
  } else {
    const families = ttlFamilies(status.data)
    body = (
      <div className="space-y-3">
        {status.status === 'stale' && (
          <InlineStatus kind="stale" action={{ label: t('Retry'), onClick: status.refresh, loading: status.refreshing }}>
            {t('Showing the last TTL status read. The latest refresh failed.')}
          </InlineStatus>
        )}
        {unverified && (
          <InlineStatus kind="warn" action={{ label: t('Re-read status'), onClick: () => void readBack() }}>
            {t('The router accepted the change, but the TTL status could not be re-read. What is shown may be out of date.')}
          </InlineStatus>
        )}
        {state === 'unknown' ? (
          <InlineStatus kind="warn" action={{ label: t('Retry'), onClick: status.refresh, loading: status.refreshing }}>
            {t('The router did not report whether TTL clamping is on.')}
          </InlineStatus>
        ) : state === 'active' ? (
          <div className="flex flex-wrap items-end gap-x-4 gap-y-3">
            <div className="flex items-center gap-2 pb-2">
              <span className="h-2 w-2 rounded-full bg-ok" aria-hidden="true" />
              <span className="tnum font-mono text-body font-semibold text-ok">{t('Active (TTL={ttl})', { ttl: status.data.ttl_value ?? '?' })}</span>
              {families && <Chip tone="default">{families}</Chip>}
            </div>
            <div className="flex items-end gap-2">
              {input()}
              <Button variant="outline" onClick={applyTtl} loading={busy}>
                {t('Update')}
              </Button>
              <Button variant="ghost" onClick={clearTtl} disabled={busy}>
                {t('Disable')}
              </Button>
            </div>
          </div>
        ) : (
          <div className="flex flex-wrap items-end gap-2">
            {input('65')}
            <Button variant="primary" onClick={applyTtl} loading={busy} disabled={!ttlText}>
              {t('Enable clamping')}
            </Button>
          </div>
        )}
      </div>
    )
  }

  return (
    <Card title={t('TTL clamping')}>
      <div className="space-y-3">
        <p className="text-meta text-ink2">
          {t(
            'Overrides the TTL / hop limit on LAN ingress traffic to prevent carrier tethering detection. Applied immediately and persists across reboots.',
          )}
        </p>
        {body}
      </div>
    </Card>
  )
}
