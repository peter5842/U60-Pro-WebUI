import type { Page } from '@playwright/test'
import { test, expect } from './support/harness'
import { openApp } from './support/app'

// System > Settings: settings backup and restore. Synthetic agent only.

const card = (page: Page) => page.locator('section', { has: page.getByRole('heading', { name: 'Backup and restore', exact: true }) })

const DOC = {
  format: 'u60-pro-webui-backup',
  version: 1,
  created: '2026-10-06 23:30:00',
  firmware: 'SYNTHETIC_FW',
  sections: { sleep: { minutes: 30 }, reboot_schedule: { enabled: false }, watchdog: { enabled: false } },
}

test('a backup downloads as a dated JSON file', async ({ page, agent }) => {
  agent.on('GET', '/api/system/backup', { data: DOC })
  await openApp(page, { group: 'system', tab: 'Settings' })
  const download = page.waitForEvent('download')
  await card(page).getByRole('button', { name: 'Download backup' }).click()
  expect((await download).suggestedFilename()).toBe('u60-pro-settings-20261006.json')
})

test('restore offers the sections in the file and sends the chosen ones with X-Confirm', async ({ page, agent }) => {
  agent.on('POST', '/api/system/restore', (req) => {
    expect(req.headers['x-confirm']).toBe('true')
    return { data: { results: { sleep: { status: 'ok' }, reboot_schedule: { status: 'skipped' }, watchdog: { status: 'failed', message: 'could not ping' } } } }
  })
  await openApp(page, { group: 'system', tab: 'Settings' })
  const c = card(page)
  await c.getByLabel('Backup file').setInputFiles({ name: 'b.json', mimeType: 'application/json', buffer: Buffer.from(JSON.stringify(DOC)) })
  await expect(c.getByText('Backup from 2026-10-06 23:30:00 · SYNTHETIC_FW')).toBeVisible()
  await c.getByLabel('Scheduled reboot').uncheck()
  await c.getByRole('button', { name: 'Restore', exact: true }).click()
  await page.getByRole('dialog').getByRole('button', { name: 'Restore' }).click()
  await expect(c.getByText('could not ping')).toBeVisible()
  await expect(c.getByText('Restored')).toBeVisible()
  const sent = agent.requests({ method: 'POST', path: '/api/system/restore' }).map((r) => (r.body as { only: string[] }).only)
  expect(sent).toEqual([['sleep', 'watchdog']])
})

test('a file that is not a backup is rejected before anything is sent', async ({ page, agent }) => {
  await openApp(page, { group: 'system', tab: 'Settings' })
  await card(page).getByLabel('Backup file').setInputFiles({ name: 'x.json', mimeType: 'application/json', buffer: Buffer.from('{"hello":1}') })
  await expect(card(page).getByText('This is not a settings backup from this dashboard.')).toBeVisible()
  expect(agent.mutations()).toEqual([])
})
