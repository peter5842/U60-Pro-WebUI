// Carrier selection, SMS forwarding and per-device traffic mappers (src/data/cellular.ts).
const { test } = require('node:test')
const assert = require('node:assert/strict')
const { loadTs } = require('./ts-loader.cjs')
const map = loadTs('data/cellular.ts')

test('carrier selection keeps valid rows and known states', () => {
  const s = map.mapCarrierSelection({
    select_mode: 'manual',
    current: { name: 'China Mobile', mcc: 460, mnc: 0 },
    scan: 'done',
    networks: [
      { state: 'current', name: 'CMCC', mccmnc: '46000', rat: '12', rat_label: '5G' },
      { state: 'weird', mccmnc: '46001', rat: '7' },
      { name: 'no code' },
    ],
    register: 'bogus',
  })
  assert.equal(s.select_mode, 'manual')
  assert.equal(s.current.mcc, '460')
  assert.equal(s.networks.length, 2)
  assert.equal(s.networks[1].state, 'unknown')
  assert.equal(s.networks[1].name, '46001')
  assert.equal(s.register, 'idle')
  assert.equal(map.mapCarrierSelection({}).scan, 'idle')
})

test('sms forwarding never assumes a configured target', () => {
  const f = map.mapSmsForward({ enabled: true, channel: 'nope', forwarded: 'x' })
  assert.equal(f.channel, 'bark')
  assert.equal(f.configured, false)
  assert.equal(f.forwarded, 0)
  assert.equal(map.mapSmsForward({ channel: 'telegram', configured: true }).channel, 'telegram')
})

test('client traffic rows need a MAC', () => {
  const r = map.mapClientTraffic({ since: '2026-10-01 09:00:00', clients: [{ mac: 'AA:BB:CC:DD:EE:FF', down_bytes: 5 }, { ip: 'x' }] })
  assert.equal(r.clients.length, 1)
  assert.deepEqual(r.clients[0], { mac: 'AA:BB:CC:DD:EE:FF', ip: undefined, up_bytes: 0, down_bytes: 5, up_rate: 0, down_rate: 0 })
})
