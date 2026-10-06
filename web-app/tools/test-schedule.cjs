// Sleep timer and reboot schedule: mappers (src/data/schedule.ts) and card helpers.
const { test } = require('node:test')
const assert = require('node:assert/strict')
const { loadTs } = require('./ts-loader.cjs')
const view = loadTs('features/system/scheduleView.ts')
const map = loadTs('data/schedule.ts')

const base = { enabled: true, mode: 'weekly', weekday: 2, interval_days: 1, hour: 2, minute: 0, window_hours: 2 }

test('sleep mapper keeps never (-1) and rejects garbage', () => {
  assert.deepEqual(map.mapSleep({ minutes: -1, options: [-1, 5, 'x', 0, 120] }), { minutes: -1, options: [-1, 5, 120] })
  assert.throws(() => map.mapSleep({ minutes: 'soon' }))
  assert.throws(() => map.mapSleep({ minutes: 0 }))
})

test('reboot schedule mapper needs every field', () => {
  assert.deepEqual(map.mapRebootSchedule(base), base)
  assert.throws(() => map.mapRebootSchedule({ ...base, hour: 24 }))
  assert.throws(() => map.mapRebootSchedule({ ...base, mode: 'daily' }))
  assert.throws(() => map.mapRebootSchedule({ ...base, window_hours: undefined }))
})

test('labels', () => {
  assert.equal(view.sleepLabel(-1), 'Never')
  assert.equal(view.sleepLabel(30), '30 min')
  assert.equal(view.sleepLabel(120), '2 h')
  assert.equal(view.weekdayName(0), 'Sunday')
  assert.equal(view.formatTime(2, 5), '02:05')
})

test('time parsing', () => {
  assert.deepEqual(view.parseTime('03:30'), { hour: 3, minute: 30 })
  assert.deepEqual(view.parseTime('3:05'), { hour: 3, minute: 5 })
  assert.equal(view.parseTime('24:00'), undefined)
  assert.equal(view.parseTime(''), undefined)
})

test('summary sentence', () => {
  assert.equal(view.scheduleSummary({ ...base, enabled: false }), 'Not scheduled')
  assert.equal(view.scheduleSummary(base), 'Every Tuesday between 02:00 and 04:00')
  assert.equal(view.scheduleSummary({ ...base, window_hours: 0 }), 'Every Tuesday at 02:00')
  assert.equal(view.scheduleSummary({ ...base, mode: 'interval' }), 'Every day between 02:00 and 04:00')
  assert.equal(view.scheduleSummary({ ...base, mode: 'interval', interval_days: 3, hour: 23, window_hours: 2 }),
    'Every 3 days after boot between 23:00 and 01:00')
})

test('only changed fields are sent', () => {
  assert.deepEqual(view.scheduleChanges(base, { ...base }), {})
  assert.deepEqual(view.scheduleChanges(base, { ...base, enabled: false, hour: 4 }), { enabled: false, hour: 4 })
  assert.deepEqual(view.scheduleChanges(base, { ...base, mode: 'interval' }), { mode: 'interval', hour: 2, minute: 0, window_hours: 2 })
})
