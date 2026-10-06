import type { Page } from '@playwright/test'
import { test, expect } from './support/harness'
import { openApp } from './support/app'
import * as fx from './support/fixtures'

// System > Settings: device sleep and scheduled reboot. Synthetic agent only.

const card = (page: Page) => page.locator('section', { has: page.getByRole('heading', { name: 'Sleep and reboot', exact: true }) })

test('changing device sleep sends the minutes once', async ({ page, agent }) => {
  agent.on('PUT', '/api/device/sleep', (req) => ({ data: fx.sleep(req.body as Record<string, unknown>) }))
  await openApp(page, { group: 'system', tab: 'Settings' })
  const select = card(page).getByLabel('Device sleep')
  await expect(select).toHaveValue('-1')
  await select.selectOption({ label: '30 min' })
  await expect(select).toHaveValue('30')
  expect(agent.requests({ method: 'PUT', path: '/api/device/sleep' }).map((r) => r.body)).toEqual([{ minutes: 30 }])
})

test('a reboot schedule sends only what changed', async ({ page, agent }) => {
  agent.on('PUT', '/api/device/reboot-schedule', (req) => ({ data: fx.rebootSchedule(req.body as Record<string, unknown>) }))
  await openApp(page, { group: 'system', tab: 'Settings' })
  const c = card(page)
  await expect(c.getByText('Not scheduled')).toBeVisible()
  await c.getByRole('switch', { name: 'Scheduled reboot' }).click()
  await c.getByRole('radio', { name: 'Every N days' }).click()
  await c.getByLabel('Every (days)').selectOption('3')
  await c.getByLabel('Reboot time').fill('04:30')
  await c.getByRole('button', { name: 'Save' }).click()
  await expect(c.getByText('Every 3 days after boot between 04:30 and 06:30')).toBeVisible()
  expect(agent.requests({ method: 'PUT', path: '/api/device/reboot-schedule' }).map((r) => r.body)).toEqual([
    { enabled: true, mode: 'interval', interval_days: 3, hour: 4, minute: 30, window_hours: 2 },
  ])
})
