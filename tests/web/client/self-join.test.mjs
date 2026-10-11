// Self-join (spec/v1.md 12.4) on the human's side: an agent asks in its main session to let in a new session of its
// machine and account (`join_ask/<nonce>`); the client answers with an agent invite link (`join_link/<nonce>`),
// confirms the Request on its own only when the ask names that Request's hash, names the new session after the ask
// with who vouched, and removes the new device when its session is archived. A Request the ask does not name is
// refused. The invite handshake and its Commits are the REAL core's; content is the stand-in's; the hub is FAKE.
import test from 'node:test'
import assert from 'node:assert/strict'
import { hubReaders } from '../stand-in/core.ts'
import { rooms, scene, storage, until } from './helpers.mjs'

const ask = (more = {}) => ({ name: 'second', folder: '~/project', host: 'desk', account: 'a1b2c3d4e5f60718', account_label: 'claude .claude', vouched_by: 'project', at: Date.now(), request_hash: null, ...more })
const linkOf = (a, agent, nonce) => a.model.sessions.get(agent.session_id)?.registers.get(`join_link/${nonce}`)?.value

test('an ask of an agent is answered with a link, and the Request it names is let in without the emoji', async t => {
  const { a, agent } = await scene(t, { agent: true })
  await agent.setRegister('join_ask/n1', ask())
  const answer = await until(() => linkOf(a, agent, 'n1'), 'the link for the ask')
  assert.ok(answer.link && answer.invite_id, JSON.stringify(answer))
  const invite = a.model.invites.get(answer.invite_id)
  assert.ok(invite.vouch, 'the invite remembers the ask it answers')
  assert.equal(invite.confirm_code, false)

  const R = await rooms({ agent: true })
  const joining = R.joinRoom({ link: answer.link, storage: storage('second'), poll_ms: 20 })
  joining.client.catch(() => {})
  await until(() => a.model.invites.get(answer.invite_id)?.invite_state === 'confirm_code', 'the Request')
  // without the hash in the ask, nothing is confirmed
  await new Promise(r => setTimeout(r, 300))
  assert.equal(a.model.invites.get(answer.invite_id).invite_state, 'confirm_code')
  const request = (await a.hub.getInvite(Buffer.from(answer.invite_id, 'hex'))).requests[0]
  await agent.setRegister('join_ask/n1', ask({ request_hash: hubReaders.requestHash(request.request, request.mac) }))
  await until(() => a.model.invites.get(answer.invite_id)?.invite_state === 'joined', 'let in on its own')
  const second = await joining.client
  t.after(() => second.stop().catch(() => {}))

  const session = await until(() => [...a.model.sessions.values()].find(s => s.session_id !== agent.session_id && s.settings?.vouched), 'the new session, named')
  assert.equal(session.settings.name, 'second')
  assert.deepEqual(session.settings.vouched, { by: agent.session_id, by_name: a.model.sessions.get(agent.session_id).settings?.name ?? 'project', account: 'a1b2c3d4e5f60718', account_label: 'claude .claude' })

  // archived: its device goes with it
  const device = session.agent_device_id
  await a.setRegisters({ [`session/${session.session_id}`]: { ...session.settings, archived: true } })
  await until(() => !(a.engine.roles?.agents ?? []).some(d => Buffer.from(d).toString('hex') === device), 'the device removed')
})

test('a Request the ask does not name is refused, and the invite is spent', async t => {
  const { a, agent } = await scene(t, { agent: true })
  await agent.setRegister('join_ask/n2', ask())
  const answer = await until(() => linkOf(a, agent, 'n2'), 'the link for the ask')
  const R = await rooms({ agent: true })
  const joining = R.joinRoom({ link: answer.link, storage: storage('other'), poll_ms: 20 })
  joining.client.catch(() => {})
  await until(() => a.model.invites.get(answer.invite_id)?.invite_state === 'confirm_code', 'the Request')
  await agent.setRegister('join_ask/n2', ask({ request_hash: 'A'.repeat(43) }))
  await until(() => a.model.invites.get(answer.invite_id)?.invite_state === 'failed', 'refused')
  assert.equal(a.model.invites.get(answer.invite_id).error, 'code-mismatch')
  joining.cancel()
})

test('an ask without an account is refused, and nothing is opened for it', async t => {
  const { a, agent } = await scene(t, { agent: true })
  const before = a.model.invites.size
  await agent.setRegister('join_ask/n3', ask({ account: '' }))
  const answer = await until(() => linkOf(a, agent, 'n3'), 'the answer')
  assert.deepEqual(answer, { refused: 'bad-ask' })
  assert.equal(a.model.invites.size, before)
})
