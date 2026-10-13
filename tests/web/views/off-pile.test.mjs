// off-pile.test.mjs: Off your mind as ONE pile (the owner's pick of 13 October 2026, "untereinander"): the first three
// running cards on top, one under the other; everything else on one pile that says how much lies on it and opens in
// place; the put-off and done ones small at the bottom. The layout from the counts is the client core's rule
// (client/src/off_pile.rs off_pile); desk.mjs offPile mirrors it and is held equal to the core's here.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import { offPile, pileWords, PILE_TOP, PILE_REST } from '../../../app/web/public/desk.mjs'

const read = f => fs.readFileSync(new URL(`../../../${f}`, import.meta.url), 'utf8')
const PKG = new URL('../../../core/wasm/pkg/trommi-core.js', import.meta.url)

test('three running on top, the rest on the pile, the real totals', () => {
  const p = offPile(5, 1, 2345, false)
  assert.deepEqual([p.top, p.moreRunning, p.putOff, p.closed, p.pile, p.behind, p.restShown, p.open], [3, 2, 1, 2345, true, 3, PILE_REST, false])
  assert.equal(pileWords(p), '+2 running · 1 put off · 2345 done')
  assert.equal(offPile(5, 1, 2345, true).restShown, 2346, 'the page shows every one')
  for (const n of [0, 1, 2, 3]) assert.equal(offPile(n, 0, 0, false).pile, false, `${n} running and nothing else: no pile`)
  const none = offPile(0, 2, 3, false)
  assert.deepEqual([none.top, none.behind, none.restShown], [0, 1, 5])
  assert.equal(pileWords(none), '2 put off · 3 done')
})

test('the same numbers as the client core', { skip: fs.existsSync(PKG) ? false : 'core/wasm/pkg is not built' }, async () => {
  const core = await import(PKG.href)
  await core.init(fs.readFileSync(new URL('../../../core/wasm/pkg/trommi_core_wasm_bg.wasm', import.meta.url)))
  for (const running of [0, 1, 3, 4, 9]) for (const putOff of [0, 1, 7]) for (const closed of [0, 4, 12, 900]) for (const full of [false, true]) {
    assert.deepEqual({ ...core.offPile(running, putOff, closed, full) }, offPile(running, putOff, closed, full), `${running} ${putOff} ${closed} ${full}`)
  }
  assert.match(read('client/src/off_pile.rs'), new RegExp(`pub const PILE_TOP: u64 = ${PILE_TOP};[\\s\\S]*pub const PILE_REST: u64 = ${PILE_REST};`))
})

test('the Desk draws the pile: the top cards, one pile that opens, small rows at the bottom; no switch, no other look', () => {
  const src = read('app/web/public/desk.mjs'), css = read('app/web/public/desk.css')
  assert.match(src, /const pile = offPile\(works\.length, laterRows\.length, closedRows \? closedRows\.length : closedSize, full\)/, 'the real total of the closed cards')
  assert.match(src, /<div class="pile-cards">\$\{works\.slice\(0, pile\.top\)\.map\(card\)\}<\/div>/)
  assert.match(src, /<details class="pile-wrap" data-controller="oym"\$\{pile\.open \? raw\(' open'\) : ''\}>/)
  assert.match(src, /\$\{works\.slice\(pile\.top\)\.map\(card\)\}\$\{rest\.length \? html`<div class="pile-rests">/, 'opened: the other running cards, then the rest')
  assert.doesNotMatch(src, /trommi-pile|pileLook|is-tile|oym-minis|oym-stackwrap/)
  assert.doesNotMatch(css, /\.is-b |oym-mini|oym-stack/)
  assert.match(css, /\.pile-wrap:not\(\[open\]\) > \.pile-open \{ display: none; \}/)
})

test('the iPhone lays it out by the core (offPile) and draws the same pile', () => {
  const swift = read('app/ios/TrommiApp/Sources/TrommiApp/MoreScreens.swift')
  assert.match(swift, /let pile = offPile\(running: UInt64\(busy\.count\), putOff: UInt64\(later\.count\), closed: UInt64\(closed\.count\), full: full\)/)
  assert.match(swift, /struct OffPileView: View/)
  assert.doesNotMatch(swift, /struct RunningStack: View/)
})
