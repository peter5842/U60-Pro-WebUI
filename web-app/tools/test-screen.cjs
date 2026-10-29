// Geek screen mapper (src/data/screen.ts) against agent/src/screen.rs payloads.
const { test } = require('node:test')
const assert = require('node:assert/strict')
const { loadTs } = require('./ts-loader.cjs')
const map = loadTs('data/screen.ts')

test('a live session and its settings map through', () => {
  const s = map.mapScreen({
    installed: true,
    gesture_listener: true,
    session: { state: 'active', owner: 4242, reason: null, stock_ui: false, parked: true, version: '0.1.0' },
    settings: { brightness: 200, idle_secs: 300, idle_action: 'sleep', theme: 'amber', scanlines: false, start_page: 'cell', gesture: true, auto_open: 'wake' },
  })
  assert.equal(s.installed, true)
  assert.equal(s.state, 'active')
  assert.equal(s.version, '0.1.0')
  assert.equal(s.reason, undefined)
  assert.deepEqual(s.settings, {
    brightness: 200,
    idle_secs: 300,
    idle_action: 'sleep',
    theme: 'amber',
    scanlines: false,
    start_page: 'cell',
    gesture: true,
    auto_open: 'wake',
  })
})

test('missing binary, refusals and junk fall back safely', () => {
  const missing = map.mapScreen({ installed: false, session: null, settings: {} })
  assert.equal(missing.installed, false)
  assert.equal(missing.state, 'unknown')
  assert.equal(missing.settings.auto_open, 'off')
  assert.equal(missing.settings.brightness, 160)
  const refused = map.mapScreen({ installed: true, session: { state: 'refused', reason: 'device is in power-off charging' }, settings: { theme: 'pink', brightness: 999, start_page: 'x' } })
  assert.equal(refused.state, 'refused')
  assert.equal(refused.reason, 'device is in power-off charging')
  assert.equal(refused.settings.theme, 'green')
  assert.equal(refused.settings.brightness, 160)
  assert.equal(refused.settings.start_page, 'home')
  assert.equal(map.mapScreen({ session: { state: 'weird' } }).state, 'unknown')
})
