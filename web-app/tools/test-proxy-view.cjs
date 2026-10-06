// Proxy (mihomo) mappers and view logic. Fixtures follow agent/src/mihomo/mod.rs payloads.
const { test } = require('node:test')
const assert = require('node:assert/strict')
const { loadTs } = require('./ts-loader.cjs')
const view = loadTs('features/proxy/proxyView.ts')
const map = loadTs('data/proxy.ts')

test('status mapper validates fields and flattens the selected route', () => {
  const s = map.mapProxyStatus({
    installed: true,
    version: 'v1.19.32',
    running: true,
    enabled: true,
    mode: 'rule',
    preset: 'bogus',
    tun: false,
    tun_active: false,
    mixed_port: 7890,
    subscriptions: 2,
    traffic: { up_total: 10, down_total: 20, up_rate: null, down_rate: 'x', connections: 3 },
    route: { group: '节点选择', chain: ['节点选择', '自动选择', 'HK 01'] },
    profile: { id: 'a1b2c3d4', name: 'Main' },
    restarts: 1,
    last_error: '',
  })
  assert.equal(s.preset, undefined)
  assert.equal(s.mode, 'rule')
  assert.deepEqual(s.traffic, { up_total: 10, down_total: 20, up_rate: undefined, down_rate: undefined, connections: 3 })
  assert.deepEqual(s.route, ['节点选择', '自动选择', 'HK 01'])
  assert.equal(view.currentRoute(s), '节点选择 → 自动选择 → HK 01')
  assert.deepEqual(s.profile, { id: 'a1b2c3d4', name: 'Main' })
  assert.equal(map.mapProxyStatus({ profile: { id: 'x' }, route: { chain: [] } }).profile, undefined)
  assert.equal(map.mapProxyStatus({ route: { chain: [] } }).route, undefined)
  assert.equal(s.last_error, undefined)
  assert.deepEqual(view.serviceState(s), { label: 'Running', tone: 'ok' })
})

test('service state distinguishes missing core, failing and stopped', () => {
  const base = map.mapProxyStatus({ installed: true })
  assert.equal(view.serviceState(null).label, 'Unknown')
  assert.equal(view.serviceState({ ...base, installed: false }).label, 'Not installed')
  assert.equal(view.serviceState({ ...base, enabled: true }).label, 'Starting')
  assert.equal(view.serviceState({ ...base, enabled: true, last_error: 'bind failed' }).label, 'Failing')
  assert.equal(view.serviceState(base).label, 'Stopped')
})

test('subscriptions mapper drops malformed rows and empty usage', () => {
  const d = map.mapProxySubscriptions({
    running: true,
    subscriptions: [
      { id: 'a1', name: 'Main', url_masked: 'https://x/…', enabled: true, interval_hours: 24, node_count: 5,
        use_config: true, full_config: true, groups: 15,
        usage: { upload: 1, download: 2, total: 100, expire: 1900000000 } },
      { id: 'a2', name: 'Empty', enabled: false, interval_hours: 0, usage: { upload: null } },
      { name: 'no id' },
      'junk',
    ],
  })
  assert.equal(d.subscriptions.length, 2)
  assert.equal(d.subscriptions[1].usage, undefined)
  assert.equal(d.subscriptions[0].usage.total, 100)
  assert.equal(d.subscriptions[0].use_config, true)
  assert.equal(d.subscriptions[0].groups, 15)
  assert.equal(d.subscriptions[1].use_config, false)
  assert.equal(d.subscriptions[1].full_config, undefined)
})

test('usage percentage, bytes and tone', () => {
  assert.equal(view.usagePct({ upload: 5, download: 15, total: 100 }), 20)
  assert.equal(view.usagePct({ upload: 5 }), undefined)
  assert.equal(view.usedBytes({ total: 100 }), undefined)
  assert.equal(view.usedBytes({ download: 7 }), 7)
  assert.equal(view.usageTone(96), 'bg-danger')
  assert.equal(view.usageTone(85), 'bg-warn')
  assert.equal(view.usageTone(10), 'bg-accent')
})

test('expiry: none, soon, expired', () => {
  const now = 1_800_000_000
  assert.equal(view.expiry({ expire: 0 }, now), undefined)
  assert.equal(view.expiry(undefined, now), undefined)
  const soon = view.expiry({ expire: now + 3 * 86400 + 60 }, now)
  assert.equal(soon.daysLeft, 3)
  assert.equal(soon.tone, 'warn')
  assert.equal(view.expiry({ expire: now + 40 * 86400 }, now).tone, 'default')
  assert.equal(view.expiry({ expire: now - 86400 }, now).tone, 'danger')
})

test('delay labels and tones; timeouts and untested sort last', () => {
  assert.equal(view.delayLabel(undefined), '—')
  assert.equal(view.delayLabel(0), 'timeout')
  assert.equal(view.delayLabel(123), '123 ms')
  assert.equal(view.delayTone(0), 'danger')
  assert.equal(view.delayTone(150), 'ok')
  assert.equal(view.delayTone(300), 'warn')
  const nodes = new Map([
    ['B', { name: 'B', delay: 0, subscription: 'Main' }],
    ['A', { name: 'A', delay: 300, subscription: 'Main' }],
    ['C', { name: 'C', delay: 90, subscription: 'Backup' }],
    ['D', { name: 'D' }],
  ])
  const members = ['B', 'A', '自动选择', 'C', 'D']
  assert.deepEqual(view.visibleMembers(members, nodes, '', 'delay'), ['C', 'A', 'B', '自动选择', 'D'])
  assert.deepEqual(view.visibleMembers(members, nodes, '', 'config'), members)
  assert.deepEqual(view.visibleMembers(members, nodes, 'backup', 'config'), ['C'])
  assert.deepEqual(view.visibleMembers(members, nodes, '自动', 'config'), ['自动选择'])
})

test('group types: only select groups are switchable', () => {
  assert.equal(view.isSelectable({ name: 'g', type: 'Selector', all: [] }), true)
  assert.equal(view.isSelectable({ name: 'g', type: 'URLTest', all: [] }), false)
  assert.equal(view.groupTypeLabel('URLTest'), 'Fastest')
  assert.equal(view.groupTypeLabel('Mystery'), 'Mystery')
  assert.equal(view.groupTypeLabel(undefined), '')
})

test('groups and delay mappers', () => {
  const g = map.mapProxyGroups({
    running: true,
    groups: [{ name: 'PROXY', type: 'Selector', now: 'AUTO', all: ['AUTO', 'DIRECT', 'HK'] }, { now: 'x' }],
    nodes: [{ name: 'HK', delay: 80, subscription_id: 'a1', subscription: 'Main' }, { name: 'JP' }, { delay: 3 }],
  })
  assert.equal(g.groups.length, 1)
  assert.equal(g.nodes.length, 2)
  assert.equal(g.nodes[1].subscription, undefined)
  assert.deepEqual(map.mapProxyDelays({ delays: { HK: 80, JP: 0, bad: 'x' } }), { HK: 80, JP: 0 })
})

test('form validation mirrors the agent', () => {
  assert.deepEqual(view.validateSubscription('Main', 'https://sub.example.com/api?token=x'), {})
  assert.equal(view.validateSubscription('', 'https://a').name, 'Enter a name')
  assert.equal(view.validateSubscription('x'.repeat(33), 'https://a').name, 'At most 32 characters')
  assert.ok(view.validateSubscription('Main', 'ftp://a').url)
  assert.ok(view.validateSubscription('Main', 'https://a/b c').url)
  assert.deepEqual(view.validatePort('7890'), { ok: true, port: 7890 })
  assert.equal(view.validatePort('9090').ok, false)
  assert.equal(view.validatePort('80').ok, false)
  assert.equal(view.validatePort('abc').ok, false)
})

test('relative time', () => {
  const now = Date.parse('2026-10-06T12:00:00Z')
  assert.equal(view.ago('2026-10-06T11:59:30Z', now), 'just now')
  assert.equal(view.ago('2026-10-06T11:30:00Z', now), '30 min ago')
  assert.equal(view.ago('2026-10-06T07:00:00Z', now), '5 h ago')
  assert.equal(view.ago('2026-10-03T12:00:00Z', now), '3 d ago')
  assert.equal(view.ago('garbage', now), undefined)
})
