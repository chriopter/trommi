// A sign-in with the code into an account with sessions: the device is in once the room group took its join, with
// its KeyPackages at the hub at once; the session groups are joined behind it. A failure after the room join never
// throws the device away. STAND-IN core and FAKE hub: nothing here is evidence against a hub.
import test from 'node:test'
import assert from 'node:assert/strict'
import { addAgent, rooms, scene, storage, until } from './helpers.mjs'

const roomOf = fake => [...fake.state.rooms.values()][0]

test('a sign-in with the code says how far it is, stocks its KeyPackages at once and joins the sessions behind it', async t => {
  const { fake, a, recovery_code } = await scene(t, { agent: true })
  await addAgent(t, a, {}, 'agent2'); await addAgent(t, a, {}, 'agent3')
  await a.settle()
  const sessions = 3
  const R = await rooms()
  const steps = []
  // the session groups' Commits are slow to read: the sign-in does not wait for them
  fake.faults.add({ method: 'GET', path: /\/log$/, delay_ms: 30, times: 1000 })
  const kpBefore = roomOf(fake).key_packages.size
  const { client: c, sessions: joining } = await R.joinWithCode({ storage: storage('c'), hub_url: fake.url, room_id: a.model.room.room_id, code: recovery_code, device_name: 'c', progress: s => steps.push(s) })
  t.after(() => c.stop().catch(() => {}))
  assert.deepEqual(steps[0], { step: 'reading' })
  assert.ok(steps.some(s => s.step === 'checking' && s.total > 0 && s.done === s.total), `the check reached its total: ${JSON.stringify(steps)}`)
  assert.deepEqual(steps.at(-1), { step: 'joining' })
  assert.equal(roomOf(fake).key_packages.size, kpBefore + 1, 'the new device has KeyPackages at the hub as soon as it is in')
  const done = await joining
  assert.deepEqual(done.not_joined, [])
  assert.equal(done.joined, sessions)
  await c.start()
  await until(() => c.engine.groups.filter(g => g.session).length === sessions, 'the new device in every session')
})

test('a session join the hub refuses leaves the device in the room and lists the session', async t => {
  const { fake, a, recovery_code } = await scene(t, { agent: true })
  await a.settle()
  const R = await rooms()
  // (the first Commit posted is the room group's join; every one after it, a session's, is refused for good)
  let commits = 0
  fake.faults.add({ method: 'POST', path: /^\/v1\/groups\/[^/]+\/commits$/, refuse: { error: 'forbidden', status: 403 }, times: 100, when: () => ++commits > 1 })
  const { client: c, sessions: joining } = await R.joinWithCode({ storage: storage('c'), hub_url: fake.url, room_id: a.model.room.room_id, code: recovery_code, device_name: 'c' })
  t.after(() => c.stop().catch(() => {}))
  const done = await joining
  assert.equal(done.joined + done.not_joined.length, 1)
  assert.ok(c.engine.is_human, 'the device is in the room whatever the sessions did')
})
