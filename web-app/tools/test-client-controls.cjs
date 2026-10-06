// Client controls: block list mapper (src/data/clients.ts) and naming helpers.
const { test } = require('node:test')
const assert = require('node:assert/strict')
const { loadTs } = require('./ts-loader.cjs')
const view = loadTs('features/network/clientsView.ts')
const map = loadTs('data/clients.ts')

test('display name prefers the user-set name', () => {
  assert.equal(view.displayName({ name: 'TV', hostname: 'android-1' }), 'TV')
  assert.equal(view.displayName({ hostname: 'android-1' }), 'android-1')
  assert.equal(view.displayName({ name: '', hostname: '' }), undefined)
})

test('names follow the agent rule', () => {
  assert.equal(view.validateClientName('客厅电视'), undefined)
  assert.equal(view.validateClientName('Living room TV'), undefined)
  assert.equal(view.validateClientName(''), 'Enter a name')
  assert.equal(view.validateClientName('x'.repeat(33)), 'At most 32 characters')
  assert.equal(view.validateClientName(' TV'), 'No spaces at the start or end')
  assert.ok(view.validateClientName('a;b'))
  assert.ok(view.validateClientName('a"b'))
})

test('block list mapper drops malformed rows', () => {
  assert.deepEqual(map.mapBlocklist({ blocked: [{ mac: '02:00:00:00:00:01', name: 'TV' }, { name: 'no mac' }, 'x'], max: 32, available: true }), {
    blocked: [{ mac: '02:00:00:00:00:01', name: 'TV' }],
    max: 32,
    available: true,
  })
  assert.deepEqual(map.mapBlocklist({}), { blocked: [], max: 32, available: false })
})
