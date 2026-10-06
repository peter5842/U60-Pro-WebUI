import type { Locator, Page } from '@playwright/test'
import { test, expect, type MockAgent } from './support/harness'
import { openApp } from './support/app'

/**
 * Shared-primitive accessibility: confirm dialog, tabs, focus treatment, live regions.
 * Everything runs against the synthetic agent; the "reboot" below is a mocked POST.
 */

async function openRebootDialog(page: Page, agent: MockAgent) {
  agent.on('POST', '/api/device/reboot', { data: {} })
  await openApp(page, { group: 'system', tab: 'Settings' })
  const reboot = page.getByRole('button', { name: 'Reboot', exact: true })
  await reboot.click()
  const dialog = page.getByRole('dialog', { name: 'Reboot the device?' })
  await expect(dialog).toBeVisible()
  return { reboot, dialog }
}

// The dev server serves the app's own module instance at this URL, so calling confirm() from the page
// reaches the mounted ConfirmHost. Kept out of the TypeScript module graph on purpose.
interface FeedbackModule {
  confirm: (o: Record<string, unknown>) => Promise<boolean>
}
const FEEDBACK_MODULE: string = '/src/ui/feedback.tsx'

const insideDialog = (page: Page) => page.evaluate(() => !!document.activeElement?.closest('dialog'))

test.describe('confirm dialog', () => {
  test('is a native modal dialog with associated title and description, Cancel focused first', async ({ page, agent }) => {
    const { dialog } = await openRebootDialog(page, agent)
    expect(await dialog.evaluate((d: HTMLDialogElement) => d.tagName === 'DIALOG' && d.open && d.matches(':modal'))).toBe(true)
    // Accessible name comes from aria-labelledby; description from aria-describedby.
    const wiring = await dialog.evaluate((d) => {
      const text = (attr: string) =>
        (d.getAttribute(attr) ?? '')
          .split(' ')
          .map((id) => document.getElementById(id)?.textContent ?? '')
          .join(' ')
      return { title: text('aria-labelledby'), desc: text('aria-describedby') }
    })
    expect(wiring.title).toBe('Reboot the device?')
    expect(wiring.desc).toContain('connections will drop')
    await expect(page.getByRole('button', { name: 'Cancel' })).toBeFocused()
    expect(agent.mutations()).toEqual([])
  })

  test('Tab and Shift+Tab cycle inside the dialog in both directions', async ({ page, agent }) => {
    const { dialog } = await openRebootDialog(page, agent)
    const cancel = dialog.getByRole('button', { name: 'Cancel' })
    const confirm = dialog.getByRole('button', { name: 'Reboot' })
    await expect(cancel).toBeFocused()
    const forward = [confirm, cancel, confirm, cancel]
    for (const next of forward) {
      await page.keyboard.press('Tab')
      await expect(next).toBeFocused()
      expect(await insideDialog(page)).toBe(true)
    }
    for (const next of [confirm, cancel, confirm]) {
      await page.keyboard.press('Shift+Tab')
      await expect(next).toBeFocused()
      expect(await insideDialog(page)).toBe(true)
    }
    expect(agent.mutations()).toEqual([])
  })

  test('background content is inert while open', async ({ page, agent }) => {
    await openRebootDialog(page, agent)
    // The page behind the modal cannot be hit: a point over the "Shut down" button lands on the dialog's scrim.
    const hit = await page.evaluate(() => {
      const el = [...document.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Shut down')!
      const r = el.getBoundingClientRect()
      const top = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2)
      return { onDialog: !!top?.closest('dialog'), hitBackground: top === el }
    })
    expect(hit).toEqual({ onDialog: true, hitBackground: false })
    expect(agent.mutations()).toEqual([])
  })

  test('Escape resolves false, sends nothing and restores focus to the opener', async ({ page, agent }) => {
    const { reboot, dialog } = await openRebootDialog(page, agent)
    await page.keyboard.press('Escape')
    await expect(dialog).toBeHidden()
    await expect(reboot).toBeFocused()
    expect(agent.mutations()).toEqual([])
  })

  test('Cancel resolves false and restores focus', async ({ page, agent }) => {
    const { reboot, dialog } = await openRebootDialog(page, agent)
    await dialog.getByRole('button', { name: 'Cancel' }).click()
    await expect(dialog).toBeHidden()
    await expect(reboot).toBeFocused()
    expect(agent.mutations()).toEqual([])
  })

  test('backdrop click resolves false; a click inside the dialog does nothing', async ({ page, agent }) => {
    const { dialog } = await openRebootDialog(page, agent)
    await dialog.getByRole('heading', { name: 'Reboot the device?' }).click()
    await dialog.getByText('connections will drop').click()
    await expect(dialog).toBeVisible()
    expect(agent.mutations()).toEqual([])

    await page.mouse.click(4, 4) // the scrim, outside the panel
    await expect(dialog).toBeHidden()
    expect(agent.mutations()).toEqual([])
  })

  test('double Confirm sends exactly one request', async ({ page, agent }) => {
    const { dialog } = await openRebootDialog(page, agent)
    const confirm = dialog.getByRole('button', { name: 'Reboot' })
    // Two synchronous clicks on the same element: the second must be a no-op.
    await confirm.evaluate((b: HTMLButtonElement) => {
      b.click()
      b.click()
    })
    await expect.poll(() => agent.mutations().length).toBe(1)
    await page.waitForTimeout(300)
    expect(agent.mutations().map((r) => `${r.method} ${r.path}`)).toEqual(['POST /api/device/reboot'])
    await expect(dialog).toBeHidden()
  })

  test('an overlapping confirm() resolves false and leaves the open dialog alone', async ({ page, agent }) => {
    const { dialog } = await openRebootDialog(page, agent)
    const second = await page.evaluate(async (mod) => {
      const m = (await import(/* @vite-ignore */ mod)) as FeedbackModule
      return m.confirm({ title: 'Second confirmation' })
    }, FEEDBACK_MODULE)
    expect(second).toBe(false)
    await expect(dialog).toBeVisible()
    await expect(page.getByRole('dialog', { name: 'Second confirmation' })).toHaveCount(0)
    expect(agent.mutations()).toEqual([])
  })

  test('renders details, effect and recovery, and focuses Cancel for connection kind', async ({ page }) => {
    await openApp(page)
    await page.evaluate(async (mod) => {
      const m = (await import(/* @vite-ignore */ mod)) as FeedbackModule
      void m.confirm({
        title: 'Turn off 5 GHz Wi-Fi?',
        kind: 'connection',
        confirmLabel: 'Turn off',
        details: [
          { label: 'Operation', value: 'Disable radio' },
          { label: 'Radio', value: '5 GHz' },
        ],
        consequence: 'Devices on this band will disconnect.',
        recovery: 'Reconnect to the 2.4 GHz network.',
      })
    }, FEEDBACK_MODULE)
    const dialog = page.getByRole('dialog', { name: 'Turn off 5 GHz Wi-Fi?' })
    await expect(dialog).toBeVisible()
    await expect(dialog.getByRole('button', { name: 'Cancel' })).toBeFocused()
    await expect(dialog.locator('dl')).toContainText('5 GHz')
    await expect(dialog).toContainText('Devices on this band will disconnect.')
    await expect(dialog).toContainText('Reconnect to the 2.4 GHz network.')
    await page.keyboard.press('Escape')
    await expect(dialog).toBeHidden()
  })

  test('restores focus to main when the opener has been removed', async ({ page, agent }) => {
    await openApp(page)
    await page.evaluate(async (mod) => {
      const btn = document.createElement('button')
      btn.id = 'temp-opener'
      btn.textContent = 'temp'
      document.body.appendChild(btn)
      btn.focus()
      const m = (await import(/* @vite-ignore */ mod)) as FeedbackModule
      void m.confirm({ title: 'Opener gone?' })
      btn.remove()
    }, FEEDBACK_MODULE)
    await expect(page.getByRole('dialog', { name: 'Opener gone?' })).toBeVisible()
    await page.keyboard.press('Escape')
    await expect(page.getByRole('dialog')).toHaveCount(0)
    expect(await page.evaluate(() => document.activeElement?.tagName)).toBe('MAIN')
    expect(agent.mutations()).toEqual([])
  })
})

test.describe('tabs', () => {
  test('tablist is named; one tab stop; tabs and panel are wired by ids', async ({ page }) => {
    await openApp(page, { group: 'network' })
    const list = page.getByRole('tablist', { name: 'Network sections' })
    await expect(list).toBeVisible()
    const tabs = list.getByRole('tab')
    await expect(tabs).toHaveCount(4)
    await expect(tabs.nth(0)).toHaveAttribute('aria-selected', 'true')
    await expect(tabs.nth(1)).toHaveAttribute('aria-selected', 'false')
    const stops = await tabs.evaluateAll((els) => els.map((e) => e.getAttribute('tabindex')))
    expect(stops).toEqual(['0', '-1', '-1', '-1'])
    const panel = page.getByRole('tabpanel', { name: 'Clients' })
    await expect(panel).toBeVisible()
    await expect(panel).toHaveAttribute('tabindex', '0')
    const wired = await tabs.nth(0).evaluate((t) => ({
      controls: t.getAttribute('aria-controls'),
      panelLabelledBy: document.getElementById(t.getAttribute('aria-controls') ?? '')?.getAttribute('aria-labelledby'),
      id: t.id,
    }))
    expect(wired.panelLabelledBy).toBe(wired.id)
    // Inactive panels are not in the DOM.
    await expect(page.getByRole('tabpanel')).toHaveCount(1)
  })

  test('arrows wrap and move focus only; Enter and Space activate; Home/End jump', async ({ page }) => {
    await openApp(page, { group: 'network' })
    const list = page.getByRole('tablist', { name: 'Network sections' })
    const [clients, wifi, router, ports] = ['Clients', 'Wi-Fi', 'Router', 'Ports'].map((n) => list.getByRole('tab', { name: n }))

    await clients.focus()
    await page.keyboard.press('ArrowRight')
    await expect(wifi).toBeFocused()
    // Manual activation: focus moved, selection did not.
    await expect(clients).toHaveAttribute('aria-selected', 'true')
    await expect(wifi).toHaveAttribute('aria-selected', 'false')
    expect(await wifi.getAttribute('tabindex')).toBe('0')
    expect(await clients.getAttribute('tabindex')).toBe('-1')

    await page.keyboard.press('Enter')
    await expect(wifi).toHaveAttribute('aria-selected', 'true')
    await expect(page.getByRole('tabpanel', { name: 'Wi-Fi' })).toBeVisible()

    await page.keyboard.press('ArrowRight')
    await expect(router).toBeFocused()
    await page.keyboard.press('ArrowRight')
    await expect(ports).toBeFocused()
    await page.keyboard.press('ArrowRight') // wraps to the first
    await expect(clients).toBeFocused()
    await page.keyboard.press('ArrowLeft') // wraps to the last
    await expect(ports).toBeFocused()
    await page.keyboard.press('Home')
    await expect(clients).toBeFocused()
    await page.keyboard.press('End')
    await expect(ports).toBeFocused()
    await page.keyboard.press('ArrowLeft')
    await expect(router).toBeFocused()
    await expect(wifi).toHaveAttribute('aria-selected', 'true') // still: nothing activated yet
    await page.keyboard.press('Space')
    await expect(router).toHaveAttribute('aria-selected', 'true')
    await expect(page.getByRole('tabpanel', { name: 'Router' })).toBeVisible()
  })

  test('Tab leaves the strip and re-enters on the selected tab', async ({ page }) => {
    await openApp(page, { group: 'network' })
    const list = page.getByRole('tablist', { name: 'Network sections' })
    await list.getByRole('tab', { name: 'Clients' }).focus()
    await page.keyboard.press('ArrowRight')
    await page.keyboard.press('Tab')
    // Out of the tablist (into the panel or beyond), never onto another tab.
    expect(await page.evaluate(() => document.activeElement?.getAttribute('role'))).not.toBe('tab')
    await page.keyboard.press('Shift+Tab')
    await expect(list.getByRole('tab', { name: 'Clients' })).toBeFocused()
  })
})

// ── Focus treatment ───────────────────────────────────────────────────────────

interface FocusReading {
  style: string
  width: number
  offset: number
  color: [number, number, number]
  accent: [number, number, number]
  bg: [number, number, number]
}

/** Computed outline of the focused element, plus the colour it sits on (first opaque ancestor). */
function readFocus(page: Page): Promise<FocusReading> {
  return page.evaluate(() => {
    const el = document.activeElement as HTMLElement
    const cs = getComputedStyle(el)
    const parse = (v: string): [number, number, number, number] => {
      const m = v.match(/[\d.]+/g)?.map(Number) ?? [0, 0, 0, 0]
      return [m[0], m[1], m[2], m[3] ?? 1]
    }
    let p: HTMLElement | null = el.parentElement
    let bg: [number, number, number, number] = [0, 0, 0, 0]
    while (p) {
      bg = parse(getComputedStyle(p).backgroundColor)
      if (bg[3] > 0.99) break
      p = p.parentElement
    }
    const accent = document.createElement('i')
    accent.style.color = 'rgb(var(--accent))'
    el.parentElement!.appendChild(accent)
    const a = parse(getComputedStyle(accent).color)
    accent.remove()
    const c = parse(cs.outlineColor)
    return {
      style: cs.outlineStyle,
      width: parseFloat(cs.outlineWidth),
      offset: parseFloat(cs.outlineOffset),
      color: [c[0], c[1], c[2]],
      accent: [a[0], a[1], a[2]],
      bg: [bg[0], bg[1], bg[2]],
    }
  })
}

function contrast(a: number[], b: number[]) {
  const lum = (rgb: number[]) => {
    const [r, g, bl] = rgb.map((v) => {
      const c = v / 255
      return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4
    })
    return 0.2126 * r + 0.7152 * g + 0.0722 * bl
  }
  const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x)
  return (hi + 0.05) / (lo + 0.05)
}

async function expectVisibleFocus(page: Page, target: Locator) {
  await target.focus()
  const r = await readFocus(page)
  expect(r.style).toBe('solid')
  expect(r.width).toBe(2)
  expect(r.offset).toBeGreaterThanOrEqual(2)
  expect(r.color).toEqual(r.accent) // opaque token colour, not a 55 % alpha blend
  expect(contrast(r.color, r.bg)).toBeGreaterThanOrEqual(3)
}

for (const theme of ['light', 'dark'] as const) {
  test(`keyboard focus ring is visible on a filled button and a field (${theme})`, async ({ page, agent }) => {
    agent.on('POST', '/api/device/reboot', { data: {} })
    await openApp(page, { group: 'system', tab: 'Settings', theme })
    await page.keyboard.press('Tab') // switch the page to keyboard modality
    await expectVisibleFocus(page, page.getByRole('button', { name: 'Reboot', exact: true })) // solid danger fill

    await page.getByRole('button', { name: 'Network', exact: true }).first().click()
    await page.getByRole('tab', { name: 'Router' }).click()
    await page.keyboard.press('Tab')
    const field = page.getByLabel('LAN IP')
    await expectVisibleFocus(page, field)
    expect(await field.evaluate((e) => getComputedStyle(e).outlineStyle)).not.toBe('none')
  })
}

// ── Live regions ──────────────────────────────────────────────────────────────

test.describe('toast live regions', () => {
  test('polite status and assertive alert hosts are mounted and empty at startup', async ({ page }) => {
    await openApp(page)
    const status = page.locator('[data-toast-region="polite"]')
    const alert = page.locator('[data-toast-region="assertive"]')
    await expect(status).toHaveCount(1)
    await expect(alert).toHaveCount(1)
    await expect(status).toBeEmpty()
    await expect(alert).toBeEmpty()
  })

  test('errors go to the alert region, persist, and have a named dismiss button', async ({ page, agent }) => {
    agent.on('POST', '/api/device/reboot', { error: 'synthetic reboot failure', status: 500 })
    await openApp(page, { group: 'system', tab: 'Settings' })
    await page.getByRole('button', { name: 'Reboot', exact: true }).click()
    await page.getByRole('dialog').getByRole('button', { name: 'Reboot' }).click()
    const toast = page.locator('[data-toast-region="assertive"]').getByText('synthetic reboot failure')
    await expect(toast).toBeVisible()
    await page.waitForTimeout(5600) // past the success-toast lifetime
    await expect(toast).toBeVisible()
    await page.getByRole('button', { name: 'Dismiss notification' }).click()
    await expect(toast).toBeHidden()
  })

  test('success goes to the polite region and clears itself', async ({ page, agent }) => {
    agent.on('POST', '/api/device/reboot', { data: {} })
    await openApp(page, { group: 'system', tab: 'Settings' })
    await page.getByRole('button', { name: 'Reboot', exact: true }).click()
    await page.getByRole('dialog').getByRole('button', { name: 'Reboot' }).click()
    const toast = page.locator('[data-toast-region="polite"]').getByText('Reboot command sent')
    await expect(toast).toBeVisible()
    await expect(toast).toBeHidden({ timeout: 8000 })
  })
})
