// A Chat's window on this device: its first window is the last 24 hours (or 50 items, if that is more), and this
// browser keeps only the newest items of each Chat in its cache; older ones are read again from the hub when the
// person scrolls up (history is not forward secret, spec 1; the hub keeps a session's Chat). STAND-IN core, FAKE hub.
import test from 'node:test'
import assert from 'node:assert/strict'
import { KEEP_PER_CHAT } from '../../../app/web/core/client.ts'
import { scene } from './helpers.mjs'

test('a long Chat: the cache keeps its newest items only; the window opens on the last day and reads further back from the hub', async t => {
  const { a, agent } = await scene(t, { agent: true })
  const total = KEEP_PER_CHAT + 160
  for (let i = 0; i < total; i++) await agent.say({ text: `said ${i}` })
  await agent.client.settle({ timeout_ms: 60_000 }); await a.settle({ timeout_ms: 60_000 })
  const key = `chat:session/${agent.session_id}`
  await a.flush()
  for (let i = 0; i < 50 && (await a['cache'].range(`tl/${key}/`)).length > KEEP_PER_CHAT + 100; i++) await new Promise(r => setTimeout(r, 20))
  const kept = await a['cache'].range(`tl/${key}/`)
  assert.ok(kept.length >= KEEP_PER_CHAT && kept.length < total, `the cache keeps the newest (${kept.length} of ${total})`)
  assert.equal(kept.at(-1)[1].content?.text, `said ${total - 1}`, 'the newest is among them')
  // the first window: a day back, here everything was said within it, so as many as the cap allows
  const first = await a.loadTimeline(key, { limit: 50, since_ms: 86_400_000 })
  assert.equal(first.loaded, KEEP_PER_CHAT)
  assert.equal(first.has_more, true)
  // scrolling up reads what the cache no longer holds from the hub, to the first item
  for (let more = true; more;) more = (await a.loadTimeline(key, { limit: 50 })).has_more
  const items = [...a.model.timelines.get(key).items.values()].map(i => i.content?.text).filter(Boolean)
  assert.ok(items.includes('said 0') && items.includes(`said ${total - 1}`), 'from the first to the newest')
})

test('a Chat whose last day is short opens on 50 items', async t => {
  const { a, agent } = await scene(t, { agent: true })
  for (let i = 0; i < 120; i++) await agent.say({ text: `said ${i}` })
  await agent.client.settle({ timeout_ms: 60_000 }); await a.settle({ timeout_ms: 60_000 })
  const key = `chat:session/${agent.session_id}`
  const first = await a.loadTimeline(key, { limit: 50, since_ms: 1 })
  assert.equal(first.loaded, 50)
  assert.equal(first.has_more, true)
})
