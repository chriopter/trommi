// A new device that signs in with the recovery code (spec/v1.md 8.4) and then takes the hub's order: that it goes
// live, however the hub's answers are timed, and that a catch-up which takes long says how far it is. STAND-IN core
// (real groups and envelopes of the binding) and FAKE hub: nothing here is evidence against a hub.
import test from 'node:test'
import assert from 'node:assert/strict'
import { addHuman, rooms, scene, sleep, storage, until } from './helpers.mjs'

const headOf = fake => [...fake.state.rooms.values()][0].change
/** The last change of the hub's order that a page or the stream serves. A Commit's SealedKey takes the number after
 *  the Commit's (hub delivery.rs `commit_in`) and is served by neither, so a device that holds everything stands
 *  one below `headOf` while a Commit is the room's last word, until the next envelope or catch-up passes it. */
const servedOf = fake => [...fake.state.rooms.values()][0].changes.at(-1)?.change ?? 0

test('a first catch-up on a slow disk: the position moves while a page is taken, batch by batch, not only at its end', async t => {
  const { fake, a, agent, recovery_code } = await scene(t, { agent: true })
  for (let i = 0; i < 150; i++) { await a.saveNote({ text: `note ${i}` }); await agent.say({ text: `said ${i}` }); await a.sendMessage({ session_id: agent.session_id, text: `asked ${i}` }) }
  await a.settle(); await agent.settle()
  const head = headOf(fake)
  assert.ok(head >= 450, `the room holds ${head} changes: one page, more than two batches of it`)
  // every durable step of the new device's store takes 40 ms (in a browser it is one strict transaction: 10 ms
  // on a quiet disk, far more on a loaded one), and a page is reported at least every 20 ms
  const R = await rooms({ slow_store_ms: 40, timing: { progress: 20 } })
  const { client: c } = await R.joinWithCode({ storage: storage('c'), hub_url: fake.url, room_id: a.model.room.room_id, code: recovery_code, device_name: 'c' })
  t.after(() => c.stop().catch(() => {}))
  const seen = []
  c.on('change', change => { if (change.room && c.model.room.connection === 'catching_up') seen.push(c.model.room.last_envelope_number) })
  const pages = fake.requests.length
  await c.start()
  await until(() => c.model.room.connection === 'live', 'the new device live')
  assert.equal(fake.requests.slice(pages).filter(r => r.method === 'GET' && r.path === '/v1/changes' && Number(r.query.after) < head).length >= 1, true, 'it asked for the page')
  const steps = [...new Set(seen)]
  assert.deepEqual(steps, [...steps].sort((x, y) => x - y), 'the position only moves upwards')
  assert.ok(steps.filter(n => n < head).length >= 3, `the position was told at ${steps.length} places while the page was taken (${steps.join(', ')}): a page of ${head} changes must not stand at its start until it is through`)
  assert.ok(c.engine.position >= head)
})

test('twelve devices sign in with the code, one after another, into a room that is written to, the hub slow and failing at random: each goes live', async t => {
  const { fake, R, a, agent, recovery_code } = await scene(t, { agent: true })
  for (let i = 0; i < 10; i++) { await a.saveNote({ text: `note ${i}` }); await agent.say({ text: `said ${i}` }); await a.sendMessage({ session_id: agent.session_id, text: `asked ${i}` }) }
  await agent.askCard({ title: 'Asked before', options: [{ key: 'ok', label: 'OK' }] })
  await a.settle(); await agent.settle()
  // the same faults in every run: a small generator with a fixed seed
  let seed = 20261010
  const rnd = n => { seed = (seed * 1103515245 + 12345) & 0x7fffffff; return seed % n }
  const routes = [['GET', '/v1/changes'], ['GET', '/v1/welcomes'], ['GET', '/v1/stream'], ['POST', /\/tokens$/], ['PUT', '/v1/key-packages'], ['GET', /\/log$/], ['GET', '/v1/desk']]
  for (let n = 0; n < 12; n++) {
    let writing = true
    const writer = (async () => { for (let k = 0; writing; k++) { await (k % 2 ? agent.say({ text: `during ${n}/${k}` }) : a.saveNote({ text: `during ${n}/${k}` })).catch(() => {}); await sleep(5 + rnd(10)) } })()
    try {
      // while it joins: answers that take their time
      fake.faults.clear()
      for (const [method, path] of routes) if (rnd(2)) fake.faults.add({ method, path, delay_ms: rnd(40), times: 1 + rnd(3) })
      const { client: c } = await R.joinWithCode({ storage: storage('c'), hub_url: fake.url, room_id: a.model.room.room_id, code: recovery_code, device_name: `c${n}` })
      t.after(() => c.stop().catch(() => {}))
      const head = headOf(fake)
      // while it starts: slow answers, cut connections, `rate-limited` without a time (as the real hub answers a
      // KeyPackage upload right after a log-in), a sign-in to make again, a hub that fails
      for (const [method, path] of routes) {
        const kind = rnd(6)
        if (kind === 0) fake.faults.add({ method, path, delay_ms: rnd(80), times: 1 + rnd(3) })
        else if (kind === 1) fake.faults.add({ method, path, drop: ['before', 'after', 'mid'][rnd(3)], times: 1 + rnd(2) })
        else if (kind === 2) fake.faults.add({ method, path, refuse: { error: 'rate-limited', status: 429 }, times: 1 + rnd(3) })
        else if (kind === 3) fake.faults.add({ method, path, refuse: { error: 'unauthorised', status: 401 }, times: 1 + rnd(2) })
        else if (kind === 4) fake.faults.add({ method, path, raw: { status: 503, body: 'no' }, times: 1 + rnd(2) })
      }
      const alerts = []
      c.engine.on('alert', x => alerts.push(x.code))
      await c.start()
      // the writes go on until it reached the hub's change of its sign-in; then they stop, so that it can be seen
      // live: a writer that outpaces a loaded machine keeps every device in a catch-up pass, whatever the code does
      await until(() => c.engine.position >= head, () => `device ${n} at the hub's change ${head} (it holds ${c.model.room.connection} at ${c.engine.position}, alerts: ${alerts.join(', ') || 'none'})`, 30_000)
      writing = false
      await writer
      // (live, and holding all of the hub's order it can be served: not the room's last number, which may be a SealedKey)
      await until(() => c.model.room.connection === 'live' && c.engine.position >= servedOf(fake), () => `device ${n} live and at the hub's change ${servedOf(fake)} (the room's last number ${headOf(fake)}; it holds ${c.model.room.connection} at ${c.engine.position}, alerts: ${alerts.join(', ') || 'none'})`, 20_000)
      await c.stop()
    } finally { writing = false; await writer }
  }
  fake.faults.clear()
  await a.settle()
  assert.equal([...a.model.members.values()].filter(m => m.device_role === 'human' && m.is_active).length, 13, 'the founder sees all twelve')
})

test('a live device after a Commit that is the room\'s last word: it holds all the hub serves, one below the SealedKey\'s number, and stays live', async t => {
  const { fake, R, a } = await scene(t)
  const b = await addHuman(t, R, a, 'b')
  const x = await addHuman(t, R, a, 'x')
  await a.saveNote({ text: 'before' })
  await a.settle(); await b.settle()
  await until(() => b.engine.position >= servedOf(fake), 'b holds the room')
  await a.removeDevices([x.my_device_id])
  await a.settle()
  // the Commit came on the stream; its SealedKey took the room's last number, which no stream event carries
  await until(() => b.engine.position >= servedOf(fake), () => `b at the Commit ${servedOf(fake)} (it holds ${b.engine.position})`)
  assert.equal(headOf(fake), servedOf(fake) + 1, 'the room\'s last number is the SealedKey\'s')
  await sleep(300)
  assert.equal(b.model.room.connection, 'live')
  assert.equal(b.engine.position, servedOf(fake), 'nothing more to take: no catch-up is due for a number nothing is served under')
  // the next envelope passes it
  await a.saveNote({ text: 'after' })
  await until(() => b.engine.position === headOf(fake), 'b at the next envelope')
})

test('a device back after long stocks its KeyPackages before its catch-up is through, though the count it kept says the hub holds enough', async t => {
  const { fake, R, a, agent } = await scene(t, { agent: true })
  const b = await addHuman(t, R, a, 'b')
  await b.stop()
  for (let i = 0; i < 20; i++) await agent.say({ text: `said ${i}` })
  await a.settle(); await agent.settle()
  // while it was away its KeyPackages ran out at the hub (handed out, or older than 90 days): none is left
  const room = [...fake.state.rooms.values()][0]
  assert.ok(room.key_packages.delete(Buffer.from(b.engine.device_id).toString('base64url')))
  assert.equal((await a.engine.hub.claimKeyPackages([b.engine.device_id], { partial: true })).length, 0, 'none at the hub')
  // the catch-up is slow: every page of the hub's order takes a second and a half
  fake.faults.add({ method: 'GET', path: '/v1/changes', delay_ms: 1500, times: 1e6 })
  const again = await R.openRoom({ storage: b.stored_as })
  t.after(() => again.stop().catch(() => {}))
  const asked = fake.requests.length
  const uploads = () => fake.requests.slice(asked).filter(r => r.method === 'PUT' && r.path === '/v1/key-packages' && r.status === 200 && (r.body?.single_use ?? []).length > 0)
  const starting = again.start({ stream: false })
  await until(() => uploads().length, 'the device uploads KeyPackages', 5000)
  assert.notEqual(again.model.room.connection, 'live', 'while it still catches up')
  assert.equal(fake.requests.slice(asked).filter(r => r.method === 'GET' && r.path === '/v1/changes' && r.status !== null).length, 0, 'before the first page of its catch-up was answered')
  // a session founded now finds one of it
  assert.equal((await a.engine.hub.claimKeyPackages([b.engine.device_id], { partial: true })).length, 1, 'its KeyPackage is at the hub')
  fake.faults.clear()
  await starting
})
