// Settings backup file handling (src/features/system/backupView.ts).
const { test } = require('node:test')
const assert = require('node:assert/strict')
const { loadTs } = require('./ts-loader.cjs')
const view = loadTs('features/system/backupView.ts')

test('only our backups are accepted, and only known sections offered', () => {
  assert.equal(view.parseBackup('not json').ok, false)
  assert.equal(view.parseBackup('{"format":"other"}').ok, false)
  assert.equal(view.parseBackup('{"format":"u60-pro-webui-backup","sections":{}}').ok, false)
  const p = view.parseBackup(JSON.stringify({ format: 'u60-pro-webui-backup', created: '2026-10-06 23:30:00', sections: { sleep: { minutes: -1 }, watchdog: null, mystery: {} } }))
  assert.equal(p.ok, true)
  assert.deepEqual(p.sections, ['sleep'])
  assert.equal(p.created, '2026-10-06 23:30:00')
})

test('file name uses the router date', () => {
  assert.equal(view.backupFileName('2026-10-06 23:30:00'), 'u60-pro-settings-20261006.json')
  assert.equal(view.backupFileName(undefined, new Date(2026, 0, 2)), 'u60-pro-settings-20260102.json')
})
