// A hub that withholds a part of a device's chain is said (`withheld`), also while the room's past is still being
// read after a sign-in: by the reading of the past itself, and for a live envelope shown ahead of its chain when
// that reading does not bring it or does not end. STAND-IN core (the real chain checks) and FAKE hub.
import test from 'node:test'
import assert from 'node:assert/strict'
import { rooms, scene, storage, until } from './helpers.mjs'

const withheldSaid = c => c.model.alerts.some(x => x.code === 'withheld')

/** Three notes of the founder; the hub then serves the second one nowhere: not in its order, not by its chain. */
async function hidden(t, timing = {}) {
  const { fake, a, recovery_code } = await scene(t, { timing })
  const first = await a.saveNote({ text: 'the first' }), second = await a.saveNote({ text: 'the second, withheld' }), third = await a.saveNote({ text: 'the third' })
  await a.settle()
  const at = a.model.notes.get(second).envelope_number
  assert.ok(at > 0)
  const without = json => ({ ...json, items: json.items.filter(item => item.change !== at) })
  // how the hub serves the room's past (a rescan from the room's start), and whether it serves the founder's
  // envelopes from the withheld one on at all
  const past = { how: 'at once', cut: false }
  const served = json => (past.cut ? { ...json, items: json.items.filter(item => item.kind !== 'envelope' || item.change < at) } : without(json))
  const isPast = rq => rq.query.get('after') === '0'
  fake.faults.add({ method: 'GET', path: '/v1/changes', times: 1e9, refuse: { error: 'overloaded', status: 503 }, when: rq => past.how === 'never' && isPast(rq) })
  fake.faults.add({ method: 'GET', path: '/v1/changes', times: 1e9, delay_ms: 400, answer: served, when: rq => past.how === 'slowly' && isPast(rq) })
  fake.faults.add({ method: 'GET', path: '/v1/changes', times: 1e9, answer: served })
  fake.faults.add({ method: 'GET', path: /\/chains\/[^/]+$/, times: 1e9, answer: without })
  const R = await rooms({ timing })
  const signIn = async () => {
    const { client } = await R.joinWithCode({ storage: storage('c'), hub_url: fake.url, room_id: a.model.room.room_id, code: recovery_code, device_name: 'c' })
    t.after(() => client.stop().catch(() => {}))
    return client
  }
  /** Starts a device and returns once it asked the hub for the room's past (which the hub answers slowly). */
  const startInThePast = async c => {
    const from = fake.requests.length
    const started = c.start()
    started.catch(() => {})
    await until(() => fake.requests.slice(from).some(r => r.method === 'GET' && r.path === '/v1/changes' && r.query.after === '0'), 'the past is asked for')
    t.after(() => started.catch(() => {}))
  }
  return { a, first, second, third, past, signIn, startInThePast }
}

test('the reading of the past says a part of a chain the hub does not serve', async t => {
  const { first, second, third, signIn } = await hidden(t)
  const c = await signIn()
  await c.start()
  await until(() => !c.engine.rescanPending && c.model.notes.get(first)?.text === 'the first', 'the past is read')
  await until(() => withheldSaid(c), 'the gap in the founder\'s chain is said')
  assert.equal(c.model.notes.get(second), undefined)
  assert.equal(c.model.notes.get(third), undefined, 'what follows the gap is not shown')
})

test('a live envelope shown ahead of its chain while the past is read: the gap before it is said', async t => {
  const { a, past, signIn, startInThePast } = await hidden(t)
  const c = await signIn()
  // (the reading of the past is slow: the live envelope comes while it is due)
  past.how = 'slowly'
  await startInThePast(c)
  assert.ok(c.engine.rescanPending, 'the past is still to be read')
  const live = await a.saveNote({ text: 'written while the past is read' })
  await until(() => c.model.notes.get(live)?.text === 'written while the past is read', 'the live note is shown ahead of its chain')
  await until(() => withheldSaid(c), 'the gap before the live note is said')
})

test('a hub that never serves the past cannot keep a live envelope ahead of its chain for good', async t => {
  const { a, past, signIn } = await hidden(t, { ahead_wait: 300 })
  const c = await signIn()
  // (the reading of the past, from the room's start, is never answered)
  past.how = 'never'
  await c.start()
  const live = await a.saveNote({ text: 'written while the past is read' })
  await until(() => c.model.notes.get(live)?.text === 'written while the past is read', 'the live note is shown ahead of its chain')
  assert.ok(c.engine.rescanPending, 'the past is not read')
  await until(() => withheldSaid(c), 'the gap before the live note is said although the past never came')
})

test('a hub that leaves the live envelope out of the past it serves: its chain is still checked', async t => {
  const { a, past, signIn, startInThePast } = await hidden(t)
  const c = await signIn()
  past.how = 'slowly'
  await startInThePast(c)
  const live = await a.saveNote({ text: 'written while the past is read' })
  await until(() => c.model.notes.get(live)?.text === 'written while the past is read', 'the live note is shown ahead of its chain')
  // the past as the hub serves it from now on: nothing of the founder's notes after the first (so the reading of
  // the past meets no gap of its own), the live one left out too
  past.cut = true
  await until(() => withheldSaid(c), 'the gap before the live note is said')
})
