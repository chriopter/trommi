// blitz-order.test.mjs: Blitz walks the Desk's rows in the client core's order, and after a card leaves the walk the
// next one is the client core's (core/client/src/model/walk.rs next_open_after), as on the iPhone.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'

const read = f => fs.readFileSync(new URL(`../../../app/web/public/${f}`, import.meta.url), 'utf8')
const app = read('app.mjs'), desk = read('desk.mjs'), card = read('card.mjs')

test('the walk is the core\'s desk_order, and the Desk\'s rows are the walk', () => {
  assert.match(app, /export const walkOf = m => \(m\.walk \?\?= \(\(\) => \{[\s\S]*?m\.state\.desk_order/)
  assert.match(desk, /const deskCards = m => walkOf\(m\)/)
})

test('after a card leaves the walk, the next comes from the core, in Blitz and from a card\'s page', () => {
  assert.match(card, /if \(form\.has\('walk'\)[^\n]*\n\s*const nextId = await Promise\.resolve\(hub\.nextOpenAfter\?\.\(id, deskScope\(model\(\)\)\)\)/)
  assert.match(desk, /const nextId = done\.length && from \? await Promise\.resolve\(hub\.nextOpenAfter\?\.\(from\.id, deskScope\(m\)\)\)/)
  assert.doesNotMatch(card + desk, /walk0/)
})
