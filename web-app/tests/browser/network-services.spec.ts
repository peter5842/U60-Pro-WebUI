import type { Page } from '@playwright/test'
import { test, expect } from './support/harness'
import { openApp } from './support/app'
import * as fx from './support/fixtures'

// Port rules, DMZ, static DHCP and the connection watchdog. Synthetic agent only.

const card = (page: Page, title: string) => page.locator('section', { has: page.getByRole('heading', { name: title, exact: true }) })

test('a port forward is validated, added and deleted', async ({ page, agent }) => {
  let rules: Record<string, unknown>[] = []
  agent.on('POST', '/api/router/port-forwards', (req) => {
    const b = req.body as Record<string, unknown>
    rules = [{ id: 'cfg1', kind: b.kind, ip: b.ip, external_start: b.external_start, external_end: b.external_end ?? b.external_start, proto: b.proto, comment: b.comment }]
    return { data: fx.portRules({ rules }) }
  })
  agent.on('POST', '/api/router/port-forwards/delete', () => ({ data: fx.portRules({ rules: [] }) }))
  await openApp(page, { group: 'network', tab: 'Ports' })
  const c = card(page, 'Port forwarding')
  await c.getByRole('button', { name: 'Add rule' }).click()
  await c.getByLabel('Device IP').fill('192.168.1.20')
  await c.getByLabel('First port').fill('8000')
  await c.getByLabel('Last port (optional)').fill('8010')
  await c.getByLabel('Name').fill('nas box')
  await c.getByRole('button', { name: 'Add rule' }).click()
  await expect(c.getByText(/Not in the router’s network/)).toBeVisible()
  await expect(c.getByText(/no spaces/)).toBeVisible()
  expect(agent.mutations()).toEqual([])

  await c.getByLabel('Device IP').fill('192.168.0.20')
  await c.getByLabel('Name').fill('nas')
  await c.getByRole('button', { name: 'Add rule' }).click()
  await expect(c.getByText('8000-8010 → 192.168.0.20')).toBeVisible()
  expect(agent.requests({ method: 'POST', path: '/api/router/port-forwards' }).map((r) => r.body)).toEqual([
    { kind: 'forward', ip: '192.168.0.20', proto: 'both', comment: 'nas', external_start: 8000, external_end: 8010 },
  ])

  await c.getByRole('button', { name: 'Delete nas' }).click()
  await page.getByRole('dialog').getByRole('button', { name: 'Delete' }).click()
  await expect(c.getByText('No port rules')).toBeVisible()
  expect(agent.requests({ method: 'POST', path: '/api/router/port-forwards/delete' }).map((r) => r.body)).toEqual([{ kind: 'forward', id: 'cfg1' }])
})

test('DMZ needs a LAN address and a confirmation', async ({ page, agent }) => {
  agent.on('PUT', '/api/router/firewall', (req) => ({ data: fx.firewall(req.body as Record<string, unknown>) }))
  await openApp(page, { group: 'network', tab: 'Ports' })
  const c = card(page, 'UPnP, DMZ and remote access')
  await c.getByRole('switch', { name: 'DMZ' }).click()
  await expect(c.getByText('Enter an IPv4 address such as 192.168.0.20')).toBeVisible()
  await c.getByLabel('DMZ host').fill('192.168.0.30')
  await c.getByRole('switch', { name: 'DMZ' }).click()
  await page.getByRole('dialog').getByRole('button', { name: 'Enable DMZ' }).click()
  await expect(c.getByRole('switch', { name: 'DMZ' })).toBeChecked()
  expect(agent.requests({ method: 'PUT', path: '/api/router/firewall' }).map((r) => r.body)).toEqual([{ dmz_enabled: true, dmz_ip: '192.168.0.30' }])
})

test('a fixed address can be picked from the connected devices', async ({ page, agent }) => {
  agent.on('POST', '/api/router/dhcp-bindings', (req) => {
    const b = req.body as { mac: string; ip: string }
    return { data: fx.dhcpBindings({ bindings: [{ id: 'cfg9', mac: b.mac.toUpperCase(), ip: b.ip }] }) }
  })
  await openApp(page, { group: 'network', tab: 'Router' })
  const c = card(page, 'Fixed IP addresses (static DHCP)')
  await c.getByRole('button', { name: 'Add fixed address' }).click()
  await c.getByLabel('Pick a connected device').selectOption({ index: 1 })
  await c.getByRole('button', { name: 'Add', exact: true }).click()
  await expect(c.getByText(/after a restart/)).toBeVisible()
  const sent = agent.requests({ method: 'POST', path: '/api/router/dhcp-bindings' }).map((r) => r.body)
  expect(sent).toEqual([{ mac: '02:00:5E:00:00:01', ip: '192.168.0.101' }])
})

test('a watchdog the router cannot confirm stays off and says why', async ({ page, agent }) => {
  agent.on('PUT', '/api/router/watchdog', { error: 'the router could not ping 10.255.255.1, so the watchdog stayed off', status: 409 })
  await openApp(page, { group: 'system', tab: 'Settings' })
  const c = card(page, 'Connection watchdog')
  await c.getByRole('switch', { name: 'Connection watchdog' }).click()
  await c.getByLabel('Address to ping').fill('10.255.255.1')
  await c.getByRole('button', { name: 'Save' }).click()
  await expect(page.getByText(/could not ping 10\.255\.255\.1/)).toBeVisible()
  expect(agent.requests({ method: 'PUT', path: '/api/router/watchdog' }).map((r) => r.body)).toEqual([
    { enabled: true, host: '10.255.255.1', interval_minutes: 2, failures: 3 },
  ])
})
