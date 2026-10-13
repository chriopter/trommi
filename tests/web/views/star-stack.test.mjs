// star-stack.test.mjs: his starred cards as a small stack beside the note (the owner, 13 October 2026; sidebar.mjs
// starStack). Which cards and in what order is the client core's (client/src/stars.rs, client.starredCards); the
// page only draws them. The demo room answers the same rule.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import { demo } from './demo-room.mjs'

const read = p => fs.readFileSync(new URL(`../../../${p}`, import.meta.url), 'utf8')

test('the stack takes its cards from the client core and draws them; no switch between proposals', () => {
  const src = read('app/web/public/sidebar.mjs')
  assert.match(src, /list = await client\.starredCards\(\)/)
  assert.match(src, /const cards = list\.map\(s => m\.byCard\?\.get\(s\.object_id\)\)\.filter\(Boolean\)/)
  assert.match(src, /\$\{starStack\(base\)\}<button type="button" class="corner-note-head"/, 'it stands beside the note')
  assert.doesNotMatch(src, /trommi-stars|STAR_LOOKS/)
  assert.match(read('app/web/core/worker-protocol.ts'), /'starredCards'/)
  assert.match(read('app/web/core/client.ts'), /starredCards\(\)[^\n]*this\.board\.starredCards\(\)/)
})

test('open: the cards fan up into a column above the stack; Escape or a tap beside lays them back', () => {
  const css = read('app/web/public/sidebar.css'), src = read('app/web/public/sidebar.mjs')
  assert.match(css, /\.ss-open \{ display: none; position: absolute; right: 0; bottom: calc\(100% \+ 10px\)/)
  assert.match(css, /\.star-stack\.is-open \.ss-open \{ display: grid; \}/)
  assert.match(src, /e\.key === 'Escape' && this\.element\.classList\.contains\('is-open'\)/)
  assert.match(src, /if \(!this\.element\.contains\(e\.target\)\) this\.set\(false\)/)
})

test('the demo room: the last starred first, ties by id, a star taken away or of no card is none', () => {
  const c = Object.create(demo.MockClient.prototype)
  c.model = { cards: new Map([['a', {}], ['b', {}], ['c', {}]]), human: { stars: new Map([['a', { at: 1 }], ['b', { at: 5 }], ['c', null], ['x', { at: 9 }]]) } }
  assert.deepEqual(c.starredCards().map(s => s.object_id), ['b', 'a'])
})

test('the iPhone draws the core\'s list beside the Desk', () => {
  assert.match(read('app/ios/TrommiClient/Sources/TrommiClient/Board.swift'), /engine\.starredCards\(\)/)
  assert.match(read('app/ios/TrommiApp/Sources/TrommiApp/DeskScreen.swift'), /StarStack\(\)/)
})
