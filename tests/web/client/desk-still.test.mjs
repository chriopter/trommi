// The Desk a person looks at stays still while a new device reads the room's past. A card stands on the Desk by the
// change it was first filed at (core/client/src/desk.rs). A device that signs in knows each open card by its newest
// version from the hub's Desk, and the Desk's row says the change of its first (`first_change`), so the card stands
// in its place at once and reading its first version later moves nothing. Before, a card stood by the time of the
// version known and moved when the reading of the past reached its first one: the Desk shuffled for as long as the
// reading took.
// The REAL core (real envelopes) and the FAKE hub.
import test from 'node:test'
import assert from 'node:assert/strict'
import { rooms, scene, sleep, storage, until } from './helpers.mjs'

test('a sign-in shows the open cards in their final order at once: no card moves, and no past is read', async t => {
  const { fake, a, agent, recovery_code } = await scene(t, { agent: true })
  const options = [{ key: 'yes', label: 'Yes' }, { key: 'no', label: 'No' }]
  const open = []
  // a history with open cards spread through it, oldest first, and closed ones between them
  for (let i = 0; i < 12; i++) {
    open.push(await agent.askCard({ title: `still open ${i}`, options }, { urgency: i % 4 === 1 ? 'high' : 'normal' }))
    await agent.settle()
    const done = await agent.askCard({ title: `done ${i}`, options })
    await agent.settle()
    await agent.closeCard(done, { close_summary: 'done' })
    for (let k = 0; k < 6; k++) await agent.say({ text: `said ${i}/${k}` })
    await agent.settle()
    await sleep(3)
  }
  // later the agent wrote some of the open ones again (it explained, it added a picture): their newest version is
  // far from their first
  for (const [k, card] of open.entries()) if (k % 3 !== 1) { await agent.reviseCard(card, { body: `plainly ${k}` }); await agent.settle(); await sleep(3) }
  await a.settle(); await agent.settle()
  await until(() => a.model.stack.length === open.length, 'the first device shows every open card')
  const final = [...a.model.stack]
  assert.notDeepEqual(final, [...final].sort((x, y) => a.model.cards.get(x).envelope_number - a.model.cards.get(y).envelope_number), 'the scene: the Desk\'s order is not the order of the newest versions')

  // the new device's store is slow, so the reading of the past is told batch by batch, as on a real disk
  const R = await rooms({ slow_store_ms: 10, timing: { progress: 10 } })
  const { client: c, sessions } = await R.joinWithCode({ storage: storage('c'), hub_url: fake.url, room_id: a.model.room.room_id, code: recovery_code, device_name: 'c' })
  t.after(() => c.stop().catch(() => {}))
  const seen = [], walks = [], parts = []
  let read = 0
  c.on('change', () => {
    read = Math.max(read, c.model.room.reading_progress?.done ?? 0)
    const now = c.model.stack.join(' ')
    if (c.model.stack.length === open.length && seen.at(-1) !== now) seen.push(now)
    // (and while only some of them stand, those stand in the order they keep)
    if (c.model.stack.length && parts.at(-1) !== now) parts.push(now)
    const rows = c.model.desk_order.join(' ')
    if (c.model.desk_order.length === open.length && walks.at(-1) !== rows) walks.push(rows)
  })
  await c.start()
  assert.equal((await sessions).joined, 1)
  await until(() => c.model.room.connection === 'live' && !c.engine.rescanPending && !c.model.room.reading, 'the sign-in is through', 30_000)
  // (9.0.6: the Desk and what is new, never the room's past)
  assert.equal(read, 0, 'no past was read')
  assert.deepEqual([...c.model.stack], final, 'both devices show the same Desk in the end')
  assert.deepEqual(seen, [final.join(' ')], `the whole Desk stood in one order from its first showing on (${seen.length} orders seen)`)
  for (const part of parts) { const ids = part.split(' '); assert.deepEqual(ids, final.filter(id => ids.includes(id)), 'a Desk that shows some of the cards shows them in the order they keep') }
  assert.deepEqual(walks, [a.model.desk_order.join(' ')], `the Desk's rows stood in one order, the first device's (${walks.length} orders seen)`)
  // the change each card stands by is the same on both devices; the time shown with it is its signed first version's
  // once the card is opened, which reads its versions by the card's own route and nothing else
  for (const id of open) assert.equal(c.model.cards.get(id).first_envelope_number, a.model.cards.get(id).first_envelope_number)
  const asked = fake.requests.length
  await c.loadTimeline(`chat:card/${open[0]}`, { limit: 50 })
  const paths = fake.requests.slice(asked).map(r => r.path)
  assert.equal(paths.filter(p => p.startsWith('/v1/cards/')).length, 1, `the card's versions are read when it is opened (${paths.join(', ')})`)
  assert.ok(!paths.some(p => p === '/v1/changes' || p.includes('/chains/')), 'and no history beside them')
  await until(() => c.model.cards.get(open[0]).created_at === a.model.cards.get(open[0]).created_at, 'the time of its first version')
})
