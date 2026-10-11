// A fresh sign-in on a new browser into a room with a past: the timeline of what its Desk shows, change by change.
// The Desk holds only what the hub's Desk lists as open; an old question whose answer comes later in the history never
// stands on it, a request not read yet is never offered to allow, the agents' presence and the session's name arrive
// without anything else happening, and the room says it is still being read meanwhile. STAND-IN core and FAKE hub:
// nothing here is evidence against a hub.
import test from 'node:test'
import assert from 'node:assert/strict'
import { rooms, scene, sleep, storage, until } from './helpers.mjs'

test('a fresh sign-in: the Desk never shows a settled question, unread requests wait quietly, presence and names fill in', async t => {
  const { fake, a, agent, recovery_code } = await scene(t, { agent: true })
  await agent.setRegister('profile', { agent_name: 'trommi' })
  // the past: requests answered, cards answered and closed, requests that ran out unanswered (the hub still lists
  // those as open: it cannot see an expiry)
  const settled = new Set()
  for (let i = 0; i < 12; i++) {
    const id = await agent.askPermission({ tool_name: 'Bash', description: `old ${i}`, input_preview: 'ls' })
    await until(() => a.model.permissions.get(id), 'the founder sees the request')
    await a.verdict({ object_id: id, allow: i % 2 === 0 })
    settled.add(id)
  }
  for (let i = 0; i < 3; i++) {
    const id = await agent.askCard({ title: `old card ${i}`, options: [{ key: 'y', label: 'Yes' }] })
    await until(() => a.model.cards.get(id), 'the founder sees the card')
    await a.answer({ object_id: id, choices: ['y'] })
    // (the agent closes a card of its own once the hub's copy of it came back: before that its core does not hold
    //  the object, and a later version of it is forbidden; a slow hub showed it in CI)
    await agent.client.settle()
    await agent.closeCard(id, { close_summary: 'done' })
    settled.add(id)
  }
  const expired = new Set()
  for (let i = 0; i < 8; i++) expired.add(await agent.askPermission({ tool_name: 'Bash', description: `ran out ${i}`, input_preview: 'ls' }, Date.now() + 200))
  const open = []
  for (let i = 0; i < 3; i++) open.push(await agent.askCard({ title: `open ${i}`, options: [{ key: 'y', label: 'Yes' }] }))
  const openRequest = await agent.askPermission({ tool_name: 'Bash', description: 'the open one', input_preview: 'ls' })
  await agent.client.settle(); await a.settle()
  await sleep(300)

  // the new browser: the session's history is slow to read, so its join comes behind the founder's Add (5.2.7)
  fake.faults.add({ method: 'GET', path: /\/log$/, delay_ms: 400, times: 1000 })
  fake.faults.add({ method: 'GET', path: '/v1/changes', delay_ms: 100, times: 1000 })
  const R = await rooms()
  const { client: c, sessions } = await R.joinWithCode({ storage: storage('c'), hub_url: a.model.room.hub_url, room_id: a.model.room.room_id, code: recovery_code, device_name: 'c' })
  t.after(() => c.stop().catch(() => {}))
  const wrong = [], seen = { reading_empty: false }
  const look = () => {
    const m = c.model
    for (const id of m.stack) if (!open.includes(id)) wrong.push(`card ${settled.has(id) ? 'settled' : 'unknown'} on the Desk`)
    for (const id of m.open_permission_ids) {
      const p = m.permissions.get(id)
      // (one not read yet is shown as loading by the views, never as a question; one read must be the open one)
      if (p.content_state === 'ok' && id !== openRequest) wrong.push(`request ${settled.has(id) ? 'answered' : expired.has(id) ? 'ran out' : 'unknown'} offered to allow`)
      if (settled.has(id)) wrong.push('an answered request on the Desk')
    }
    if (m.room.reading && !m.stack.length) seen.reading_empty = true
  }
  c.on('change', look)
  look()
  assert.equal(c.model.room.reading, true, 'the room is being read from the start: the Desk says so, not "all quiet"')
  await c.start()
  await sessions
  await until(() => {
    const m = c.model, s = m.sessions.get(agent.session_id)
    return m.stack.length === 3 && open.every(id => m.cards.get(id)?.content_state === 'ok') && m.permissions.get(openRequest)?.content_state === 'ok'
      && s?.profile?.agent_name === 'trommi' && !m.room.reading
  }, 'the Desk complete and read, the session named, the reading over', 20_000)
  await until(() => c.model.members.get(agent.device_id)?.is_online, 'the agent is here at once: the stream said so when it opened')
  assert.deepEqual(wrong, [], 'what stood on the Desk on the way')
  assert.ok(seen.reading_empty, 'while nothing stood on the Desk yet, the room said it was being read')
  assert.deepEqual(c.model.open_permission_ids, [openRequest])
  assert.deepEqual(new Set(c.model.stack), new Set(open))
})
