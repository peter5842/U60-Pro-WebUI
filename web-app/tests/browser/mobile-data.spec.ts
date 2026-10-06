import { test, expect } from './support/harness'
import { openApp } from './support/app'
import * as fx from './support/fixtures'
import type { Page } from '@playwright/test'

// Modem > Data: mobile data switch and the monthly limit. Synthetic agent only.

const card = (page: Page, title: string) =>
  page.locator('section', { has: page.getByRole('heading', { name: title, exact: true }) })

test.describe('mobile data', () => {
  test('disconnect is confirmed first and sends exactly one PUT', async ({ page, agent }) => {
    agent.on('PUT', '/api/modem/data', () => ({ data: fx.mobileData({ connected: false, connect_status: 'ppp_disconnected', ipv4: null, ipv6: null }) }))
    await openApp(page, { group: 'modem', tab: 'Data' })
    const data = card(page, 'Mobile data')
    await expect(data.getByText('Connected', { exact: true })).toBeVisible()

    await data.getByRole('button', { name: 'Disconnect' }).click()
    await expect(page.getByRole('dialog')).toBeVisible()
    await page.getByRole('dialog').getByRole('button', { name: 'Cancel' }).click()
    expect(agent.mutations()).toEqual([])

    await data.getByRole('button', { name: 'Disconnect' }).click()
    await page.getByRole('dialog').getByRole('button', { name: 'Disconnect' }).click()
    await expect(data.getByText('Disconnected', { exact: true })).toBeVisible()
    await expect(data.getByRole('button', { name: 'Connect' })).toBeVisible()
    expect(agent.requests({ method: 'PUT', path: '/api/modem/data' }).map((r) => r.body)).toEqual([{ connect: false }])
  })

  test('connect needs no confirmation', async ({ page, agent }) => {
    agent.on('GET', '/api/modem/data', { data: fx.mobileData({ connected: false, connect_status: 'ppp_disconnected' }) })
    agent.on('PUT', '/api/modem/data', () => ({ data: fx.mobileData() }))
    await openApp(page, { group: 'modem', tab: 'Data' })
    const data = card(page, 'Mobile data')
    await data.getByRole('button', { name: 'Connect' }).click()
    await expect(data.getByText('Connected', { exact: true })).toBeVisible()
    await expect(page.getByRole('dialog')).toHaveCount(0)
    expect(agent.requests({ method: 'PUT', path: '/api/modem/data' }).map((r) => r.body)).toEqual([{ connect: true }])
  })
})

test.describe('monthly limit', () => {
  test('turning the limit on validates, then sends bytes and the alert share', async ({ page, agent }) => {
    agent.on('PUT', '/api/data-usage/limit', (req) => ({ data: fx.dataLimit(req.body as Record<string, unknown>) }))
    await openApp(page, { group: 'modem', tab: 'Data' })
    const limit = card(page, 'Monthly limit')
    await limit.getByRole('switch', { name: 'Monthly limit' }).click()

    const gb = limit.getByLabel('Limit (GB)')
    await expect(gb).toHaveValue('100')
    await gb.fill('abc')
    await limit.getByRole('button', { name: 'Save' }).click()
    await expect(limit.getByText('Enter a number of GB, e.g. 100')).toBeVisible()
    expect(agent.mutations()).toEqual([])

    await gb.fill('300')
    await limit.getByLabel('Alert at (%)').fill('90')
    await limit.getByRole('button', { name: 'Save' }).click()
    await expect(limit.getByRole('button', { name: 'Save' })).toHaveCount(0)
    expect(agent.requests({ method: 'PUT', path: '/api/data-usage/limit' }).map((r) => r.body)).toEqual([
      { enabled: true, limit_bytes: 322122547200, alert_percent: 90 },
    ])
  })

  test('a time limit set in the stock UI is shown read-only', async ({ page, agent }) => {
    agent.on('GET', '/api/data-usage/limit', { data: fx.dataLimit({ enabled: true, kind: 'time', limit_bytes: null }) })
    await openApp(page, { group: 'modem', tab: 'Data' })
    const limit = card(page, 'Monthly limit')
    await expect(limit.getByText(/connection-time limit is set in the stock web UI/)).toBeVisible()
    await expect(limit.getByRole('switch')).toHaveCount(0)
  })
})
