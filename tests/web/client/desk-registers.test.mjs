// A sign-in shows the Desk's registers with the Desk: the names of sessions, the person's settings of them, the
// desks, a snooze, the names of the other devices. They come with the hub's Desk as each writer's newest value and
// stand from the overlay of the device (spec 9.3.6) before the reading of the past reaches them; before, each waited for that
// reading, and a snoozed card stood on the Desk meanwhile.
// The REAL core (real envelopes) and the FAKE hub.
import test from 'node:test'
import assert from 'node:assert/strict'
import { rooms, scene, storage, until } from './helpers.mjs'

test('a sign-in shows names, desks, settings and snoozes with the Desk, before the past is read', async t => {
  const { fake, a, agent, recovery_code } = await scene(t, { agent: true })
  const sid = agent.session_id
  // a long past, the registers in the middle of it, and more after them
  const talk = async (n, what) => { for (let i = 0; i < n; i++) { await agent.say({ text: `${what} ${i}` }); if (i % 50 === 0) await agent.settle() } await agent.settle() }
  await talk(400, 'before')
  const options = [{ key: 'y', label: 'Y' }, { key: 'n', label: 'N' }]
  const snoozed = await agent.askCard({ title: 'put off', options })
  const waiting = await agent.askCard({ title: 'waits', options })
  await agent.settle()
  await agent.client.setRegisters({ profile: { agent_name: 'Probe Agent', title: 'Probing' } }, { session_id: sid })
  await a.setRegisters({ [`session/${sid}`]: { name: 'Named By Human', icon: 'pen' } })
  await a.setRegisters({ [`snooze/${snoozed}`]: { until: Date.now() + 3600_000 } })
  await a.setRegisters({ 'desk/lab': { name: 'Lab', created_at: 5 } })
  await talk(400, 'after')
  await a.settle()
  await until(() => a.model.stack.length === 1, 'the first device puts the card off')

  const R = await rooms({ slow_store_ms: 25, timing: { progress: 10 } })
  const { client: c, sessions } = await R.joinWithCode({ storage: storage('c'), hub_url: fake.url, room_id: a.model.room.room_id, code: recovery_code, device_name: 'c' })
  t.after(() => c.stop().catch(() => {}))
  let first = null
  const look = () => {
    const m = c.model
    if (first || !m.cards.has(waiting)) return
    first = {
      read: m.room.reading_progress?.done ?? 0, total: m.room.reading_progress?.total ?? 0, oldest: [...(m.timelines.get(`chat:session/${sid}`)?.items.values() ?? [])].some(i => i.content?.text === 'before 0'), stack: [...m.stack],
      profile: m.sessions.get(sid)?.profile?.agent_name ?? null, name: m.sessions.get(sid)?.settings?.name ?? null,
      desks: [...m.human.desks.keys()].sort(), snoozed: m.cards.get(snoozed)?.snoozed_until ?? null,
      named_a: m.members.get(a.my_device_id)?.device_name ?? null,
    }
  }
  c.on('change', look)
  await c.start()
  await sessions
  await until(() => first, 'the Desk shows')
  // (the agent's oldest line, 400 lines before the registers in the hub's order, comes only with the past)
  assert.ok(!first.oldest, 'the Desk showed before the past was read')
  assert.deepEqual(first.stack, [waiting], 'the snoozed card is off the Desk from the first showing on')
  assert.ok(first.snoozed, 'the snooze stands')
  assert.equal(first.profile, 'Probe Agent', 'the session\'s profile stands')
  assert.equal(first.name, 'Named By Human', 'the person\'s name for the session stands')
  assert.deepEqual(first.desks, ['lab', 'main'], 'both desks stand')
  assert.equal(first.named_a, 'a', 'the other device has its name')
  await until(() => c.model.room.connection === 'live' && !c.engine.rescanPending && !c.model.room.reading, 'the past read', 60_000)
  assert.deepEqual([...c.model.stack], [waiting])
  assert.ok([...(c.model.timelines.get(`chat:session/${sid}`)?.items.values() ?? [])].some(i => i.content?.text === 'before 0'), 'the past brings the oldest line (what the first showing lacked)')
  assert.equal(c.model.sessions.get(sid)?.settings?.name, 'Named By Human')
})
