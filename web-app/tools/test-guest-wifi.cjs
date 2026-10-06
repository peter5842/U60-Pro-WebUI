// Guest Wi-Fi editor helpers (src/features/network/guestView.ts) and the guest mapper.
const { test } = require('node:test')
const assert = require('node:assert/strict')
const { loadTs } = require('./ts-loader.cjs')
const view = loadTs('features/network/guestView.ts')

const off = { ssid: 'Guest', enabled_2g: false, enabled_5g: false, security: 'none', has_key: true, hidden: false, active_minutes: 240 }

test('turning on sends both bands and restarts the timer', () => {
  const d = { ...view.draftFromGuest(off), enabled: true }
  assert.deepEqual({ ...view.buildGuestPatch(d, off) }, { guest_disabled_2g: '0', guest_disabled_5g: '0', guest_active_time: '240' })
  assert.deepEqual({ ...view.buildGuestPatch(view.draftFromGuest(off), off) }, {})
})

test('password is sent only when typed and secured', () => {
  const d = { ...view.draftFromGuest(off), security: 'psk2+ccmp', password: 'visitors-2026' }
  assert.deepEqual({ ...view.buildGuestPatch(d, off) }, { guest_encryption: 'psk2+ccmp', guest_key: 'visitors-2026' })
  assert.deepEqual({ ...view.buildGuestPatch({ ...d, security: 'none' }, off) }, {})
})

test('validation mirrors the agent and the stock rule', () => {
  const base = view.draftFromGuest(off)
  assert.deepEqual(view.validateGuest(base, off), {})
  assert.ok(view.validateGuest({ ...base, ssid: '' }, off).ssid)
  assert.ok(view.validateGuest({ ...base, ssid: 'a;b' }, off).ssid)
  // Switching an open network to WPA2 needs a new password even if an old key is stored.
  assert.ok(view.validateGuest({ ...base, security: 'sae', password: '' }, off).password)
  assert.ok(view.validateGuest({ ...base, security: 'sae', password: 'short' }, off).password)
  assert.equal(view.validateGuest({ ...base, security: 'sae', password: 'longenough' }, off).password, undefined)
  // Open and unlimited is refused only when the network is on.
  assert.ok(view.validateGuest({ ...base, enabled: true, minutes: 0 }, off).minutes)
  assert.equal(view.validateGuest({ ...base, enabled: false, minutes: 0 }, off).minutes, undefined)
  const secured = { ...off, security: 'psk2+ccmp' }
  assert.equal(view.validateGuest({ ...view.draftFromGuest(secured), enabled: true, minutes: 0 }, secured).minutes, undefined)
})

test('labels', () => {
  assert.equal(view.timeLimitLabel(0), 'No limit')
  assert.equal(view.timeLimitLabel(480), '8 h')
  assert.equal(view.leftLabel(3900), '1 h 05 min left')
  assert.equal(view.leftLabel(20), '1 min left')
  assert.equal(view.securityLabel('sae-mixed'), 'WPA2/WPA3')
})
