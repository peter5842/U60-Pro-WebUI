// Translation coverage: every t('…') literal in src/ has a Chinese entry with the same
// placeholders, t() is only called with literals, and the catalogs hold no stale keys.
const { test } = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')
const { loadTs } = require('./ts-loader.cjs')

const SRC = path.join(__dirname, '../src')
const { ZH, AREAS } = loadTs('i18n/zh/index.ts')
const i18n = loadTs('i18n/index.ts')

function sources(dir = SRC, out = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name)
    if (entry.isDirectory()) {
      if (entry.name !== 'i18n') sources(full, out)
    } else if (/\.tsx?$/.test(entry.name)) {
      out.push(full)
    }
  }
  return out
}

// `t(` not preceded by an identifier character or a dot, then a string or template literal.
const CALL = /(?<![\w$.])t\(\s*/g
const LITERAL = /^(?:'(?:\\.|[^'\\])*'|"(?:\\.|[^"\\])*"|`(?:\\.|[^`\\$])*`)/

function calls() {
  const keys = new Map() // key → first "file:line"
  const nonLiteral = []
  for (const file of sources()) {
    const text = fs.readFileSync(file, 'utf8')
    for (const m of text.matchAll(CALL)) {
      const rest = text.slice(m.index + m[0].length)
      const lit = rest.match(LITERAL)
      const where = `${path.relative(SRC, file)}:${text.slice(0, m.index).split('\n').length}`
      if (!lit) {
        nonLiteral.push(`${where}  t(${rest.slice(0, 40).split('\n')[0]}…`)
        continue
      }
      const key = vm.runInNewContext(lit[0])
      if (!keys.has(key)) keys.set(key, where)
    }
  }
  return { keys, nonLiteral }
}

const placeholders = (s) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort()

test('t() is only called with string literals', () => {
  assert.deepEqual(calls().nonLiteral, [])
})

test('every UI string has a Chinese translation', () => {
  const missing = [...calls().keys].filter(([key]) => !(key in ZH)).map(([key, where]) => `${where}  ${JSON.stringify(key)}`)
  assert.deepEqual(missing, [], `missing Chinese translations:\n${missing.join('\n')}`)
})

test('translations keep the same placeholders', () => {
  const bad = Object.entries(ZH)
    .filter(([en, zh]) => placeholders(en).join() !== placeholders(zh).join())
    .map(([en, zh]) => `${JSON.stringify(en)} → ${JSON.stringify(zh)}`)
  assert.deepEqual(bad, [])
})

test('catalogs have no unused or conflicting entries', () => {
  const used = calls().keys
  const unused = Object.keys(ZH).filter((key) => !used.has(key))
  assert.deepEqual(unused, [], `unused catalog keys (remove them):\n${unused.join('\n')}`)
  const seen = new Map()
  const conflicts = []
  for (const [area, catalog] of Object.entries(AREAS)) {
    for (const [key, value] of Object.entries(catalog)) {
      const prev = seen.get(key)
      if (prev && prev.value !== value) conflicts.push(`${JSON.stringify(key)}: ${prev.area}=${prev.value} vs ${area}=${value}`)
      if (!prev) seen.set(key, { area, value })
    }
  }
  assert.deepEqual(conflicts, [], 'the same English text is translated differently in two areas; move it to common.ts')
})

test('t() falls back to English outside the browser and interpolates', () => {
  assert.equal(i18n.lang(), 'en')
  assert.equal(i18n.t('Using {name}', { name: 'HK 01' }), 'Using HK 01')
  assert.equal(i18n.t('{n} left', {}), '{n} left')
})
