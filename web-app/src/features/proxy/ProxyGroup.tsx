import { useState } from 'react'
import { api } from '../../data/api'
import { usePoll } from '../../data/poll'
import type { ProxyStatus } from '../../types'
import { TabPanel, Tabs } from '../../ui/Tabs'
import NodesTab from './NodesTab'
import OverviewTab from './OverviewTab'
import SettingsTab from './SettingsTab'
import SubscriptionsTab from './SubscriptionsTab'
import { t } from '../../i18n'

type Tab = 'overview' | 'subscriptions' | 'nodes' | 'settings'

export default function ProxyGroup() {
  const [tab, setTab] = useState<Tab>('overview')
  // Shared by Overview and Settings; fast while visible so rates stay live.
  const status = usePoll<ProxyStatus>('proxy-status', api.proxyStatus, tab === 'overview' ? 3000 : 10_000)

  return (
    <div className="space-y-4">
      <div>
        <h1 className="hidden font-display text-2xl font-semibold tracking-[-0.015em] text-ink lg:block">{t('Proxy')}</h1>
        <p className="lg:mt-0.5 text-body text-ink2">{t('mihomo subscriptions, nodes and LAN routing')}</p>
      </div>

      <Tabs
        tabs={[
          { id: 'overview', label: t('Overview') },
          { id: 'subscriptions', label: t('Subscriptions') },
          { id: 'nodes', label: t('Nodes') },
          { id: 'settings', label: t('Settings') },
        ]}
        active={tab}
        onChange={setTab}
        label={t('Proxy sections')}
        idBase="proxy"
      />

      <TabPanel idBase="proxy" id={tab} className="space-y-4">
        {tab === 'overview' && <OverviewTab status={status} />}
        {tab === 'subscriptions' && <SubscriptionsTab />}
        {tab === 'nodes' && <NodesTab />}
        {tab === 'settings' && <SettingsTab status={status} />}
      </TabPanel>
    </div>
  )
}
