// A hub that withholds a part of a device's chain is said (`withheld`). A device that signed in reads no past
// (9.0.6): what the hub withholds of the time before is not read at all, and the warning is about the chains as
// they go on from the sign-in. STAND-IN core (the real chain checks) and FAKE hub.
import test from 'node:test'
import assert from 'node:assert/strict'
import { rooms, scene, storage, until } from './helpers.mjs'

const withheldSaid = c => c.model.alerts.some(x => x.code === 'withheld')

async function signedIn(t) {
  const { fake, a, recovery_code } = await scene(t)
  await a.saveNote({ text: 'before the sign-in' })
  await a.settle()
  const R = await rooms({})
  const stored_as = storage('c')
  const { client: c } = await R.joinWithCode({ storage: stored_as, hub_url: fake.url, room_id: a.model.room.room_id, code: recovery_code, device_name: 'c' })
  t.after(() => c.stop().catch(() => {}))
  await c.start()
  await until(() => c.model.room.connection === 'live', 'the new device live')
  await c.settle(); await a.settle()
  return { fake, a, c, stored_as }
}

test('after a sign-in, a part of a chain the hub does not serve in its order is said', async t => {
  const { fake, a, c, stored_as } = await signedIn(t)
  // the founder writes three notes; the hub serves the second one nowhere: not in its order, not by its chain
  const first = await a.saveNote({ text: 'the first' })
  await a.settle(); await c.settle()
  await until(() => c.model.notes.get(first)?.text === 'the first', 'the first note, live')
  let at = null
  const without = json => ({ ...json, items: json.items.filter(item => item.change !== at) })
  fake.faults.add({ method: 'GET', path: '/v1/changes', times: 1e9, answer: without })
  fake.faults.add({ method: 'GET', path: /\/chains\/[^/]+$/, times: 1e9, answer: without })
  fake.dropStreams()
  await c.stop()
  const second = await a.saveNote({ text: 'the second, withheld' })
  await a.settle()
  at = a.model.notes.get(second).envelope_number
  const third = await a.saveNote({ text: 'the third' })
  await a.settle()
  const R = await rooms({})
  const again = await R.openRoom({ storage: stored_as })
  t.after(() => again.stop().catch(() => {}))
  await again.start()
  await until(() => withheldSaid(again), 'the gap in the founder\'s chain is said')
  // (the Desk may still list the notes as open objects, taken from their signed headers: the warning is the chain's)
  void second; void third
})

test('a part the hub withholds from before the sign-in is not read: no chain is asked from its start, nothing is said', async t => {
  const { fake, c } = await signedIn(t)
  assert.equal(fake.requests.filter(r => r.path.includes('/chains/')).length, 0, 'no chain read')
  assert.equal(withheldSaid(c), false)
})
