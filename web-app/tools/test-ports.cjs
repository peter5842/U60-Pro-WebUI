// Router network services: mappers (src/data/netsvc.ts) and form validation (portsView, scheduleView).
const { test } = require('node:test')
const assert = require('node:assert/strict')
const { loadTs } = require('./ts-loader.cjs')
const view = loadTs('features/network/portsView.ts')
const sched = loadTs('features/system/scheduleView.ts')
const map = loadTs('data/netsvc.ts')

test('LAN host validation mirrors the agent', () => {
  const v = (ip) => view.validateLanHost(ip, '192.168.0.1', '255.255.255.0')
  assert.equal(v('192.168.0.20'), undefined)
  assert.ok(v('192.168.0.1'))
  assert.ok(v('192.168.0.255'))
  assert.ok(v('192.168.0.0'))
  assert.ok(v('192.168.1.20'))
  assert.ok(v('300.1.1.1'))
  assert.ok(v('abc'))
  // Without the LAN config only the format is checked.
  assert.equal(view.validateLanHost('10.0.0.5'), undefined)
})

test('ports, comments and summaries', () => {
  assert.equal(view.parsePort('8080'), 8080)
  assert.equal(view.parsePort('0'), undefined)
  assert.equal(view.parsePort('65001', 65000), undefined)
  assert.equal(view.validateComment('nas'), undefined)
  assert.ok(view.validateComment('has space'))
  assert.ok(view.validateComment(''))
  assert.equal(view.isReservedPort(32003), true)
  const fwd = { id: 'a', kind: 'forward', ip: '192.168.0.20', external_start: 8000, external_end: 8010, proto: 'both', comment: 'nas' }
  assert.equal(view.ruleSummary(fwd), '8000-8010 → 192.168.0.20')
  assert.equal(view.ruleSummary({ ...fwd, external_end: 8000 }), '8000 → 192.168.0.20')
  assert.equal(view.ruleSummary({ ...fwd, kind: 'mapping', external_end: 8000, internal: 22 }), '8000 → 192.168.0.20:22')
  assert.equal(view.protoLabel('both'), 'TCP+UDP')
})

test('watchdog validation', () => {
  assert.deepEqual(sched.validateWatchdog('223.5.5.5', '5', '3'), {})
  assert.deepEqual(sched.validateWatchdog('www.baidu.com', '2', '1'), {})
  const e = sched.validateWatchdog('bad host', '1', '0')
  assert.ok(e.host && e.interval && e.failures)
  assert.ok(sched.validateWatchdog('999.1.1.1', '5', '3').host)
})

test('mappers drop malformed rows', () => {
  const r = map.mapPortRules({
    forward_enabled: true,
    rules: [
      { id: 'cfg1', kind: 'forward', ip: '192.168.0.20', external_start: 80, external_end: 81, proto: 'tcp', comment: 'web' },
      { id: 'cfg2', kind: 'nope', ip: '192.168.0.20', external_start: 80, external_end: 81 },
      'junk',
    ],
  })
  assert.equal(r.rules.length, 1)
  assert.equal(r.max_per_kind, 20)
  assert.equal(r.mapping_enabled, false)
  const b = map.mapDhcpBindings({ enabled: 1, bindings: [{ id: 'x', mac: '02:00:00:00:00:01', ip: '192.168.0.50' }, { id: 'y' }] })
  assert.equal(b.enabled, true)
  assert.equal(b.bindings.length, 1)
  assert.deepEqual(map.mapWatchdog({ enabled: '1', host: '', interval_minutes: 5 }), { enabled: true, host: undefined, interval_minutes: 5, failures: undefined })
  assert.deepEqual(map.mapClock({ servers: ['a', '', 3] }).servers, ['a'])
})
