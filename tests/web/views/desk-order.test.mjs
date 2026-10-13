// desk-order.test.mjs: the Desk's rows stand in the client core's order (client/src/desk.rs walk_order, the patch's
// desk_order); the page sorts nothing of its own.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'

const src = fs.readFileSync(new URL('../../../app/web/public/desk.mjs', import.meta.url), 'utf8')

test('the rows follow the core\'s desk_order, no order of the page\'s own', () => {
  assert.match(src, /for \(const id of model\.state\.desk_order \?\? \[\]\)/)
  assert.doesNotMatch(src, /byUrgency|urgRank/)
})
