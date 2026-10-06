const { test } = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')
const ts = require('typescript')
// These fixtures evaluate one module in isolation; i18n resolves to English.
const i18nStub = {
  t: (text, vars) => (vars ? text.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? String(vars[k]) : m)) : text),
}

function load() {
  const context = { exports: {}, require: (spec) => (spec.endsWith('i18n') ? i18nStub : {}) }
  const source = fs.readFileSync(path.join(__dirname, '../src/app/alerts.ts'), 'utf8')
  vm.runInNewContext(ts.transpileModule(source, { compilerOptions: {
    module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022,
  } }).outputText, context)
  return context.exports
}
const { deriveConditions, dismiss, endEpisodes, isDismissed } = load()

const fresh = { sampled_at_ms: 1, age_ms: 500, ttl_ms: 1000, stale: false, error: null }
const stale = (age_ms) => ({ ...fresh, age_ms, stale: true, error: 'ubus timeout' })
const home = (over = {}) => ({
  signal: null, battery: { percent: 80, plugged: true, charging: true, temperature_c: 30 },
  speed: null, device: null, wan: null, wan6: null, cpu: null, memory: null, usage: null,
  thermal: { cpu_temp_c: 50 }, charge_control_error: null, sources: { signal: fresh }, ...over,
})
// Values built inside the VM context have its realm's prototypes; compare plain copies.
const plain = (v) => JSON.parse(JSON.stringify(v))
const visible = (dismissals, conditions) => plain(conditions.active.filter((a) => !isDismissed(dismissals, a)))

test('alert identity carries no ages or readings', () => {
  const a = deriveConditions(home({ sources: { signal: stale(4000) } }), null).active[0]
  const b = deriveConditions(home({ sources: { signal: stale(9000) } }), null).active[0]
  assert.equal(a.id, 'source:signal')
  assert.equal(a.id, b.id)
  assert.equal(a.title, b.title)
  assert.notEqual(a.detail, b.detail)
})

test('a dismissed alert stays dismissed while its age changes', () => {
  const first = deriveConditions(home({ sources: { signal: stale(4000) } }), null)
  let d = dismiss(new Map(), first.active[0])
  const later = deriveConditions(home({ sources: { signal: stale(19000) } }), null)
  d = endEpisodes(d, later.resolved)
  assert.deepEqual(visible(d, later), [])
})

test('after an observed recovery the same failure shows again', () => {
  let d = dismiss(new Map(), deriveConditions(home({ sources: { signal: stale(4000) } }), null).active[0])
  d = endEpisodes(d, deriveConditions(home(), null).resolved)
  const recurrence = deriveConditions(home({ sources: { signal: stale(3000) } }), null)
  assert.equal(visible(endEpisodes(d, recurrence.resolved), recurrence).length, 1)
})

test('a dismissed warning that escalates to an error shows again', () => {
  const warm = deriveConditions(home({ battery: { percent: 80, plugged: true, temperature_c: 46 } }), null)
  const d = dismiss(new Map(), warm.active.find((a) => a.id === 'battery:temperature'))
  const hot = deriveConditions(home({ battery: { percent: 80, plugged: true, temperature_c: 51 } }), null)
  const shown = visible(endEpisodes(d, hot.resolved), hot)
  assert.deepEqual(shown.map((a) => [a.id, a.level]), [['battery:temperature', 'error']])
})

test('an unknown observation is not treated as recovery', () => {
  const hot = deriveConditions(home({ battery: { percent: 80, plugged: true, temperature_c: 46 } }), null)
  let d = dismiss(new Map(), hot.active[0])
  // Battery missing, then the whole dashboard failing: neither proves the temperature fell.
  d = endEpisodes(d, deriveConditions(home({ battery: null }), null).resolved)
  d = endEpisodes(d, deriveConditions(null, 'Timed out').resolved)
  assert.equal(d.get('battery:temperature'), 'warning')
  const again = deriveConditions(home({ battery: { percent: 80, plugged: true, temperature_c: 47 } }), null)
  assert.deepEqual(visible(endEpisodes(d, again.resolved), again), [])
})

test('dismissing one source leaves another failing source visible', () => {
  const both = deriveConditions(home({ sources: { signal: stale(4000), battery: stale(4000) } }), null)
  const d = dismiss(new Map(), both.active.find((a) => a.id === 'source:signal'))
  assert.deepEqual(visible(d, both).map((a) => a.id), ['source:battery'])
})

test('a dashboard refresh failure recovers only after a successful poll', () => {
  const failed = deriveConditions(home(), 'Timed out reaching the agent')
  assert.equal(failed.active[0].id, 'dashboard:refresh')
  assert.equal(failed.resolved.has('dashboard:refresh'), false)
  assert.equal(deriveConditions(home(), null).resolved.has('dashboard:refresh'), true)
})

test('endEpisodes returns the same map when nothing recovered', () => {
  const d = new Map([['source:signal', 'warning']])
  assert.equal(endEpisodes(d, new Set(['battery:low'])), d)
})
