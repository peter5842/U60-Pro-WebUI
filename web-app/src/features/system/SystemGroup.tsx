import { useState } from 'react'
import { t } from '../../i18n'
import { TabPanel, Tabs } from '../../ui/Tabs'
import MetricsTab from './MetricsTab'
import ToolsTab from './ToolsTab'
import SettingsTab from './SettingsTab'

type Tab = 'metrics' | 'tools' | 'settings'

export default function SystemGroup({ onLogout }: { onLogout: () => void }) {
  const [tab, setTab] = useState<Tab>('metrics')

  return (
    <div className="space-y-4">
      <div>
        <h1 className="hidden font-display text-2xl font-semibold tracking-[-0.015em] text-ink lg:block">{t('System')}</h1>
        <p className="lg:mt-0.5 text-body text-ink2">{t('Health metrics, diagnostic tools and device controls')}</p>
      </div>

      <Tabs
        tabs={[
          { id: 'metrics', label: t('Metrics') },
          { id: 'tools', label: t('Tools') },
          { id: 'settings', label: t('Settings') },
        ]}
        active={tab}
        onChange={setTab}
        label={t('System sections')}
        idBase="system"
      />

      <TabPanel idBase="system" id={tab} className="space-y-4">
        {tab === 'metrics' && <MetricsTab />}
        {tab === 'tools' && <ToolsTab />}
        {tab === 'settings' && <SettingsTab onLogout={onLogout} />}
      </TabPanel>
    </div>
  )
}
