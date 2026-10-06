import type { Page } from '@playwright/test'
import { test, expect, type MockAgent } from './support/harness'
import { openApp } from './support/app'
import * as fx from './support/fixtures'

// PLAN2 R07/R08/R11/U03 — Modem > APN. Every mutation is a mocked request.

type Json = Record<string, unknown>

interface ApnState {
  mode: number
  profiles: Json[]
}

const PROFILES = (activeId: string | null): Json[] => [
  { profilename: 'Synthetic Internet', wanapn: 'internet.example', username: 'syn-user', password: 'syn-secret-pass', pdpType: 3, pppAuthMode: 1, profileId: '1', isEnable: activeId === '1' },
  { profilename: 'Synthetic M2M', wanapn: 'm2m.example', username: '', password: '', pdpType: 1, pppAuthMode: 0, profileId: '2', isEnable: activeId === '2' },
]

/** A stateful APN endpoint set that mimics the agent: activation sets manual mode, then enables the profile. */
function apnAgent(agent: MockAgent, initial: Partial<ApnState> = {}): ApnState {
  const state: ApnState = { mode: 0, profiles: PROFILES(null), ...initial }
  agent.on('GET', '/api/router/apn/mode', () => ({ data: fx.apnMode({ apn_mode: state.mode }) }))
  agent.on('GET', '/api/router/apn/profiles', () => ({ data: fx.apnProfiles({ apnListArray: state.profiles }) }))
  agent.on('PUT', '/api/router/apn/mode', (req) => {
    state.mode = (req.body as { apn_mode: number }).apn_mode
    return { data: {} }
  })
  agent.on('POST', '/api/router/apn/profiles/activate', (req) => {
    const id = (req.body as { profileId: string }).profileId
    state.mode = 1
    state.profiles = state.profiles.map((p) => ({ ...p, isEnable: p.profileId === id }))
    return { data: {} }
  })
  return state
}

const modeGroup = (page: Page) => page.getByRole('radiogroup', { name: 'APN mode' })
const radio = (page: Page, name: string) => modeGroup(page).getByRole('radio', { name })
const checked = (page: Page, name: string) => expect(radio(page, name)).toHaveAttribute('aria-checked', 'true')

test.describe('activation and mode stay coherent', () => {
  test('automatic -> activate a profile -> both panels show manual and Automatic stays selectable', async ({ page, agent }) => {
    apnAgent(agent, { mode: 0 })
    await openApp(page, { group: 'modem', tab: 'APN' })
    await checked(page, 'Automatic')

    await page.getByRole('button', { name: 'Activate Synthetic M2M' }).click()
    const dialog = page.getByRole('dialog', { name: /Activate APN profile "Synthetic M2M"/ })
    await expect(dialog).toBeVisible()
    await expect(dialog).toContainText('Mobile data reconnects')
    await expect(dialog).toContainText('m2m.example')
    await expect(dialog).toContainText('Automatic → Manual')
    // Credentials are never echoed into a confirmation.
    await expect(dialog).not.toContainText('syn-secret-pass')
    await dialog.getByRole('button', { name: 'Activate', exact: true }).click()

    // Mode panel and profile panel both reflect the authoritative state.
    await checked(page, 'Manual')
    await expect(page.getByText('Active', { exact: true })).toHaveCount(1)
    await expect(page.locator('div', { hasText: /^Synthetic M2M/ }).getByText('Active', { exact: true }).first()).toBeVisible()
    await expect(radio(page, 'Automatic')).toBeEnabled()

    expect(agent.requests({ method: 'POST', path: '/api/router/apn/profiles/activate' }).map((r) => r.body)).toEqual([{ profileId: '2' }])
    expect(agent.mutations()).toHaveLength(1)

    // Returning to Automatic is possible: select, Apply, confirm.
    await radio(page, 'Automatic').click()
    await expect(page.getByRole('button', { name: 'Apply' })).toBeEnabled()
    expect(agent.mutations()).toHaveLength(1)
  })

  test('manual -> automatic: draft, Apply, confirm, one PUT, then both panels re-read', async ({ page, agent }) => {
    const state = apnAgent(agent, { mode: 1, profiles: PROFILES('1') })
    await openApp(page, { group: 'modem', tab: 'APN' })
    await checked(page, 'Manual')
    await expect(page.getByRole('button', { name: 'Apply' })).toBeDisabled()

    await radio(page, 'Automatic').click()
    await checked(page, 'Automatic')
    expect(agent.mutations()).toEqual([])

    await page.getByRole('button', { name: 'Apply' }).click()
    const dialog = page.getByRole('dialog', { name: 'Switch APN mode to automatic?' })
    await expect(dialog).toContainText('Manual → Automatic')
    await expect(dialog).toContainText('Mobile data reconnects')
    await dialog.getByRole('button', { name: 'Switch to automatic' }).click()

    await expect(page.getByRole('button', { name: 'Apply' })).toBeDisabled()
    await checked(page, 'Automatic')
    expect(state.mode).toBe(0)
    expect(agent.requests({ method: 'PUT', path: '/api/router/apn/mode' }).map((r) => r.body)).toEqual([{ apn_mode: 0 }])
    expect(agent.mutations()).toHaveLength(1)
    // Coordinated read-back of both resources after the accepted change.
    await expect.poll(() => agent.requests({ method: 'GET', path: '/api/router/apn/mode' }).length).toBe(2)
    await expect.poll(() => agent.requests({ method: 'GET', path: '/api/router/apn/profiles' }).length).toBe(2)
    // In automatic mode the stored selection is not shown as the active APN.
    await expect(page.getByText('Active', { exact: true })).toHaveCount(0)
    await expect(page.getByText('Selected', { exact: true })).toHaveCount(1)
  })
})

test.describe('no accidental submissions', () => {
  test('Cancel in the mode confirmation sends nothing and keeps the draft', async ({ page, agent }) => {
    apnAgent(agent, { mode: 1, profiles: PROFILES('1') })
    await openApp(page, { group: 'modem', tab: 'APN' })
    await radio(page, 'Automatic').click()
    await page.getByRole('button', { name: 'Apply' }).click()
    const dialog = page.getByRole('dialog', { name: 'Switch APN mode to automatic?' })
    await expect(dialog.getByRole('button', { name: 'Cancel' })).toBeFocused()
    await dialog.getByRole('button', { name: 'Cancel' }).click()
    await expect(dialog).toBeHidden()
    expect(agent.mutations()).toEqual([])
    await checked(page, 'Automatic')
  })

  test('Cancel and Escape in the activation confirmation send nothing', async ({ page, agent }) => {
    apnAgent(agent, { mode: 1, profiles: PROFILES('1') })
    await openApp(page, { group: 'modem', tab: 'APN' })
    await page.getByRole('button', { name: 'Activate Synthetic M2M' }).click()
    const dialog = page.getByRole('dialog', { name: /Activate APN profile/ })
    await dialog.getByRole('button', { name: 'Cancel' }).click()
    await expect(dialog).toBeHidden()
    await page.getByRole('button', { name: 'Activate Synthetic M2M' }).click()
    await expect(dialog).toBeVisible()
    await page.keyboard.press('Escape')
    await expect(dialog).toBeHidden()
    expect(agent.mutations()).toEqual([])
  })

  test('arrow keys on the mode radiogroup only change the draft; nothing is sent until Apply is confirmed', async ({ page, agent }) => {
    apnAgent(agent, { mode: 1, profiles: PROFILES('1') })
    await openApp(page, { group: 'modem', tab: 'APN' })
    await radio(page, 'Manual').focus()
    await page.keyboard.press('ArrowLeft')
    await checked(page, 'Automatic')
    await page.keyboard.press('ArrowRight')
    await checked(page, 'Manual')
    await page.keyboard.press('ArrowLeft')
    await checked(page, 'Automatic')
    expect(agent.mutations()).toEqual([])
    await expect(page.getByRole('dialog')).toHaveCount(0)

    await page.getByRole('button', { name: 'Apply' }).click()
    await page.getByRole('dialog').getByRole('button', { name: 'Switch to automatic' }).click()
    await expect(page.getByRole('button', { name: 'Apply' })).toBeDisabled()
    expect(agent.requests({ method: 'PUT', path: '/api/router/apn/mode' }).map((r) => r.body)).toEqual([{ apn_mode: 0 }])
  })

  test('while an operation is pending every conflicting control is locked', async ({ page, agent }) => {
    const state = apnAgent(agent, { mode: 0 })
    const hold = agent.defer('POST', '/api/router/apn/profiles/activate')
    await openApp(page, { group: 'modem', tab: 'APN' })
    await page.getByRole('button', { name: 'Activate Synthetic M2M' }).click()
    await page.getByRole('dialog').getByRole('button', { name: 'Activate', exact: true }).click()
    await hold.waitForRequest()

    await expect(page.getByRole('button', { name: 'Activate Synthetic Internet' })).toBeDisabled()
    await expect(page.getByRole('button', { name: 'Add APN profile' })).toBeDisabled()
    await expect(radio(page, 'Manual')).toBeDisabled()
    await expect(page.getByRole('button', { name: 'Apply' })).toBeDisabled()
    // A deferred reply bypasses apnAgent's handler, so apply the router's side effect here;
    // otherwise the coordinated read-back would (correctly) report Automatic again.
    const id = ((await hold.waitForRequest()).body as { profileId: string }).profileId
    state.mode = 1
    state.profiles = state.profiles.map((p) => ({ ...p, isEnable: p.profileId === id }))
    hold.resolve({})
    await checked(page, 'Manual')
    expect(agent.mutations()).toHaveLength(1)
  })
})

test.describe('honest read and write states', () => {
  test('accepted change with a failed read-back is reported as unverified and is not retried', async ({ page, agent }) => {
    apnAgent(agent, { mode: 1, profiles: PROFILES('1') })
    await openApp(page, { group: 'modem', tab: 'APN' })
    await radio(page, 'Automatic').click()
    // From now on every read fails.
    agent.on('GET', '/api/router/apn/mode', { error: 'synthetic read failure', status: 503 })
    agent.on('GET', '/api/router/apn/profiles', { error: 'synthetic read failure', status: 503 })
    await page.getByRole('button', { name: 'Apply' }).click()
    await page.getByRole('dialog').getByRole('button', { name: 'Switch to automatic' }).click()

    await expect(page.getByText(/accepted the change, but the current APN state could not be confirmed/)).toBeVisible()
    await checked(page, 'Automatic') // the accepted value, not a rollback
    expect(agent.requests({ method: 'PUT', path: '/api/router/apn/mode' })).toHaveLength(1)
    expect(agent.mutations()).toHaveLength(1)

    // Re-reading is explicit and succeeds once reads work again.
    agent.on('GET', '/api/router/apn/mode', () => ({ data: fx.apnMode({ apn_mode: 0 }) }))
    agent.on('GET', '/api/router/apn/profiles', () => ({ data: fx.apnProfiles() }))
    await page.getByRole('button', { name: 'Re-read APN state' }).click()
    await expect(page.getByText(/could not be confirmed/)).toBeHidden()
    expect(agent.mutations()).toHaveLength(1)
  })

  test('a profiles read failure shows an error with Retry, never "No manual APN profiles"; mode stays independent', async ({ page, agent }) => {
    apnAgent(agent, { mode: 1, profiles: PROFILES('1') })
    agent.fail('GET', '/api/router/apn/profiles', { status: 503, error: 'synthetic outage' })
    await openApp(page, { group: 'modem', tab: 'APN' })
    await expect(page.getByText(/APN profiles could not be read/)).toBeVisible()
    await expect(page.getByText('No manual APN profiles')).toHaveCount(0)
    await checked(page, 'Manual')

    agent.on('GET', '/api/router/apn/profiles', () => ({ data: fx.apnProfiles() }))
    await page.getByRole('button', { name: 'Retry' }).click()
    await expect(page.getByText('Synthetic Internet')).toBeVisible()
  })

  test('a genuinely empty profile list is an empty state, not an error', async ({ page, agent }) => {
    apnAgent(agent, { mode: 0, profiles: [] })
    await openApp(page, { group: 'modem', tab: 'APN' })
    await expect(page.getByText('No manual APN profiles')).toBeVisible()
    await expect(page.getByText(/could not be read/)).toHaveCount(0)
  })

  test('a missing apn_mode is unknown, not automatic: no radio is checked', async ({ page, agent }) => {
    apnAgent(agent)
    agent.on('GET', '/api/router/apn/mode', { data: {} })
    await openApp(page, { group: 'modem', tab: 'APN' })
    await expect(page.getByText(/does not recognise/)).toBeVisible()
    await expect(radio(page, 'Automatic')).toHaveAttribute('aria-checked', 'false')
    await expect(radio(page, 'Manual')).toHaveAttribute('aria-checked', 'false')
    await radio(page, 'Automatic').click()
    await expect(page.getByRole('button', { name: 'Apply' })).toBeEnabled()
    expect(agent.mutations()).toEqual([])
  })

  test('a failed activation reports the error, sends one request and shows the router state', async ({ page, agent }) => {
    apnAgent(agent, { mode: 0 })
    agent.on('POST', '/api/router/apn/profiles/activate', { error: 'synthetic activation failure', status: 503 })
    await openApp(page, { group: 'modem', tab: 'APN' })
    await page.getByRole('button', { name: 'Activate Synthetic M2M' }).click()
    await page.getByRole('dialog').getByRole('button', { name: 'Activate', exact: true }).click()
    await expect(page.getByText('synthetic activation failure')).toBeVisible()
    // The backend rolled back to automatic; the panels show what the router reports.
    await checked(page, 'Automatic')
    await expect(page.getByText('Active', { exact: true })).toHaveCount(0)
    expect(agent.requests({ method: 'POST', path: '/api/router/apn/profiles/activate' })).toHaveLength(1)
    await expect(page.getByText(/accepted the change/)).toHaveCount(0)
  })

  test('a failed profile save keeps the form draft, including the password, and sends no extra request', async ({ page, agent }) => {
    apnAgent(agent)
    agent.on('POST', '/api/router/apn/profiles', { error: 'synthetic add failure', status: 400 })
    await openApp(page, { group: 'modem', tab: 'APN' })
    await page.getByRole('button', { name: 'Add APN profile' }).click()
    await page.getByLabel('Profile name').fill('Draft Profile')
    await page.getByRole('textbox', { name: 'APN', exact: true }).fill('draft.example')
    // Credentials are only offered (and accepted by the agent) with PAP/CHAP.
    await page.getByLabel('Authentication').selectOption({ label: 'PAP' })
    await page.getByLabel('Password').fill('draft-secret')
    await page.getByRole('button', { name: 'Add profile' }).click()

    await expect(page.getByText('synthetic add failure')).toBeVisible()
    await expect(page.getByLabel('Profile name')).toHaveValue('Draft Profile')
    await expect(page.getByRole('textbox', { name: 'APN', exact: true })).toHaveValue('draft.example')
    await expect(page.getByLabel('Password')).toHaveValue('draft-secret')
    const posts = agent.requests({ method: 'POST', path: '/api/router/apn/profiles' })
    expect(posts).toHaveLength(1)
    expect(posts[0].body).toMatchObject({ profilename: 'Draft Profile', wanapn: 'draft.example', password: 'draft-secret', pppAuthMode: 1 })
    expect(agent.mutations()).toHaveLength(1)
  })

  test('deleting a profile keeps its confirmation and drops the redundant success toast', async ({ page, agent }) => {
    const state = apnAgent(agent, { mode: 1, profiles: PROFILES('1') })
    agent.on('POST', '/api/router/apn/profiles/delete', (req) => {
      state.profiles = state.profiles.filter((p) => p.profileId !== (req.body as { profileId: string }).profileId)
      return { data: {} }
    })
    await openApp(page, { group: 'modem', tab: 'APN' })
    await page.getByRole('button', { name: 'Delete Synthetic M2M' }).click()
    await page.getByRole('dialog', { name: /Delete APN profile "Synthetic M2M"/ }).getByRole('button', { name: 'Delete' }).click()
    await expect(page.getByText('Synthetic M2M')).toHaveCount(0)
    await expect(page.locator('[data-toast]')).toHaveCount(0)
    expect(agent.requests({ method: 'POST', path: '/api/router/apn/profiles/delete' }).map((r) => r.body)).toEqual([{ profileId: '2' }])
  })
})

test.describe('editing a profile', () => {
  test('an inactive profile is saved in place without a confirmation', async ({ page, agent }) => {
    const state = apnAgent(agent, { mode: 1, profiles: PROFILES('1') })
    agent.on('PUT', '/api/router/apn/profiles', (req) => {
      const b = req.body as Record<string, unknown>
      state.profiles = state.profiles.map((p) => (p.profileId === b.profileId ? { ...p, profilename: b.profilename as string, wanapn: b.wanapn as string } : p))
      return { data: {} }
    })
    await openApp(page, { group: 'modem', tab: 'APN' })
    await page.getByRole('button', { name: 'Edit Synthetic M2M' }).click()
    await page.getByRole('textbox', { name: 'APN', exact: true }).fill('m2m.changed')
    await page.getByRole('button', { name: 'Save', exact: true }).click()
    await expect(page.getByText(/m2m\.changed/)).toBeVisible()
    await expect(page.getByRole('dialog')).toHaveCount(0)
    const puts = agent.requests({ method: 'PUT', path: '/api/router/apn/profiles' })
    expect(puts).toHaveLength(1)
    expect(puts[0].body).toMatchObject({ profileId: '2', profilename: 'Synthetic M2M', wanapn: 'm2m.changed' })
  })

  test('editing the active profile asks first', async ({ page, agent }) => {
    apnAgent(agent, { mode: 1, profiles: PROFILES('1') })
    agent.on('PUT', '/api/router/apn/profiles', { data: {} })
    await openApp(page, { group: 'modem', tab: 'APN' })
    await page.getByRole('button', { name: 'Edit Synthetic Internet' }).click()
    await page.getByRole('button', { name: 'Save', exact: true }).click()
    await expect(page.getByRole('dialog')).toContainText('Change the active APN profile?')
    await page.getByRole('dialog').getByRole('button', { name: 'Cancel' }).click()
    expect(agent.mutations()).toEqual([])
  })
})
