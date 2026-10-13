// off-ticks.test.mjs: Off your mind (desk.mjs endList, desk.css): an answered, done or read row carries a green tick on
// its right, a shredded one the shredder and is struck through, a withdrawn one the return arrow; nothing else is struck through.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'

const read = f => fs.readFileSync(new URL(`../../../app/web/public/${f}`, import.meta.url), 'utf8')

test('a closed row: the tick when answered or done, the shredder or the return arrow when thrown away', () => {
  const src = read('desk.mjs')
  const box = src.slice(src.indexOf('function miniRow('), src.indexOf('return html`<li class="oym-mini"'))
  assert.match(box, /s\.g === 'trash' \? sk\(c\.status === 'shredded' \? 'shred' : 'withdraw'\) : sk\('tick'\)/, 'shredded: the shredder; withdrawn: the return arrow; else the tick')
})

test('only a shredded row is struck through', () => {
  const css = read('desk.css'), src = read('desk.mjs')
  const struck = css.split('}').filter(r => /\.oym-mini/.test(r) && /line-through/.test(r))
  assert.equal(struck.length, 1, struck.join('}\n'))
  assert.match(struck[0], /\.oym-mini\[data-shredded\]/)
  assert.match(src, /s\.g === 'trash' && c\.status === 'shredded' \? raw\(' data-shredded'\)/, 'only a shredded row is marked so')
})

test('the cards behind the stack are the front card\'s size, a little narrower and lower; never turned', () => {
  const css = read('desk.css')
  const sheet = css.match(/^\.oym-sheet \{[^}]*\}/m)?.[0] ?? ''
  assert.match(sheet, /left: calc\(var\(--k\) \* 12px\); right: calc\(var\(--k\) \* 12px\)/)
  assert.match(sheet, /transform: translateY\(calc\(var\(--k\) \* 7px\)\)/)
  assert.doesNotMatch(css.match(/\.oym-sheet[^{]*\{[^}]*\}/g).join('\n'), /rotate/, 'a turn runs askew across a wide card')
})

test('a put-off line says until when, or nothing; a line with no time never shows the epoch', () => {
  const src = read('desk.mjs')
  assert.match(src, /const when = s\.g === 'later' \? \(until \? html`<span class="ago oym-mini-ago">until \$\{until\}<\/span>` : ''\) : s\.at > 0 \? agoSpan\(s\.at, 'ago oym-mini-ago'\) : ''/)
})
