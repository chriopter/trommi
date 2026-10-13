// A sign-in with the code into an account with sessions: the device is in once the room group took its join, with
// its KeyPackages at the hub at once; the session groups are joined behind it. A failure after the room join never
// throws the device away. STAND-IN core and FAKE hub: nothing here is evidence against a hub.
import test from 'node:test'
import assert from 'node:assert/strict'
import { Hub } from '../../../app/web/core/hub.ts'
import { roomsOn } from '../../../app/web/core/room.ts'
import { MemoryStore, TIMING, addAgent, core, memoryCache, rooms, scene, storage, until, wipe } from './helpers.mjs'

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
  const kpBefore = roomOf(fake).key_packages.size, before = fake.requests.length
  const { client: c, sessions: joining } = await R.joinWithCode({ storage: storage('c'), hub_url: fake.url, room_id: a.model.room.room_id, code: recovery_code, device_name: 'c', progress: s => steps.push(s) })
  t.after(() => c.stop().catch(() => {}))
  assert.deepEqual(steps[0], { step: 'reading' })
  // (only the room group is read and checked before the join, 8.5: no session's GroupInfos or Commits)
  assert.ok(!steps.some(s => s.step === 'reading' && s.total), `no session is read before the join: ${JSON.stringify(steps)}`)
  assert.ok(steps.some(s => s.step === 'checking' && s.total > 0 && s.done === s.total), `the check reached its total: ${JSON.stringify(steps)}`)
  assert.deepEqual(steps.at(-1), { step: 'joining' })
  assert.equal(roomOf(fake).key_packages.size, kpBefore + 1, 'the new device has KeyPackages at the hub as soon as it is in')
  const done = await joining
  assert.deepEqual(done.not_joined, [])
  assert.equal(done.joined, sessions)
  // (the three main sessions' joins in one request, `POST /v1/commits`, not one each)
  const posts = fake.requests.slice(before).filter(r => r.method === 'POST' && (r.path === '/v1/commits' || /^\/v1\/groups\/[^/]+\/commits$/.test(r.path)))
  assert.deepEqual(posts.map(r => r.path === '/v1/commits' ? 'together' : 'alone'), ['alone', 'together'], 'the room group\'s join alone, then the sessions\' together')
  // (and what the joins are built from is read in one request too, while the room group is joined)
  assert.equal(fake.requests.slice(before).filter(r => r.method === 'POST' && r.path === '/v1/groups/read').length, 1)
  assert.equal(fake.requests.slice(before).filter(r => r.method === 'GET' && /^\/v1\/groups\/[^/]+\/log$/.test(r.path)).length, 1, 'the room group\'s log alone is read by itself')
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

test('joins posted together: one refused for good, one the hub throttles, the others taken; each as if posted alone', async t => {
  const { fake, a, recovery_code } = await scene(t, { agent: true })
  await addAgent(t, a, {}, 'agent2'); await addAgent(t, a, {}, 'agent3')
  await a.settle()
  const R = await rooms()
  // (the hub refuses the first session's join for good and throttles the second's once, before it takes them)
  const seen = []
  let round = 0
  fake.joinFault = g => {
    g = String(g)
    if (!seen.includes(g)) seen.push(g)
    if (seen.indexOf(g) === 0) return fake.refusal('forbidden', 'not this one')
    if (seen.indexOf(g) === 1 && round++ === 0) return fake.refusal('rate-limited', 'later', 1)
    return null
  }
  const { client: c, sessions: joining } = await R.joinWithCode({ storage: storage('c'), hub_url: fake.url, room_id: a.model.room.room_id, code: recovery_code, device_name: 'c' })
  t.after(() => c.stop().catch(() => {}))
  await c.start()
  const done = await joining
  assert.ok(round >= 1, 'the second was throttled once')
  assert.equal(done.not_joined.length, 1, 'the one refused for good is listed')
  assert.equal(done.not_joined[0].code, 'forbidden')
  assert.equal(done.joined, 2, 'the throttled one went through on its next post, the third at once')
})

test('a hub without POST /v1/commits and POST /v1/groups/read (from before them): the sessions are read and joined one by one, as before', async t => {
  const { fake, a, recovery_code } = await scene(t, { agent: true })
  await addAgent(t, a, {}, 'agent2')
  await a.settle()
  const R = await rooms()
  fake.faults.add({ method: 'POST', path: '/v1/commits', refuse: { error: 'not-found', status: 404 }, times: 100 })
  fake.faults.add({ method: 'POST', path: '/v1/groups/read', refuse: { error: 'not-found', status: 404 }, times: 100 })
  const before = fake.requests.length
  const { client: c, sessions: joining } = await R.joinWithCode({ storage: storage('c'), hub_url: fake.url, room_id: a.model.room.room_id, code: recovery_code, device_name: 'c' })
  t.after(() => c.stop().catch(() => {}))
  await c.start()
  const done = await joining
  assert.deepEqual(done.not_joined, [])
  assert.equal(done.joined, 2)
  const posts = fake.requests.slice(before).filter(r => r.method === 'POST' && (r.path === '/v1/commits' || /^\/v1\/groups\/[^/]+\/commits$/.test(r.path)))
  assert.equal(posts.filter(r => r.path === '/v1/commits').length, 1, 'asked once, then never again')
})

test('a session join the core answers busy (a join of the group on its way, or this device added meanwhile) is tried again and goes through', async t => {
  const { fake, a, recovery_code } = await scene(t, { agent: true })
  await a.settle()
  // the stand-in core, its session joins answered `busy` twice: what a device answers while a join of the group
  // waits for its place in the log, or another human device's Add of it waits as a Welcome
  const base = await core()
  let busy = 2
  const R = roomsOn({
    core: async () => ({
      ...base,
      errorCode: e => (e?.injected ? 'busy' : base.errorCode(e)),
      createDevice: async store => {
        const d = await base.createDevice(store)
        return new Proxy(d, { get(target, name) {
          const f = target[name]
          if (name !== 'joinSessionCheckedKeys') return typeof f === 'function' ? f.bind(target) : f
          return async (...args) => { if (busy > 0) { busy--; throw Object.assign(new Error('busy'), { injected: true }) } return f.apply(target, args) }
        } })
      },
    }),
    store: name => new MemoryStore(name),
    cache: async name => memoryCache(name),
    destroy: async name => { wipe(name) },
    timing: TIMING,
    hub: opts => Object.assign(new Hub(opts), { timing: { ...new Hub(opts).timing, get_retry: [20, 40], backoff_first: 30, backoff_max: 200, stream_stood: 50 } }),
  })
  const { client: c, sessions: joining } = await R.joinWithCode({ storage: storage('c'), hub_url: fake.url, room_id: a.model.room.room_id, code: recovery_code, device_name: 'c' })
  t.after(() => c.stop().catch(() => {}))
  const done = await joining
  // (another human device may add it meanwhile, 5.2.7: then it is a member before the second busy is asked for)
  assert.ok(busy < 2, 'the core said busy, and the join was tried again')
  assert.deepEqual([done.joined, done.not_joined.map(n => n.code)], [1, []])
})
