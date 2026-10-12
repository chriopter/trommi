// A start from this device's cache shows only what the hub's Desk lists as open: a card closed on another device
// since the last visit never stands on the Desk, not even before the catch-up has read its closing (the person's
// rule: the Desk is the hub's open list). The REAL core and the FAKE hub.
import test from 'node:test'
import assert from 'node:assert/strict'
import { scene, sleep, until } from './helpers.mjs'

/** B sees three cards and a status info open and goes away; on A two of the cards are answered and closed by the
 *  agent, and the info is read (closed by reading, as the owner's old status card). */
async function away(t) {
  const s = await scene(t, { agent: true, second: true })
  const { a, b, agent } = s
  const cards = []
  for (let i = 0; i < 3; i++) cards.push(await agent.askCard({ title: `Card ${i}`, options: [{ key: 'ok', label: 'OK' }] }))
  const info = await agent.askCard({ card_type: 'info', title: 'Status 11:05', body: 'All rooms still working.' })
  await agent.settle(); await a.settle(); await b.settle()
  await until(() => [...cards, info].every(id => b.model.stack.includes(id)), 'B shows the three cards and the info')
  await b.flush()
  await b.stop()
  const [kept, ...closed] = cards
  for (const id of closed) { await a.answer({ object_id: id, choices: ['ok'] }); await a.settle(); await agent.closeCard(id, { close_summary: 'done' }) }
  await a.markRead({ object_id: info })
  await agent.settle(); await a.settle()
  return { ...s, kept, closed: [...closed, info] }
}
const shownOpen = (m, ids) => ids.filter(id => m.stack.includes(id) || [...m.sessions.values()].some(s => s.open_card_ids?.includes(id)))

test('warm start: the Desk waits for the hub\'s list; a card closed elsewhere is never open in the model that is painted', async t => {
  const { R, b, kept, closed } = await away(t)
  const warm = await R.openRoom({ storage: b.stored_as })
  t.after(() => warm.stop().catch(() => {}))
  // (from the cache alone the three are still open, as the hub listed them last: the page does not paint this model,
  //  it waits for deskFirst)
  assert.deepEqual(shownOpen(warm.model, closed).sort(), [...closed].sort(), 'the cache alone holds them as open')
  await warm.deskFirst(5000)
  assert.deepEqual(shownOpen(warm.model, closed), [], 'no closed card stands after the Desk answered')
  assert.ok(warm.model.stack.includes(kept), 'the open card stands')
  assert.ok(!warm.model.room.desk_waiting)
  // and it stays so through the start and the catch-up
  const seen = new Set()
  warm.on('change', () => { for (const id of shownOpen(warm.model, closed)) seen.add(id) })
  await warm.start()
  await warm.settle()
  await until(() => closed.every(id => warm.model.cards.get(id)?.object_state !== 'open'), 'the closings are read')
  assert.deepEqual([...seen], [], 'no closed card stood open at any change')
  assert.deepEqual(warm.model.stack, [kept])
})

test('warm start with a slow hub: past the wait no open card stands (a quiet "catching up"), then exactly the hub\'s list', async t => {
  const { R, b, kept, closed } = await away(t)
  const warm = await R.openRoom({ storage: b.stored_as })
  t.after(() => warm.stop().catch(() => {}))
  const brief = warm.hub.deskBrief.bind(warm.hub)
  warm.hub.deskBrief = async (...args) => { await sleep(400); return brief(...args) }
  await warm.deskFirst(50)
  assert.equal(warm.model.room.desk_waiting, true)
  assert.deepEqual(warm.model.stack, [], 'nothing stands while the list is not there')
  await until(() => !warm.model.room.desk_waiting, 'the Desk answered')
  assert.deepEqual(shownOpen(warm.model, closed), [])
  assert.ok(warm.model.stack.includes(kept))
})

test('the next start from the cache keeps to the list read last: a card the hub had settled stays off the Desk before it answers', async t => {
  const { R, b, kept, closed } = await away(t)
  const first = await R.openRoom({ storage: b.stored_as })
  // the list is read (the closings not yet), then the page goes away
  await first.deskFirst(5000)
  await first.flush()
  await first.stop()
  const again = await R.openRoom({ storage: b.stored_as })
  t.after(() => again.stop().catch(() => {}))
  // before any reading of this run: the list kept from the last decides already
  assert.deepEqual(shownOpen(again.model, closed), [])
  assert.ok(again.model.stack.includes(kept))
})
