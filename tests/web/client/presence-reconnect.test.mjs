// Presence across a cut stream: an agent that leaves while a human device has no stream is offline once that device's
// stream is back, though its `online: false` was never heard (presence is not replayed; the hub's list of who is here,
// ended by `here`, is what a new stream is told). A device that stayed stays online. STAND-IN core and FAKE hub:
// nothing here is evidence against a hub.
import test from 'node:test'
import assert from 'node:assert/strict'
import { scene, sleep, until } from './helpers.mjs'

test('an agent that left while the stream was cut is offline once the stream is back', async t => {
  const { fake, a, b, agent } = await scene(t, { agent: true, second: true })
  await until(() => a.model.members.get(agent.device_id)?.is_online && a.model.members.get(b.my_device_id)?.is_online, 'a sees the agent and b online')

  // every stream is cut and none opens again for now
  let held = true
  fake.faults.add({ method: 'GET', path: '/v1/stream', when: () => held, refuse: { error: 'overloaded', status: 503 }, times: 1_000_000 })
  fake.dropStreams()
  await until(() => a.model.room.connection !== 'live', 'a lost its stream')
  // the agent goes while a hears nothing
  await agent.client.stop()
  await sleep(200)
  assert.equal(a.model.members.get(agent.device_id)?.is_online, true, 'a has not heard of it yet')

  const back = Date.now()
  held = false
  await until(() => a.model.room.connection === 'live', 'a has its stream again')
  await until(() => a.model.members.get(agent.device_id)?.is_online === false, 'the agent is offline on a')
  const m = a.model.members.get(agent.device_id)
  assert.ok(m.offline_since >= back, `gone since the stream came back, not unknown: ${m.offline_since}`)
  await until(() => a.model.members.get(b.my_device_id)?.is_online === true, 'b, which stayed, is online on a')
})
