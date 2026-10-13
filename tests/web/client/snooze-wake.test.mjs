// His "Later" (snooze/<id>) holds a card off the Desk until its time and only for the version he put off: a new
// version from its agent (revised, its urgency raised) brings it back, in the core's stack and on the web's Desk
// (app.mjs BoardState: the queue the Desk shows, from the client core's stack). A snooze whose time passed holds nothing
// either. A session whose only critical card is put off is not shown as waiting on him (the red hand: the core's
// core/client/src/status.rs, which reads the Desk). The client projects again when the next snooze ends (the core's wake_at).
// STAND-IN core, FAKE hub.
import test from 'node:test'
import assert from 'node:assert/strict'
import { scene, until } from './helpers.mjs'
import { BoardState, blockedOf } from '../../../app/web/public/app.mjs'

const deskOf = client => new BoardState(client).update()

test('a card put off comes back on the Desk with its next version, and when its time is past', async t => {
  const { a, agent } = await scene(t, { agent: true })
  const card = await agent.askCard({ title: 'Shared core: how fast?', options: [{ key: 'a', label: 'A' }, { key: 'b', label: 'B' }] })
  const info = await agent.askCard({ card_type: 'info', title: 'Mac in TestFlight' })
  await agent.settle()
  await until(() => a.model.stack.includes(card) && a.model.stack.includes(info), 'both on the Desk')

  // put off until tomorrow: off the Desk, on Later
  const tomorrow = Date.now() + 24 * 3600_000
  await a.snooze(card, tomorrow); await a.snooze(info, tomorrow)
  await a.settle()
  await until(() => !a.model.stack.includes(card) && !a.model.stack.includes(info), 'both off the Desk')
  let s = deskOf(a)
  assert.ok(!s.queue.includes(card) && !s.queue.includes(info), 'the web Desk holds them off too')
  assert.equal(s.byId.get(card).snoozed_until, tomorrow)

  // the agent revises the card and raises its urgency: it is back, on both
  await agent.settle()
  await agent.reviseCard(card, { title: 'Shared core: how fast should we roll it out?' })
  await agent.settle()
  await agent.next(card, { urgency: 'critical' }, false, 'critical')
  await agent.settle(); await a.settle()
  await until(() => a.model.cards.get(card)?.title === 'Shared core: how fast should we roll it out?' && a.model.stack.includes(card), 'the revised card is back in the stack')
  s = deskOf(a)
  assert.ok(s.queue.includes(card), 'the revised card is back on the web Desk')
  assert.equal(s.byId.get(card).snoozed_until, undefined)
  assert.ok(s.byId.get(card).unsnoozed, 'it says it is back from Later')
  assert.ok(!s.queue.includes(info), 'the info nobody touched stays put off')

  // the info's time passes: back without any new word
  // (long enough to be seen held on a busy machine: with 40 ms the time had passed before the first look)
  const wake = Date.now() + 1500
  await a.snooze(info, wake)
  await a.settle()
  await until(() => !deskOf(a).queue.includes(info), 'held while its time runs')
  await new Promise(r => setTimeout(r, Math.max(0, wake - Date.now()) + 250))
  const board = new BoardState(a)
  assert.ok(board.update().queue.includes(info), 'a snooze past its time holds nothing on the web Desk')
})

test('a critical card put off does not raise the red hand; the next morning is when the page wakes it', async t => {
  const { a, agent } = await scene(t, { agent: true })
  const card = await agent.askCard({ title: 'Blocked on this', options: [{ key: 'a', label: 'A' }, { key: 'b', label: 'B' }] }, { urgency: 'critical' })
  await agent.settle()
  await until(() => a.model.stack.includes(card), 'on the Desk')
  const until1 = Date.now() + 3600_000
  const rowOf = () => new BoardState(a).update().agents.find(x => x.id === deskOf(a).byId.get(card).agent)
  // (the level itself rides in the card's body, which the stand-in's codec leaves out: the red hand for a blocking
  //  question on the Desk is tested on the core's model, tests/web/model/stack-echoes.test.mjs)
  await a.snooze(card, until1)
  await a.settle()
  await until(() => !a.model.stack.includes(card), 'put off')
  await until(() => rowOf()?.blocked === null, 'no red hand for what is put off')
  assert.equal(blockedOf(rowOf(), deskOf(a)), null)
  assert.equal(a.wakeTime, until1, 'the client projects again when its time comes')
})
