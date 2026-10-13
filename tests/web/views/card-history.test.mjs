// card-history.test.mjs: a card's whole history under it (card.mjs cardThread, app.mjs eventsOf). The card shows its
// newest version on top; below, every revision stands in order with when, what changed (revise_card's note) and the
// version before it to open, the talk in between. A newer revision never folds away or replaces the ones before it.
// The history comes from the client core (types.rs `history_of`): the app only renders it.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import { World, hex, id16 } from '../model/factory.mjs'
import { BoardState } from '../../../app/web/public/app.mjs'
import { cardThread } from '../../../app/web/public/card.mjs'

/** An info card its agent revised five times, each with a note; he wrote under it after the second revision. */
function revisedFiveTimes() {
  const w = new World(); w.groups()
  const id = id16(0xc4), hid = hex(id)
  let prev = w.card(id, { card_type: 'info', title: 'Working on: the history', body: 'Started', object_version: 1, working: true }).hash
  for (let n = 2; n <= 6; n++) {
    prev = w.card(id, { card_type: 'info', title: n === 6 ? 'Done: the history' : 'Working on: the history', body: `Step ${n - 1} built`, change_note: `step ${n - 1} built`, object_version: n, ...(n < 6 ? { working: true } : {}) }, { previous: prev }).hash
    if (n === 3) w.message({ text: 'status' }, { card: id, sender: w.me })
  }
  return { w, hid }
}

test('the client core keeps every revision: a card revised five times has five', () => {
  const { w, hid } = revisedFiveTimes()
  const c = w.model.cards.get(hid)
  assert.equal(c.object_version, 6)
  assert.deepEqual(c.history.map(h => h.object_version), [1, 2, 3, 4, 5, 6])
  assert.deepEqual(c.history.slice(1).map(h => h.note), ['step 1 built', 'step 2 built', 'step 3 built', 'step 4 built', 'step 5 built'])
})

test('a card revised five times shows five entries under it, none folded away, the talk in its place', () => {
  const { w, hid } = revisedFiveTimes()
  const board = new BoardState({ model: w.model })
  const state = board.update()
  const card = state.cards.find(c => c.id === hid)
  assert.ok(card, 'the card is on the board')
  assert.equal(card.title, 'Done: the history', 'the newest version stands on top')
  const html = String(cardThread(card, { state, byAgent: new Map() }))
  const turns = [...html.matchAll(/<div class="tc-turn" id="turn-[^"]+" data-version="(\d+)">/g)].map(m => Number(m[1]))
  assert.deepEqual(turns, [2, 3, 4, 5, 6], 'every revision is an entry, in order')
  for (let n = 1; n <= 5; n++) assert.match(html, new RegExp(`step ${n} built`), `revision ${n + 1} says what changed`)
  for (let n = 1; n <= 5; n++) assert.match(html, new RegExp(`<summary>See version ${n}</summary>`), `version ${n} opens under the one after it`)
  assert.doesNotMatch(html, /Earlier versions/, 'nothing folds away behind a newer revision')
  const talk = html.indexOf('>status<'), v3 = html.indexOf('data-version="3"'), v4 = html.indexOf('data-version="4"')
  assert.ok(talk > v3 && talk < v4, 'his reply stands where it was written, between the revisions')
})
