// A session group that holds a leaf the room no longer allows is stale (5.2.8): nothing is written in it until a
// human device commits the Remove of that leaf. Here the leaf is a device that signed in with the code, joined the
// sessions and never came (a login that hung), and was removed from the room. STAND-IN core, FAKE hub.
import test from 'node:test'
import assert from 'node:assert/strict'
import { addHuman, rooms, scene, storage, until } from './helpers.mjs'

/** A device that signs in with the code, joins the room and the sessions, and is gone after that. */
async function ghost(t, fake, a, recovery_code) {
  const R = await rooms()
  const { client, sessions } = await R.joinWithCode({ storage: storage('ghost'), hub_url: fake.url, room_id: a.model.room.room_id, code: recovery_code })
  assert.deepEqual((await sessions).not_joined, [])
  const id = client.my_device_id
  await client.stop()
  return id
}

test('the device that removed a ghost from the room heals the session it was in, and a message goes out', async t => {
  const { fake, a, agent, recovery_code } = await scene(t, { agent: true })
  await a.settle(); await agent.settle()
  const g = await ghost(t, fake, a, recovery_code)
  await a.settle()
  await a.removeDevices([g])
  await a.settle()
  await a.sendMessage({ session_id: agent.session_id, text: 'after the ghost' })
  await a.settle()
  await until(async () => { await agent.settle(); return [...(agent.client.model.timelines.get(`chat:session/${agent.session_id}`)?.items.values() ?? [])].some(i => i.content?.text === 'after the ghost') }, 'the agent reads it')
})

test('a ghost removed by another device that went away at once: this device heals the session itself and its message goes out', async t => {
  const { fake, R, a, agent, recovery_code } = await scene(t, { agent: true })
  const b = await addHuman(t, R, a, 'b', { stream: false })
  await a.settle(); await agent.settle(); await b.catchUp()
  const g = await ghost(t, fake, a, recovery_code)
  await b.catchUp()
  // b removes the ghost from the room only, and is gone before it cleans the session
  const room = b.core.roomGroupId(b.room_id)
  await b.engine.land(async d => d.removeHumanDevices([await d.cutOf(room, Buffer.from(g, 'hex'))], Date.now()))
  await b.stop()
  await a.settle()
  await a.sendMessage({ session_id: agent.session_id, text: 'healed here' })
  await a.settle()
  await until(async () => { await agent.settle(); return [...(agent.client.model.timelines.get(`chat:session/${agent.session_id}`)?.items.values() ?? [])].some(i => i.content?.text === 'healed here') }, 'the agent reads it')
})

test('a message to a session that is stale on this device (it removed a ghost from the room only): the session is repaired first and the message goes out', async t => {
  const { fake, R, a, agent, recovery_code } = await scene(t, { agent: true })
  const b = await addHuman(t, R, a, 'b')
  await a.settle(); await agent.settle(); await b.settle()
  const g = await ghost(t, fake, a, recovery_code)
  await b.settle()
  // b removes the ghost from the room and nothing more: the session it is in is stale on b now
  const room = b.core.roomGroupId(b.room_id)
  await a.stop()
  await b.engine.land(async d => d.removeHumanDevices([await d.cutOf(room, Buffer.from(g, 'hex'))], Date.now()))
  const group = b.engine.groups.find(x => x.session)
  assert.ok(group.stale, 'the session is stale on b')
  await b.sendMessage({ session_id: agent.session_id, text: 'repaired first' })
  await b.settle()
  assert.equal(b.engine.groups.find(x => x.session).stale, false)
  await until(async () => { await agent.settle(); return [...(agent.client.model.timelines.get(`chat:session/${agent.session_id}`)?.items.values() ?? [])].some(i => i.content?.text === 'repaired first') }, 'the agent reads it')
})
