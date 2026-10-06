import type { Page } from '@playwright/test'
import { test, expect } from './support/harness'
import { openApp } from './support/app'
import * as fx from './support/fixtures'

// Network > Clients: rename, disconnect and the Wi-Fi block list. Synthetic agent only.

const controls = (page: Page) => page.locator('section', { has: page.getByRole('heading', { name: 'Device controls', exact: true }) })
const row = (page: Page, text: string) => controls(page).getByRole('listitem').filter({ hasText: text })

test('rename validates first, then sends the name once and shows it', async ({ page, agent }) => {
  const named = fx.clients() as { clients: Record<string, unknown>[] }
  agent.on('PUT', '/api/network/clients/name', (req) => {
    const body = req.body as { mac: string; name: string }
    named.clients = named.clients.map((c) => (c.mac === body.mac ? { ...c, name: body.name } : c))
    agent.on('GET', '/api/network/clients', { data: named })
    return { data: body }
  })
  await openApp(page, { group: 'network', tab: 'Clients' })
  await row(page, 'synthetic-phone').getByRole('button', { name: 'Rename' }).click()
  const input = controls(page).getByLabel('Name for 02:00:5E:00:00:02')
  await input.fill('a;b')
  await controls(page).getByRole('button', { name: 'Save' }).click()
  await expect(controls(page).getByText(/Quotes and shell symbols/)).toBeVisible()
  expect(agent.mutations()).toEqual([])

  await input.fill('Kitchen tablet')
  await input.press('Enter')
  await expect(row(page, 'Kitchen tablet')).toBeVisible()
  expect(agent.requests({ method: 'PUT', path: '/api/network/clients/name' }).map((r) => r.body)).toEqual([
    { mac: '02:00:5E:00:00:02', name: 'Kitchen tablet' },
  ])
})

test('USB-C clients can be renamed but not disconnected or blocked', async ({ page }) => {
  await openApp(page, { group: 'network', tab: 'Clients' })
  const usb = row(page, 'synthetic-usb-host')
  await expect(usb.getByRole('button', { name: 'Rename' })).toBeVisible()
  await expect(usb.getByRole('button', { name: 'Disconnect' })).toHaveCount(0)
  await expect(usb.getByRole('button', { name: 'Block' })).toHaveCount(0)
})

test('blocking is confirmed, then listed with Unblock', async ({ page, agent }) => {
  agent.on('PUT', '/api/network/blocklist', (req) => {
    const body = req.body as { mac: string; blocked: boolean }
    return { data: fx.blocklist({ blocked: body.blocked ? [{ mac: body.mac }] : [] }) }
  })
  await openApp(page, { group: 'network', tab: 'Clients' })
  await row(page, 'synthetic-phone').getByRole('button', { name: 'Block' }).click()
  await expect(page.getByRole('dialog')).toContainText('02:00:5E:00:00:02')
  await page.getByRole('dialog').getByRole('button', { name: 'Block' }).click()
  await expect(controls(page).getByText('Blocked from Wi-Fi (1)')).toBeVisible()

  await controls(page).getByRole('button', { name: 'Unblock' }).click()
  await expect(controls(page).getByText('Blocked from Wi-Fi (0)')).toBeVisible()
  expect(agent.requests({ method: 'PUT', path: '/api/network/blocklist' }).map((r) => r.body)).toEqual([
    { mac: '02:00:5E:00:00:02', blocked: true },
    { mac: '02:00:5E:00:00:02', blocked: false },
  ])
})
