// off-ticks.test.mjs: Off your mind (desk.mjs endList, desk.css): every closed row, answered or thrown away, carries the
// same green tick on its right; nothing in the list is struck through.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'

const read = f => fs.readFileSync(new URL(`../../../app/web/public/${f}`, import.meta.url), 'utf8')

test('a closed row gets the tick, whatever closed it', () => {
  const src = read('desk.mjs')
  const box = src.slice(src.indexOf("const box = s.g === 'works'"), src.indexOf('return html`<li class="end-row"'))
  assert.match(box, /end-tick is-done[\s\S]*sk\('tick'\)/, 'the tick is drawn')
  assert.doesNotMatch(box, /s\.g === 'done' \?/, 'not only for answered rows: the last branch takes every closed one')
})

test('nothing in Off your mind is struck through', () => {
  const css = read('desk.css')
  const rules = css.split('}').filter(r => /\.end-row/.test(r))
  assert.ok(rules.length > 0)
  assert.ok(rules.every(r => !/line-through/.test(r)), rules.filter(r => /line-through/.test(r)).join('}\n'))
})
