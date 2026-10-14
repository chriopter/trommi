// Self-join (spec/v1.md 12.4) on the human's side: an agent asks in its main session to let in a new session of its
// machine and account (`join_ask/<nonce>`, the nonce 16 random bytes as hex, spec 12.4.2); it waits for the person's one tap (`answerVouch`): Allow answers with an
// agent invite link (`join_link/<nonce>`) whose Request is confirmed when the ask names its hash, Deny refuses. The
// new session is named after the ask with who vouched, and its device goes when it is archived. A Request the ask
// does not name is refused; asks past the bounds are refused without asking. The invite handshake and its Commits are the REAL core's; content is the stand-in's; the hub is FAKE.
import test from 'node:test'
import assert from 'node:assert/strict'
import { hubReaders } from '../stand-in/core.ts'
import { rooms, scene, storage, until } from './helpers.mjs'

const ask = (more = {}) => ({ name: 'second', folder: '~/project', host: 'desk', account: 'a1b2c3d4e5f60718', account_label: 'claude .claude', vouched_by: 'project', at: Date.now(), request_hash: null, ...more })
const linkOf = (a, agent, nonce) => a.model.sessions.get(agent.session_id)?.registers.get(`join_link/${nonce}`)?.value

const sees = (a, agent, nonce) => until(() => a.model.sessions.get(agent.session_id)?.registers.get(`join_ask/${nonce}`), 'the ask')

test('an ask waits for the tap; Allow answers with a link, and the Request it names is let in without the emoji', async t => {
  const { a, agent } = await scene(t, { agent: true })
  await agent.setRegister('join_ask/a1', ask())
  await sees(a, agent, 'a1')
  await new Promise(r => setTimeout(r, 500))
  assert.equal(linkOf(a, agent, 'a1'), undefined, 'nothing is answered before the person taps')
  assert.equal([...a.model.invites.values()].filter(i => i.vouch).length, 0, 'nor any link opened')
  await a.answerVouch('a1', true)
  const answer = await until(() => linkOf(a, agent, 'a1'), 'the link for the ask')
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
  await agent.setRegister('join_ask/a1', ask({ request_hash: hubReaders.requestHash(request.request, request.mac) }))
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
  await agent.setRegister('join_ask/a2', ask())
  await sees(a, agent, 'a2')
  await a.answerVouch('a2', true)
  const answer = await until(() => linkOf(a, agent, 'a2'), 'the link for the ask')
  const R = await rooms({ agent: true })
  const joining = R.joinRoom({ link: answer.link, storage: storage('other'), poll_ms: 20 })
  joining.client.catch(() => {})
  await until(() => a.model.invites.get(answer.invite_id)?.invite_state === 'confirm_code', 'the Request')
  await agent.setRegister('join_ask/a2', ask({ request_hash: 'A'.repeat(43) }))
  await until(() => a.model.invites.get(answer.invite_id)?.invite_state === 'failed', 'refused')
  assert.equal(a.model.invites.get(answer.invite_id).error, 'code-mismatch')
  joining.cancel()
})

test('an ask without an account is refused, and nothing is opened for it', async t => {
  const { a, agent } = await scene(t, { agent: true })
  const before = a.model.invites.size
  await agent.setRegister('join_ask/a3', ask({ account: '' }))
  const answer = await until(() => linkOf(a, agent, 'a3'), 'the answer')
  assert.deepEqual(answer, { refused: 'bad-ask' })
  assert.equal(a.model.invites.size, before)
})

test('Deny refuses the ask, and nothing is opened for it', async t => {
  const { a, agent } = await scene(t, { agent: true })
  const before = a.model.invites.size
  await agent.setRegister('join_ask/a4', ask())
  await sees(a, agent, 'a4')
  await a.answerVouch('a4', false)
  assert.deepEqual(await until(() => linkOf(a, agent, 'a4'), 'the answer'), { refused: 'denied' })
  assert.equal(a.model.invites.size, before)
  await a.answerVouch('a4', true)
  assert.deepEqual(linkOf(a, agent, 'a4'), { refused: 'denied' }, 'answered once')
})

test('taps at once count against the cap, and past eight open links an ask is told to wait', async t => {
  const { a, agent } = await scene(t, { agent: true })
  for (let i = 0; i < 10; i++) await agent.setRegister(`join_ask/c${i}`, ask())
  await sees(a, agent, 'c9')
  for (let i = 0; i < 10; i++) await a.answerVouch(`c${i}`, true)
  const answers = await until(async () => { const v = Array.from({ length: 10 }, (_, i) => linkOf(a, agent, `c${i}`)); return v.every(Boolean) && v }, 'ten answers', 30000)
  assert.equal(answers.filter(x => x.link).length, 8, JSON.stringify(answers))
  assert.equal(answers.filter(x => x.refused === 'cap').length, 2)
  // another account named each time: still no more open links (and the hub's open invites stay for the person)
  for (let i = 0; i < 4; i++) await agent.setRegister(`join_ask/d${i}`, ask({ account: i.toString(16).padStart(16, '0') }))
  await sees(a, agent, 'd3')
  for (let i = 0; i < 4; i++) await a.answerVouch(`d${i}`, true)
  const more = await until(async () => { const v = Array.from({ length: 4 }, (_, i) => linkOf(a, agent, `d${i}`)); return v.every(Boolean) && v }, 'four answers', 30000)
  assert.deepEqual(more, Array(4).fill({ refused: 'busy' }))
})

test('a session that asks again under its identity is named as it was, not after its folder (the owner\'s board, 2026-10-14)', async t => {
  const { a, agent } = await scene(t, { agent: true })
  // the agent session as it was: its device names who it is, the person named its session
  await agent.reportDevice({ device_name: 'trommi', platform: 'claude-code', folder: '~/git/trommi', host: 'desk', agent: '0123456789abcdef' })
  await until(() => a.model.sessions.get(agent.session_id), 'the session')
  await a.setRegisters({ [`session/${agent.session_id}`]: { name: 'coordinator' } })
  await agent.setRegister('join_ask/a5', ask({ name: 'saeckel', folder: '~/git/saeckel', agent: '0123456789abcdef' }))
  await sees(a, agent, 'a5')
  const named = await until(async () => {
    await a.answerVouch('a5', true)
    const answer = linkOf(a, agent, 'a5')
    return answer?.invite_id && a.model.invites.get(answer.invite_id)
  }, 'the link for the ask')
  assert.equal(named.label, 'coordinator')
  // another agent's ask in the same folder: its own name
  await agent.setRegister('join_ask/a6', ask({ name: 'saeckel', folder: '~/git/saeckel', agent: 'fedcba9876543210' }))
  await sees(a, agent, 'a6')
  await a.answerVouch('a6', true)
  const other = await until(() => linkOf(a, agent, 'a6'), 'the second link')
  assert.equal(a.model.invites.get(other.invite_id).label, 'saeckel')
})
