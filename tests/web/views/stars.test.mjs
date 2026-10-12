// stars.test.mjs: his star on a card (the register star/<object>, app.mjs card.starred) in the demo room's own client,
// and Off your mind with the starred cards pinned at the top (desk.mjs pinStarred), the last starred first.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import { SKELETON, demo, readFixture } from './demo-room.mjs'
import { BoardState } from '../../../app/web/public/app.mjs'
import { pinStarred } from '../../../app/web/public/desk.mjs'

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

test('Off your mind: starred cards pinned at the top, the last starred first, each once', () => {
  const sheet = (id, starred, g = 'done') => ({ card: { id, starred }, g, at: 0, said: id })
  const items = [sheet('w1', undefined, 'works'), sheet('d1', undefined), sheet('d2', 5), sheet('d3', undefined), sheet('d4', 9)]
  const starred = [sheet('d2', 5), sheet('d4', 9), sheet('old', 2, 'trash')]   // 'old' is beyond the five the Desk shows
  const out = pinStarred(items, starred)
  assert.deepEqual(out.map(s => s.card.id), ['d4', 'd2', 'old', 'w1', 'd1', 'd3'])
  assert.deepEqual(out.map(s => Boolean(s.pinned)), [true, true, true, false, false, false])
  assert.equal(pinStarred(items, []), items, 'without stars the list is as it was')
})
