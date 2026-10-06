import type { Page } from '@playwright/test'
import { test, expect } from './support/harness'
import { openApp } from './support/app'
import * as fx from './support/fixtures'

// Network > Wi-Fi: guest network editor. Synthetic agent only.

const guest = (page: Page) => page.locator('section', { has: page.getByRole('heading', { name: 'Guest network', exact: true }) })

test('turning the guest network on with a password sends one reviewed request', async ({ page, agent }) => {
  agent.on('PUT', '/api/wifi/settings', (req) => {
    const b = req.body as Record<string, string>
    agent.on('GET', '/api/wifi/status', {
      data: fx.wifiStatus({ guest_disabled_2g: b.guest_disabled_2g, guest_disabled_5g: b.guest_disabled_5g, guest_encryption: b.guest_encryption, has_guest_key: true }),
    })
    return { data: { status: 'ok', changed: true } }
  })
  await openApp(page, { group: 'network', tab: 'Wi-Fi' })
  const g = guest(page)
  await expect(g.getByText('Off', { exact: true })).toBeVisible()
  await g.getByRole('button', { name: 'Edit' }).click()
  await g.getByRole('switch', { name: 'Guest network on' }).click()
  await g.getByLabel('Security').selectOption('psk2+ccmp')
  await g.getByRole('button', { name: 'Apply changes' }).click()
  // A new password is required when securing an open network.
  await expect(g.getByText('The password must be 8–63 characters.')).toBeVisible()
  expect(agent.mutations()).toEqual([])

  await g.getByLabel('Password', { exact: true }).fill('visitors-2026')
  await g.getByRole('button', { name: 'Apply changes' }).click()
  await page.getByRole('dialog').getByRole('button', { name: 'Apply changes' }).click()
  await expect(g.getByText('On', { exact: true })).toBeVisible()
  expect(agent.requests({ method: 'PUT', path: '/api/wifi/settings' }).map((r) => r.body)).toEqual([
    { guest_encryption: 'psk2+ccmp', guest_key: 'visitors-2026', guest_disabled_2g: '0', guest_disabled_5g: '0', guest_active_time: '240' },
  ])
})

test('an open guest network cannot be turned on without a time limit', async ({ page, agent }) => {
  await openApp(page, { group: 'network', tab: 'Wi-Fi' })
  const g = guest(page)
  await g.getByRole('button', { name: 'Edit' }).click()
  await g.getByRole('switch', { name: 'Guest network on' }).click()
  await g.getByLabel('Time limit').selectOption({ label: 'No limit' })
  await expect(g.getByText(/An open guest network needs a time limit/)).toBeVisible()
  await g.getByRole('button', { name: 'Apply changes' }).click()
  await expect(page.getByRole('dialog')).toHaveCount(0)
  expect(agent.mutations()).toEqual([])
})
