// off-ticks.test.mjs: Off your mind (desk.mjs endList, desk.css): an answered, done or read row carries a green tick on
// its right, a shredded one the shredder, a withdrawn one the return arrow; nothing in the list is struck through.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'

const read = f => fs.readFileSync(new URL(`../../../app/web/public/${f}`, import.meta.url), 'utf8')

test('a closed row: the tick when answered or done, the shredder or the return arrow when thrown away', () => {
  const src = read('desk.mjs')
  const box = src.slice(src.indexOf("const box = s.g === 'works'"), src.indexOf('return html`<li class="end-row"'))
  assert.match(box, /end-tick is-done[\s\S]*sk\('tick'\)/, 'the tick is drawn')
  assert.match(box, /s\.g === 'trash'[\s\S]*end-tick is-thrown[\s\S]*sk\(c\.status === 'shredded' \? 'shred' : 'withdraw'\)/, 'shredded: the shredder; withdrawn: the return arrow')
})

test('nothing in Off your mind is struck through', () => {
  const css = read('desk.css')
  const rules = css.split('}').filter(r => /\.end-row/.test(r))
  assert.ok(rules.length > 0)
  assert.ok(rules.every(r => !/line-through/.test(r)), rules.filter(r => /line-through/.test(r)).join('}\n'))
})
