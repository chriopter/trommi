// history-pictures.test.mjs: pictures in a card's history (a hand-back's, a version before) are small tiles that fit
// the column and open full size on a tap; the history's words are the normal small size (card.mjs cardThread, card.css).
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'

const read = f => fs.readFileSync(new URL(`../../../app/web/public/${f}`, import.meta.url), 'utf8')

test('a history picture is a tile that opens full size', () => {
  const src = read('card.mjs')
  assert.match(src, /<a class="tc-shot" href="\$\{a\.url\}" target="_blank" rel="noopener" title="Open full size"/)
  assert.match(src, /<img\$\{srcOf\(a, 320\)\} alt="\$\{a\.name\}" loading="lazy" decoding="async" width="220" height="165">/)
})

test('it never grows past the column, and stands at most 240 px high', () => {
  const css = read('card.css')
  assert.match(css, /:is\(\.tc-deed, \.tc-was\) \.shots \{ display: grid; grid-template-columns: repeat\(auto-fill, minmax\(140px, 220px\)\);[^}]*max-width: 100%; min-width: 0; \}/)
  assert.match(css, /:is\(\.tc-deed, \.tc-was\) \.shots > a \{[^}]*min-width: 0; overflow: hidden;[^}]*cursor: zoom-in; \}/)
  assert.match(css, /:is\(\.tc-deed, \.tc-was\) \.shots img \{[^}]*width: 100%; max-width: 100%; height: auto; max-height: 240px;/)
})

test('the history reads at the small size, not the tiny one', () => {
  const css = read('card.css')
  assert.match(css, /\.tc-deed \{[^}]*font: 500 var\(--t-sm\)\/1\.5 var\(--font\); \}/)
  assert.match(css, /\.tc-turn-before \{ color: var\(--accent\); font: 600 var\(--t-sm\)/)
})
