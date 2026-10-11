// Key requests (spec/v1.md 7.1.1): a human device that lacks content keys asks the human devices
// (`POST /v1/requests`, kind `handover`), and a human device that holds them answers with a key handover in the room
// group. The groups, the keys and the handover are the REAL core's; the hub is the FAKE one. Whether a key is
// missing is the core's word (an envelope that did not open for lack of it): the stand-in's content opens without
// keys, so here the core's list is handed in.
import test from 'node:test'
import assert from 'node:assert/strict'
import { addHuman, scene, sleep, until } from './helpers.mjs'

const b64 = bytes => Buffer.from(bytes).toString('base64url')
const handovers = fake => fake.requests.filter(r => r.method === 'POST' && r.path === '/v1/requests' && r.body?.kind === 'handover')
const listReads = fake => fake.requests.filter(r => r.method === 'GET' && r.path === '/v1/requests').length
const messages = (fake, group) => fake.requests.filter(r => r.method === 'POST' && r.path === `/v1/groups/${b64(group)}/messages` && r.status === 200)

test('a human device asks for the keys the core says it lacks, as a handover request naming the group', async t => {
  const { fake, R, a } = await scene(t)
  const b = await addHuman(t, R, a, 'b')
  const room = b.engine.groups.find(g => g.session === null).group
  const device = b.engine.device
  const real = device.keyRequests.bind(device)
  let asked = 0
  device.keyRequests = async now => { asked++; return asked === 1 ? [{ group: room, epochs: [0] }] : real(now) }
  b.engine.keepSoon(0)
  await until(() => handovers(fake).length === 1, 'the request reaches the hub')
  const [request] = handovers(fake)
  assert.equal(request.body.group, b64(room))
  assert.equal(request.status, 200)
  await b.settle(); await sleep(50)
  assert.equal(handovers(fake).length, 1, 'asked once: the core says when again')
})

test('another human device answers a request once, with a key handover in the room group', async t => {
  const { fake, R, a } = await scene(t)
  const b = await addHuman(t, R, a, 'b')
  const room = a.engine.groups.find(g => g.session === null).group
  const answered = []
  const device = a.engine.device
  const real = device.answerKeyRequest.bind(device)
  device.answerKeyRequest = async (group, recipient, now) => { answered.push([group, recipient]); return real(group, recipient, now) }
  const before = messages(fake, room).length
  // b's wish, as its engine posts it: seen by a on the stream, then again in the hub's list at every upkeep
  await b.engine.hub.postRequest({ kind: 'handover', group: room })
  await until(() => messages(fake, room).length > before, 'a sends a key handover in the room group')
  // the hub's list at the next upkeeps shows the same wish
  a.engine.timing.upkeep_every = 1
  for (let i = 0; i < 3; i++) { a.engine.keepSoon(0); await sleep(60) }
  assert.ok(listReads(fake) >= 1, 'the list was read')
  assert.equal(answered.length, 1, 'one answer, though the stream and the list both show the wish')
  assert.equal(Buffer.from(answered[0][1]).toString('hex'), b.my_device_id)
  // b takes it: keys it holds already are not replaced, nothing breaks
  a.engine.timing.upkeep_every = 600_000
  await a.settle(); await b.settle()
  assert.equal(b.model.alerts.length, 0, JSON.stringify(b.model.alerts))
})

test('a device does not answer its own request, and a wish older than a day is passed over', async t => {
  const { fake, R, a } = await scene(t)
  const b = await addHuman(t, R, a, 'b')
  const answered = []
  for (const x of [a, b]) {
    const device = x.engine.device, real = device.answerKeyRequest.bind(device)
    device.answerKeyRequest = async (group, recipient, now) => { answered.push(x); return real(group, recipient, now) }
  }
  // a's own wish: nobody but b answers it
  await a.engine.hub.postRequest({ kind: 'handover', group: null })
  await until(() => answered.length >= 1, 'b answers')
  await sleep(200)
  assert.deepEqual(answered, [b])
  // an old wish in the hub's list (the hub keeps them seven days): not answered
  const row = [...fake.state.rooms.values()][0].requests.at(-1)
  row.id += 1000; row.at -= 2 * 86_400_000
  b.engine.timing.upkeep_every = 1
  for (let i = 0; i < 3; i++) { b.engine.keepSoon(0); await sleep(60) }
  assert.ok(listReads(fake) >= 1, 'the list was read')
  assert.deepEqual(answered, [b])
})
