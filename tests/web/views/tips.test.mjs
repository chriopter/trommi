// Hover hints are one to three words (the owner, 13 October 2026). Every `title="…"` and `data-tip="…"` the views
// write, and every title or tip they set from a script, is read from the sources: its fixed words are counted, and a
// value put in (`${card.title}`, a session's name) counts as one word, whatever it holds. What the demo shows is
// checked as rendered by tips.browser.mjs (test:browser), names of the room counting as one word there too.
// An aria-label may say more: it is read, not shown.
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const pub = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..', '..', 'app', 'web', 'public')
const MAX_WORDS = 3
/** Hints that may say more, with why. */
const ALLOWED = new Map([])

/** The value from `at` (after its opening quote `q`) to its closing quote; a `${…}` in it becomes \u0001. */
function valueAt(src, at, q) {
  let out = '', depth = 0
  for (let i = at; i < src.length; i++) {
    const c = src[i]
    if (!depth && c === q) return out
    if (!depth && c === '\\') { out += src[++i]; continue }
    if (src.startsWith('${', i)) { if (!depth) out += '\u0001'; depth++; i++; continue }
    if (depth) { if (c === '{') depth++; else if (c === '}') depth--; continue }
    out += c
  }
  return null
}
export const wordsOf = t => t.replace(/\u0001+/g, ' X ').split(/(?:[\s·,;()[\]→—–/]|:(?!\d))+/).filter(w => w && !/^[^\p{L}\p{N}]+$/u.test(w)).length

/** Every hint the sources write: [file:line, text]. */
export function hints() {
  const files = fs.readdirSync(pub).filter(f => f.endsWith('.mjs')).concat(['demo/demo.mjs'])
  const re = /(?:\b(?:title|data-tip)=(["'])|\.title\s*=\s*(['"`])|dataset\.tip\s*=\s*(['"`])|setAttribute\(\s*['"](?:title|data-tip)['"]\s*,\s*(['"`])|\btip\s*:\s*(['"`]))/g
  const out = []
  for (const f of files) {
    const src = fs.readFileSync(path.join(pub, f), 'utf8')
    for (const m of src.matchAll(re)) {
      const q = m[1] || m[2] || m[3] || m[4] || m[5]
      const v = valueAt(src, m.index + m[0].length, q)
      if (v === null || !v.trim()) continue
      out.push([`${f}:${src.slice(0, m.index).split('\n').length}`, v.replace(/\u0001/g, '${…}')])
    }
  }
  return out
}

test('every hover hint the views write is one to three words', () => {
  const all = hints()
  assert.ok(all.length > 100, 'the hints are found')
  const long = all.filter(([, t]) => wordsOf(t.replace(/\$\{…\}/g, '\u0001')) > MAX_WORDS && !ALLOWED.has(t)).map(([at, t]) => `${at}  "${t}"`)
  assert.deepEqual(long, [], `hints longer than ${MAX_WORDS} words`)
})

test('the count: a name put in is one word, a time is one word', () => {
  assert.equal(wordsOf('Duck it (R)'), 3)
  assert.equal(wordsOf('\u0001 open'), 2)
  assert.equal(wordsOf('13 Oct, 14:02'), 3)
  assert.equal(wordsOf('Settles it: nothing follows from this answer'), 7)
})
