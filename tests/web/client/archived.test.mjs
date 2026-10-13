// Only active sessions are loaded (hub-api.md point 46): a catch-up and the Desk leave an archived session's envelopes
// out, and the session is loaded when its page is opened. A device that was away while the session was archived learns
// it from the hub's list on start. STAND-IN core and FAKE hub.
import test from 'node:test'
import { hex } from '../../../app/web/core/ids.ts'
import assert from 'node:assert/strict'
import { addAgent, memoryCache, scene, storage, until } from './helpers.mjs'

test('an archived session is not loaded at the start, only when it is opened; a device away at the archiving learns it', async t => {
  const { fake, R, a, b, agent } = await scene(t, { agent: true, second: true })
  const sid = agent.session_id, key = `chat:session/${sid}`
  await b.settle()
  await b.stop()
  // while B is away: the session's past, then the session is archived
  await agent.setRegister('profile', { agent_name: 'old helper' })
  const card = await agent.askCard({ title: 'An old question', options: [{ key: 'y', label: 'Yes' }] })
  await agent.say({ text: 'said in the old session' })
  await agent.client.settle(); await a.settle()
  await until(() => a.model.cards.get(card), 'A sees the card')
  await a.setRegisters({ [`session/${sid}`]: { archived: true } })
  await until(() => a.engine.groups.find(g => g.session && g.archived), 'A archived the session')
  await a.settle()

  const from = fake.requests.length
  const again = await R.openRoom({ storage: b.stored_as })
  t.after(() => again.stop().catch(() => {}))
  await again.start()
  await again.settle()
  await until(() => again.engine.groups.find(g => g.session && g.archived), 'B learned that the session was archived')
  const asked = fake.requests.slice(from).filter(r => r.method === 'GET' && r.path === '/v1/changes')
  assert.ok(asked.length && asked.every(r => r.query.active === '1'), 'every catch-up asks for the live sessions alone')
  assert.equal(again.model.cards.get(card), undefined, 'the archived session\'s card did not come with the catch-up')
  assert.notEqual(again.model.sessions.get(sid)?.profile?.agent_name, 'old helper')

  // opened: the session's envelopes, in one pass over its group
  const before = fake.requests.length
  await again.loadTimeline(key, { limit: 50 })
  await until(() => again.model.cards.get(card)?.title === 'An old question', 'the card, once the session is opened')
  assert.equal(again.model.sessions.get(sid)?.profile?.agent_name, 'old helper', 'its name too')
  assert.ok([...again.model.timelines.get(key).items.values()].some(i => i.content?.text === 'said in the old session'))
  const group = fake.requests.slice(before).filter(r => r.path === '/v1/changes')
  assert.equal(group.length, 1, 'one request for the whole session')
  // opened again: nothing more is asked
  const twice = fake.requests.length
  await again.loadArchived(sid)
  assert.equal(fake.requests.slice(twice).filter(r => r.path === '/v1/changes').length, 0)

  // a model built again from nothing (a cache that is not this device's): the rescan reads the live sessions alone,
  // and the archived session this device opened is loaded again with it, not lost until it is opened once more
  await again.settle()
  await again.stop()
  const cache = memoryCache(b.stored_as.name)
  const at = await cache.get('client/at')
  await cache.set('client/at', { ...at, cursor: at.cursor + 3 })
  const cold = await R.openRoom({ storage: b.stored_as })
  t.after(() => cold.stop().catch(() => {}))
  assert.equal(cold.model.cards.size, 0, 'nothing of the dropped cache is shown')
  await cold.start()
  await until(() => cold.model.cards.get(card)?.title === 'An old question' && cold.model.sessions.get(sid)?.profile?.agent_name === 'old helper', 'the opened archived session, back after the rebuild')
})

test('a session archived while a catch-up runs: what the stream brought of it meanwhile is taken, though the pages of the live sessions leave it out (torture seed 103)', async t => {
  const { fake, a, b, agent } = await scene(t, { agent: true, second: true })
  const sid = agent.session_id
  await a.settle(); await b.settle()
  // B's catch-up is slow: each page takes a while (a cut stream, a hub restarting)
  let slow = true
  fake.faults.add({ method: 'GET', path: '/v1/changes', delay_ms: 1500, times: 1e6, when: () => slow })
  const pass = b.catchUp().catch(() => {})
  await new Promise(r => setTimeout(r, 100))
  // meanwhile the agent names its session, and the session is archived right after: the name comes on B's stream
  // while B reads pages, which leave it to the pages; the page answered after the archiving leaves the session out
  await agent.setRegister('profile', { agent_name: 'late name' })
  await until(() => a.model.sessions.get(sid)?.profile?.agent_name === 'late name', 'A sees the name')
  await a.setRegisters({ [`session/${sid}`]: { archived: true } })
  await until(() => a.engine.groups.find(g => g.session && g.archived), 'A archived the session')
  await pass
  slow = false
  await b.settle()
  await until(() => b.model.sessions.get(sid)?.profile?.agent_name === 'late name', 'B shows the name the stream brought during its catch-up', 10_000)
})

test('a session joined late and archived at once: what was written in it before the join is read from its own group, archived or not (torture seed 103)', async t => {
  const { fake, R, a, recovery_code } = await scene(t, { agent: true })
  await a.settle()
  // a device signed in with the code (its rescans read the hub, not what it kept of its own run)
  const { client: b } = await R.joinWithCode({ storage: storage('b'), hub_url: fake.url, room_id: a.model.room.room_id, code: recovery_code, device_name: 'b' })
  t.after(() => b.stop().catch(() => {}))
  await b.start(); await b.settle(); await until(() => !b.engine.rescanPending, 'b read the room')
  // B cannot read its Welcomes for a while (a cut network): what the new session brings meanwhile waits for its group
  let away = true
  const bOnly = rq => (rq.headers?.authorization ?? '') === `Bearer ${b.engine.hub.token}`
  fake.faults.add({ method: 'GET', path: '/v1/welcomes', when: rq => away && bOnly(rq), refuse: { error: 'overloaded', status: 503 }, times: 1e6 })
  const agent2 = await addAgent(t, a, {}, 'agent2')
  const sid = agent2.session_id
  await agent2.setRegister('profile', { agent_name: 'quick helper' })
  await until(() => a.model.sessions.get(sid)?.profile?.agent_name === 'quick helper', 'A sees the name')
  await until(() => b.model.room.last_envelope_number >= a.model.room.last_envelope_number, 'B passed the name, without its group')
  // closed at once, before B could join it
  await a.setRegisters({ [`session/${sid}`]: { archived: true } })
  await until(() => a.engine.groups.find(g => g.session && hex(g.session.sessionId) === sid && g.archived), 'A archived the session')
  await a.settle()
  away = false
  await until(() => b.engine.groups.some(g => g.session && hex(g.session.sessionId) === sid), 'B joined the session by its Welcome')
  await until(() => b.model.sessions.get(sid)?.profile?.agent_name === 'quick helper', 'B shows the name written before its join', 15_000)
})
