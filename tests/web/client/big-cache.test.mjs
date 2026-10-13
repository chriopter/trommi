// A large account opens from its cache: the model read back is one text above a megabyte (the binding refused any
// text above 1 MiB: "too-large: a text", and the page showed "Not opened"). And a cache the core refuses for any
// other reason is dropped and read anew from the hub, never a page that does not open. The REAL core, the FAKE hub.
import test from 'node:test'
import assert from 'node:assert/strict'
import { scene, until } from './helpers.mjs'
import { loadBinding } from '../stand-in/core.ts'

const long = i => `Card ${i}: ${'a long body of a real card, with its context and its reasons. '.repeat(320)}`

test('an account whose cached model is above a megabyte opens from its cache, all of it there', async t => {
  const { R, a, agent, name } = await scene(t, { agent: true })
  const ids = []
  for (let i = 0; i < 70; i++) ids.push(await agent.askCard({ card_type: 'info', title: `Big ${i}`, body: long(i) }))
  await agent.settle()
  await until(() => ids.every(id => a.model.cards.get(id)?.title), 'A holds every card')
  await a.settle()
  await a.flush()
  await a.stop()

  const again = await R.openRoom({ storage: name })   // threw too-large here
  t.after(() => again.stop().catch(() => {}))
  for (const [i, id] of ids.entries()) assert.equal(again.model.cards.get(id)?.title, `Big ${i}`, `card ${i} from the cache`)
  await again.start()
  await again.settle()
  assert.equal(again.model.cards.size >= ids.length, true)
})

test('a cache the core refuses is dropped and the model is read anew from the hub: the account opens', async t => {
  const { R, a, agent, name } = await scene(t, { agent: true })
  const card = await agent.askCard({ card_type: 'info', title: 'Still there', body: 'after the cache was dropped' })
  await agent.settle()
  await until(() => a.model.cards.get(card), 'A holds the card')
  await a.settle()
  await a.flush()
  await a.stop()

  // the core refuses the cache once, as it refused a large one
  const core = await loadBinding()   // (the same module the rooms use)
  const load = core.ClientModel.prototype.load
  let refused = 0
  core.ClientModel.prototype.load = function () { refused++; throw Object.assign(new Error('too-large: a text'), { code: 'too-large' }) }
  let again
  try { again = await R.openRoom({ storage: name }) } finally { core.ClientModel.prototype.load = load }
  t.after(() => again.stop().catch(() => {}))
  assert.equal(refused, 1)
  await again.start()
  await again.settle()
  await until(() => again.model.cards.get(card)?.title === 'Still there', 'the card, read anew from the hub')
})
