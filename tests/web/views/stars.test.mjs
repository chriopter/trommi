// stars.test.mjs: his star on a card (the register star/<object>, app.mjs card.starred) in the demo room's own client,
// which is all the page does with it: where a starred card lies is the client core's (core/client/src/off_mind.rs).
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import { SKELETON, demo, readFixture } from './demo-room.mjs'
import { BoardState } from '../../../app/web/public/app.mjs'

test('starred and unstarred: the card says so, and the star is his own register', async () => {
  const client = new demo.MockClient(demo.variantOf(readFixture(SKELETON), '1'), { simulate: false })
  const board = new BoardState({ client, model: client.model })
  const id = [...client.model.cards.keys()][0]
  assert.ok(id, 'the skeleton room has a card')
  await client.star(id, true)
  let card = board.update().cards.find(c => c.id === id)
  assert.ok(card.starred > 0, 'starred: the card carries when')
  assert.ok(client.model.human.raw.get(`star/${id}`).value.at > 0)
  await client.star(id, false)
  card = board.update().cards.find(c => c.id === id)
  assert.equal(card.starred, undefined, 'the star taken off')
  assert.equal(client.model.human.stars.has(id), false)
})
