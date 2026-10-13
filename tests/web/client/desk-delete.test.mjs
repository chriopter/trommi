// Deleting a desk (client/src/desk_delete.rs, client.ts deskDeleteStep): the core plans each step, the client does
// it; the board's steps are the app's (here: an empty board). A delete broken off on one device is finished by
// another. STAND-IN core, FAKE hub.
import test from 'node:test'
import assert from 'node:assert/strict'
import { scene, addHuman, until } from './helpers.mjs'

const LAB = '1a'.repeat(16)
const EMPTY = { shapes: [], files: [], notes: 0, snapshot_empty: true }

/** Runs the steps on `c` until the desk is gone (or `stop` says so); the board steps answer an empty board. */
async function run(c, id, { stop = () => false } = {}) {
  const known = { files_gone: [], board: null }, seen = []
  for (let i = 0; i < 40; i++) {
    const p = await c.deskDeleteStep(id, known)
    seen.push(p.step)
    if (p.files_gone) known.files_gone.push(...p.files_gone)
    if (p.step === 'done' || (p.step === 'refused' && p.code === 'no-desk' && seen.length > 1)) return seen
    if (p.step === 'refused') throw new Error(p.code)
    if (['load_board', 'erase_board', 'write_snapshot'].includes(p.step)) known.board = EMPTY
    await c.settle()
    if (stop(p)) return seen
  }
  throw new Error(`no end: ${seen.join(' ')}`)
}

test('a desk goes with its cards, sessions and agent; the other desk keeps everything', async t => {
  const { a, agent } = await scene(t, { agent: true })
  await until(() => a.model.human.desks.get('main'), 'the first desk')
  await a.setDesk(LAB, { name: 'Lab', created_at: 1, goals: 'Ship it' })
  const sid = agent.session_id
  await until(() => a.model.sessions.get(sid)?.group_id, 'a knows the session')
  await a.setRegisters({ [`session/${sid}`]: { desk: LAB } })
  const card = await agent.askCard({ card_type: 'decision', title: 'Which one?', options: [{ key: 'x', label: 'X' }] })
  await a.settle()
  await until(() => a.model.cards.get(card)?.object_state === 'open', 'the card on a')
  const c = a.deskContents(LAB)
  assert.equal(c.sessions, 1)
  assert.equal(c.open_cards, 1)
  assert.equal(c.agents, 1)
  assert.equal(c.goals, 1)
  assert.equal(c.board, null, 'the board is not read yet')
  assert.equal(a.deskContents('main').sessions, 0)

  const steps = await run(a, LAB, { stop: p => { if (p.step === 'remove_devices') assert.notEqual(a.model.cards.get(card)?.object_state, 'open', 'the card was shredded'); return false } })
  assert.deepEqual(steps.slice(0, 3), ['shred', 'remove_devices', 'archive'], 'mark, then shred, remove the agent, archive')
  assert.equal(steps.at(-1), 'done')
  assert.equal(a.model.human.session_settings.get(sid)?.archived, true, 'the session is archived')
  assert.equal(a.model.members.get(agent.device_id)?.is_active, false, 'the agent left the room')
  assert.ok(!a.model.human.desks.get(LAB), 'the desk is gone')
  assert.ok(a.model.human.desks.get('main'), 'the other desk stays')
  assert.deepEqual(a.desksDeleting(), [])
})

test('a delete broken off after its first steps is finished by another device', async t => {
  const { a, b } = await scene(t, { second: true })
  await a.setDesk(LAB, { name: 'Lab', created_at: 1 })
  await a.settle(); await b.settle()
  // a marks the desk and stops
  await run(a, LAB, { stop: p => p.step !== 'mark' })
  await a.settle(); await b.settle()
  await until(() => b.desksDeleting().includes(LAB), 'b knows the desk is going')
  await run(b, LAB)
  await b.settle(); await a.settle()
  await until(() => !a.model.human.desks.get(LAB), 'the desk gone on a too')
  assert.deepEqual(b.desksDeleting(), [])
})

test('the last desk is refused', async t => {
  const { a } = await scene(t)
  await until(() => a.model.human.desks.get('main'), 'the first desk')
  const p = await a.deskDeleteStep('main', {})
  assert.equal(p.step, 'refused')
  assert.equal(p.code, 'last-desk')
  assert.ok(a.model.human.desks.get('main'))
})
