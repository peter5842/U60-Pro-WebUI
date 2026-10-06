import { useState } from 'react'
import { api } from '../../data/api'
import { usePoll } from '../../data/poll'
import type { ProxyStatus } from '../../types'
import { TabPanel, Tabs } from '../../ui/Tabs'
import NodesTab from './NodesTab'
import OverviewTab from './OverviewTab'
import SettingsTab from './SettingsTab'
import SubscriptionsTab from './SubscriptionsTab'

type Tab = 'overview' | 'subscriptions' | 'nodes' | 'settings'

export default function ProxyGroup() {
  const [tab, setTab] = useState<Tab>('overview')
  // Shared by Overview and Settings; fast while visible so rates stay live.
  const status = usePoll<ProxyStatus>('proxy-status', api.proxyStatus, tab === 'overview' ? 3000 : 10_000)

  return (
    <div className="space-y-4">
      <div>
        <h1 className="hidden font-display text-2xl font-semibold tracking-[-0.015em] text-ink lg:block">Proxy</h1>
        <p className="lg:mt-0.5 text-body text-ink2">mihomo subscriptions, nodes and LAN routing</p>
      </div>

      <Tabs
        tabs={[
          { id: 'overview', label: 'Overview' },
          { id: 'subscriptions', label: 'Subscriptions' },
          { id: 'nodes', label: 'Nodes' },
          { id: 'settings', label: 'Settings' },
        ]}
        active={tab}
        onChange={setTab}
        label="Proxy sections"
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
