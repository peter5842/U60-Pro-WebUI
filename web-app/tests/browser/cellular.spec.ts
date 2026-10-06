import type { Page } from '@playwright/test'
import { test, expect } from './support/harness'
import { openApp } from './support/app'
import * as fx from './support/fixtures'

// Carrier selection, SMS forwarding and per-device traffic. Synthetic agent only.

const card = (page: Page, title: string) => page.locator('section', { has: page.getByRole('heading', { name: title, exact: true }) })

const NETWORKS = [
  { state: 'current', name: 'Synthetic Mobile', mccmnc: '00101', rat: '12', rat_label: '5G' },
  { state: 'available', name: 'Other Net', mccmnc: '00102', rat: '7', rat_label: '4G' },
  { state: 'forbidden', name: 'Closed Net', mccmnc: '00103', rat: '7', rat_label: '4G' },
]

test('a network search is confirmed, polled, and a network can be registered', async ({ page, agent }) => {
  let state = fx.carriers()
  agent.on('GET', '/api/cell/operators', () => ({ data: state }))
  agent.on('POST', '/api/cell/operators/scan', () => {
    state = fx.carriers({ scan: 'done', networks: NETWORKS })
    return { data: fx.carriers({ scan: 'scanning' }) }
  })
  agent.on('POST', '/api/cell/operators/select', () => {
    state = fx.carriers({ select_mode: 'manual', scan: 'done', networks: NETWORKS, register: 'success', current: { name: 'Other Net' } })
    return { data: fx.carriers({ select_mode: 'manual', scan: 'done', networks: NETWORKS, register: 'registering' }) }
  })
  await openApp(page, { group: 'signal', tab: 'Mode & Locking' })
  const c = card(page, 'Carrier selection')
  await c.getByRole('button', { name: 'Search networks' }).click()
  await page.getByRole('dialog').getByRole('button', { name: 'Search' }).click()
  await expect(c.getByText('Other Net')).toBeVisible({ timeout: 10_000 })
  // The serving and forbidden networks cannot be picked.
  const rows = c.getByRole('listitem')
  await expect(rows.filter({ hasText: 'Closed Net' }).getByRole('button', { name: 'Register' })).toBeDisabled()
  await expect(rows.filter({ hasText: 'Synthetic Mobile' }).getByRole('button', { name: 'Register' })).toBeDisabled()

  await rows.filter({ hasText: 'Other Net' }).getByRole('button', { name: 'Register' }).click()
  await page.getByRole('dialog').getByRole('button', { name: 'Register' }).click()
  await expect(c.getByRole('button', { name: 'Back to automatic' })).toBeVisible({ timeout: 10_000 })
  expect(agent.requests({ method: 'POST', path: '/api/cell/operators/select' }).map((r) => r.body)).toEqual([{ mccmnc: '00102', rat: '7' }])
})

test('SMS forwarding stores a key without ever showing it back', async ({ page, agent }) => {
  agent.on('PUT', '/api/sms/forward', (req) => {
    const b = req.body as Record<string, unknown>
    return { data: fx.smsForward({ channel: b.channel ?? 'bark', configured: true, target_hint: 'AbCd…', enabled: b.enabled === true }) }
  })
  await openApp(page, { group: 'modem', tab: 'SMS' })
  const c = card(page, 'SMS forwarding')
  await expect(c.getByRole('switch', { name: 'SMS forwarding' })).toBeDisabled()
  await c.getByRole('button', { name: 'Set up' }).click()
  await c.getByLabel('Bark key or URL').fill('AbCdEfGhIjKlMn')
  await c.getByRole('button', { name: 'Save' }).click()
  await expect(c.getByText('Bark · AbCd…')).toBeVisible()
  await expect(c.getByText('AbCdEfGhIjKlMn')).toHaveCount(0)
  expect(agent.requests({ method: 'PUT', path: '/api/sms/forward' }).map((r) => r.body)).toEqual([
    { channel: 'bark', target: 'AbCdEfGhIjKlMn', chat_id: '', via_proxy: false },
  ])
})

test('per-device traffic shows next to each device', async ({ page, agent }) => {
  agent.on('GET', '/api/network/clients/traffic', {
    data: fx.clientTraffic({ clients: [{ mac: '02:00:5E:00:00:01', ip: '192.168.0.101', up_bytes: 2_000_000, down_bytes: 3_000_000_000, up_rate: 0, down_rate: 125_000 }] }),
  })
  await openApp(page, { group: 'network', tab: 'Clients' })
  const c = card(page, 'Device controls')
  await expect(c.getByText('↓ 3.0 GB · ↑ 2.0 MB · 1.0 Mbps')).toBeVisible()
  await expect(c.getByText('Internet traffic counted since 2026-10-01 09:00:00')).toBeVisible()
})
