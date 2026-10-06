// Mobile data and monthly limit: mappers (src/data/wwan.ts) and card helpers.
const { test } = require('node:test')
const assert = require('node:assert/strict')
const { loadTs } = require('./ts-loader.cjs')
const view = loadTs('features/modem/dataLimitView.ts')
const map = loadTs('data/wwan.ts')

test('mobile data mapper never invents a connection', () => {
  assert.deepEqual(map.mapMobileData({ connected: true, ipv4: '10.0.0.2', roaming_allowed: 0 }), {
    connected: true, connect_status: undefined, auto_connect: undefined, roaming_allowed: false,
    ipv4: '10.0.0.2', ipv6: undefined,
  })
  assert.equal(map.mapMobileData({}).connected, false)
})

test('limit mapper keeps time limits apart', () => {
  assert.deepEqual(map.mapDataLimit({ enabled: true, kind: 'data', limit_bytes: 1073741824, alert_percent: 80 }),
    { enabled: true, kind: 'data', limit_bytes: 1073741824, alert_percent: 80 })
  assert.equal(map.mapDataLimit({ kind: 'time' }).kind, 'time')
  assert.equal(map.mapDataLimit({ alert_percent: 0 }).alert_percent, undefined)
})

test('GB parsing uses binary GB like the stock UI', () => {
  assert.deepEqual(view.parseLimitGb('300'), { ok: true, bytes: 322122547200 })
  assert.deepEqual(view.parseLimitGb('1.5'), { ok: true, bytes: 1610612736 })
  assert.equal(view.parseLimitGb('0').ok, false)
  assert.equal(view.parseLimitGb('abc').ok, false)
  assert.equal(view.parseLimitGb('1.234').ok, false)
  assert.equal(view.bytesToGbText(322122547200), '300')
  assert.equal(view.bytesToGbText(1610612736), '1.5')
  assert.equal(view.bytesToGbText(undefined), '')
})

test('alert percentage and usage share', () => {
  assert.deepEqual(view.parseAlertPercent('80'), { ok: true, percent: 80 })
  assert.equal(view.parseAlertPercent('100').ok, false)
  assert.equal(view.parseAlertPercent('8.5').ok, false)
  assert.equal(view.limitPct(50, 200), 25)
  assert.equal(view.limitPct(null, 200), undefined)
  assert.equal(view.limitPct(5, undefined), undefined)
})
