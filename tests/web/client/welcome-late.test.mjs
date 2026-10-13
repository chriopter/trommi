// A Welcome joined later than the envelopes of its group (a helper session's name, written while this device could
// not read its Welcomes yet): what the group's envelopes said before the join is read once the device is in, also
// when a rescan ran meanwhile that still found the group missing, and whatever learning the group's past brings. A
// failed read of the hub's Welcomes is tried again by itself. STAND-IN core and FAKE hub.
import test from 'node:test'
import assert from 'node:assert/strict'
import { HubError } from '../../../app/web/core/hub.ts'
import { addAgent, scene, until } from './helpers.mjs'

const hexOf = bytes => Buffer.from(bytes).toString('hex')

test('a session\'s name, written before this device could read the session\'s Welcome, is read once the Welcome is joined', async t => {
  const { a, b } = await scene(t, { second: true })
  const hub = b.engine.hub, welcomes = hub.welcomes.bind(hub), groupInfo = hub.groupInfo.bind(hub)
  // B's reads of the hub's Welcomes fail (a flaky network), A's do not
  let failing = true, failed = 0
  hub.welcomes = async () => { if (failing) { failed++; throw new HubError('unavailable', 'the network is down', { status: 502 }) } return welcomes() }
  // learning the session's past brings B no rescan (as for a helper session whose walk the core refuses, 5.2.6, or one
  // joined at its founding): only the rule under test reads the envelope again
  hub.groupInfo = async (group, epoch) => { if (group.length !== 32) throw new HubError('not-found', 'no GroupInfo here', { status: 404 }); return groupInfo(group, epoch) }
  // a new agent: A founds its session with B in it; its name comes in an envelope B cannot read yet
  const agent = await addAgent(t, a, {}, 'agent2')
  const sid = agent.session_id
  const inSession = () => b.engine.groups.some(g => g.session && hexOf(g.session.sessionId) === sid)
  await agent.setRegister('profile', { agent_name: 'the new helper' })
  await agent.client.settle(); await a.settle()
  await until(() => a.model.sessions.get(sid)?.profile?.agent_name === 'the new helper', 'A reads the name')
  // B took the envelope, found the group missing, and its rescan found it missing still
  await until(() => failed > 0, 'B tried to read its Welcomes')
  await b.settle()
  assert.ok(!inSession(), 'B is not in the session yet')
  assert.notEqual(b.model.sessions.get(sid)?.profile?.agent_name, 'the new helper')
  // a failed read of the Welcomes is tried again by itself, ever later, while nothing happens in the room
  const tries = failed
  await until(() => failed >= tries + 2, 'B reads its Welcomes again by itself')
  // the network is back: B joins the session by itself and reads the name written before it joined
  failing = false
  await until(inSession, 'B joins the session by itself')
  await until(() => b.model.sessions.get(sid)?.profile?.agent_name === 'the new helper', 'B reads the name written before it joined')
})

test('a session archived before this device could read its Welcome is held as archived once the Welcome is joined', async t => {
  const { a, b } = await scene(t, { second: true })
  const hub = b.engine.hub, welcomes = hub.welcomes.bind(hub)
  let failing = true
  hub.welcomes = async () => { if (failing) throw new HubError('unavailable', 'the network is down', { status: 502 }); return welcomes() }
  const agent = await addAgent(t, a, {}, 'agent2')
  const sid = agent.session_id
  const held = () => b.engine.groups.find(g => g.session && hexOf(g.session.sessionId) === sid)
  await agent.setRegister('profile', { agent_name: 'a short helper' })
  await agent.client.settle()
  // A archives the session; the hub's word of it on the stream comes while B does not hold the group yet
  await a.setRegisters({ [`session/${sid}`]: { archived: true } })
  await until(() => a.engine.groups.find(g => g.session && hexOf(g.session.sessionId) === sid)?.archived, 'A archived the session')
  await a.settle(); await b.settle()
  assert.ok(!held(), 'B is not in the session yet')
  failing = false
  await until(held, 'B joins the session')
  await until(() => held()?.archived, 'B holds the session as archived, as the hub does')
  await b.settle()
  assert.equal(b.model.sessions.get(sid)?.is_active, false, 'and does not show it as open')
})
