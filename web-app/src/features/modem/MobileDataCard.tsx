import { useState } from 'react'
import { api } from '../../data/api'
import { useResource } from '../../data/poll'
import { t } from '../../i18n'
import type { MobileDataState } from '../../types'
import { Button } from '../../ui/controls'
import { confirm, toast, toastError } from '../../ui/feedback'
import { Card, Chip, InlineStatus, Row, Skeleton } from '../../ui/primitives'

export default function MobileDataCard() {
  const data = useResource<MobileDataState>('mobile-data', api.mobileData)
  const [busy, setBusy] = useState(false)
  const d = data.data

  async function toggle() {
    if (!d || busy) return
    const connect = !d.connected
    if (!connect) {
      const ok = await confirm({
        title: t('Disconnect mobile data?'),
        body: t('Every device on this router loses internet access until mobile data is connected again.'),
        kind: 'connection',
        confirmLabel: t('Disconnect'),
        consequence: t('Internet access stops for all devices.'),
        recovery: t('The dashboard and the LAN keep working; connect again here.'),
      })
      if (!ok) return
    }
    setBusy(true)
    try {
      const next = await api.mobileDataSet(connect)
      data.mutate(next)
      if (next.connected === connect) toast(connect ? t('Mobile data connected') : t('Mobile data disconnected'))
      else toast(connect ? t('Connecting… the link is not up yet') : t('Disconnecting… the link is still up'), 'err')
    } catch (e) {
      toastError(e, t('Failed to change mobile data'))
      data.refresh()
    } finally {
      setBusy(false)
    }
  }

  let body
  if (data.status === 'loading') body = <Skeleton className="h-16" />
  else if (!d)
    body = (
      <InlineStatus kind="error" action={{ label: t('Retry'), onClick: data.refresh, loading: data.refreshing }}>
        {data.error
          ? t('Mobile data state could not be read: {error}', { error: data.error })
          : t('Mobile data state could not be read.')}
      </InlineStatus>
    )
  else
    body = (
      <div className="space-y-2">
        <Row label={t('Status')} value={<Chip tone={d.connected ? 'ok' : 'default'}>{d.connected ? t('Connected') : t('Disconnected')}</Chip>} />
        {d.ipv4 && <Row label="IPv4" value={d.ipv4} mono />}
        {d.ipv6 && <Row label="IPv6" value={d.ipv6} mono wrap />}
        {d.roaming_allowed !== undefined && <Row label={t('Data roaming')} value={d.roaming_allowed ? t('Allowed') : t('Not allowed')} />}
      </div>
    )

  return (
    <Card
      title={t('Mobile data')}
      action={
        d && (
          <Button size="sm" variant={d.connected ? 'outline' : 'primary'} onClick={() => void toggle()} loading={busy}>
            {d.connected ? t('Disconnect') : t('Connect')}
          </Button>
        )
      }
    >
      {body}
    </Card>
  )
}
