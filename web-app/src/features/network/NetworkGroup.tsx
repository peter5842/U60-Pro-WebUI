import { useState } from 'react'
import { t } from '../../i18n'
import { TabPanel, Tabs } from '../../ui/Tabs'
import ClientsTab from './ClientsTab'
import WifiTab from './WifiTab'
import RouterTab from './RouterTab'
import PortsTab from './PortsTab'

type Tab = 'clients' | 'wifi' | 'router' | 'ports'

export default function NetworkGroup() {
  const [tab, setTab] = useState<Tab>('clients')

  return (
    <div className="space-y-4">
      <div>
        <h1 className="hidden font-display text-2xl font-semibold tracking-[-0.015em] text-ink lg:block">{t('Network')}</h1>
        <p className="lg:mt-0.5 text-body text-ink2">{t('Connected clients, Wi-Fi and router settings')}</p>
      </div>

      <Tabs
        tabs={[
          { id: 'clients', label: t('Clients') },
          { id: 'wifi', label: 'Wi-Fi' },
          { id: 'router', label: t('Router') },
          { id: 'ports', label: t('Ports') },
        ]}
        active={tab}
        onChange={setTab}
        label={t('Network sections')}
        idBase="network"
      />

      <TabPanel idBase="network" id={tab} className="space-y-4">
        {tab === 'clients' && <ClientsTab />}
        {tab === 'wifi' && <WifiTab />}
        {tab === 'router' && <RouterTab />}
        {tab === 'ports' && <PortsTab />}
      </TabPanel>
    </div>
  )
}
