// fuzz.test.mjs: the web model (the client core's, factory.mjs) under random stories of hand-made core results
// (factory.mjs): cards and their versions, answers, take backs, Chat lines, registers, requests, notes, clock jumps,
// odd payloads and refused envelopes, in any order. README "Fuzzing".
// Properties, after every step: nothing throws; the page's copy (mirror.ts: a snapshot, then one patch per change)
// is the model; at the end the cache's records (cacheRecords -> JSON -> modelFromCache) give the model back.
// FUZZ_CASES (default 60) and FUZZ_SEED set the campaign; a failure names its seed.
//   node --test tests/web/model/fuzz.test.mjs
import test from 'node:test'
import assert from 'node:assert/strict'
import { applyPatch, mirrorOf, patchOf, snapshotOf } from '../../../app/web/core/mirror.ts'
import { cacheRecords, emptyChange, modelFromCache } from '../../../app/web/core/model.ts'
import { World, canon, hex, id16, project, withDesk } from './factory.mjs'

/** mulberry32: a small seeded source. */
function rng(seed) {
  let a = seed >>> 0
  const next = () => { a = (a + 0x6D2B79F5) >>> 0; let t = a; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) / 4294967296 }
  return { next, below: n => Math.floor(next() * n), pick: list => list[Math.floor(next() * list.length)], oneIn: n => Math.floor(next() * n) === 0 }
}

const firstDiff = (a, b) => { const x = canon(a), y = canon(b); if (x === y) return null; let i = 0; while (x[i] === y[i]) i++; return `${x.slice(Math.max(0, i - 140), i + 100)}\n   vs\n${y.slice(Math.max(0, i - 140), i + 100)}` }

const ODD = ['', ' ', 'ä€😀', '‮', '<script>', 'x'.repeat(3000), null, 0, -1, 1e300, [], {}, true]
const OPTIONS = [{ key: 'a', label: 'A' }, { key: 'b', label: 'B', final: true }]

function oneStory(seed) {
  const r = rng(seed)
  const w = new World()
  const mirror = mirrorOf(structuredClone(snapshotOf(w.model)))
  const store = new Map()
  const steps = []
  w.onChange = c => {
    applyPatch(mirror, structuredClone(patchOf(w.model, c)))
    for (const [key, value] of cacheRecords(w.model, c)) { if (value === undefined) store.delete(key); else store.set(key, JSON.parse(JSON.stringify(value))) }
  }
  w.groups()
  const cards = [1, 2, 3, 4].map(k => id16(0xc0, k))
  const answers = new Map()
  const odd = () => r.pick(ODD)
  for (let i = 0; i < 30; i++) {
    const card = r.pick(cards)
    const known = w.objects.get(card)
    const op = r.below(12)
    steps.push(`${op}:${hex(card).slice(0, 6)}`)
    switch (op) {
      case 0: case 1: {
        const fields = { card_type: r.pick(['decision', 'info', 'poll', odd()]), title: r.oneIn(4) ? odd() : `T${i}`, options: r.oneIn(5) ? odd() : OPTIONS, object_version: known ? 2 + i : 1 }
        w.card(card, fields, { urgency: r.pick(['low', 'normal', 'high', 'critical']), previous: known?.current ?? null, state: r.oneIn(6) ? 'closed' : 'open' })
        break
      }
      case 2: case 3:
        if (known) {
          // (odd choices in the body; the core's binding holds strings only, as it reads them from a decoded body)
          const choices = r.oneIn(4) ? [odd()] : [r.pick(['a', 'b'])]
          answers.set(card, w.answer(card, { answer_action: r.pick(['answer', 'read', odd()]), choices, note: r.oneIn(3) ? odd() : 'ok' }, { state: r.pick(['answered', 'closed']), bind_choices: choices.map(String) }))
        }
        break
      case 4:
        if (known && answers.has(card)) w.takeBack(card, answers.get(card).hash)
        break
      case 5: w.message({ text: r.oneIn(3) ? odd() : `line ${i}`, terminal: r.pick([undefined, 'answer', odd()]) }, { card: r.oneIn(2) ? card : null, sender: r.pick([w.agent, w.me]) }); break
      case 6: w.register(r.pick([`draft/${card}`, `snooze/${card}`, `duck/${card}`, 'crown', `session/${w.session}`, `star/${card}`, 'kit']), r.oneIn(4) ? null : r.pick([{ until: w.clock + r.below(7_200_000) }, { note: odd() }, { name: odd() }, { session_id: w.session }, odd()]), { lamport: 1 + i }); break
      case 7: w.register(r.pick(['profile', 'status_line/t', 'heard']), r.pick([{ model: 'opus', agent_name: odd() }, { label: 'T', state: r.pick(['working', 'done', odd()]) }, null]), { sender: w.agent, lamport: 1 + i }); break
      case 8: {
        const req = id16(0xa0, i)
        w.take({ kind: 'request', object: w.object(req, 'request', 'open', { urgency: 'critical' }), payload: { schema_version: 2, tool_name: r.oneIn(3) ? odd() : 'Bash', description: 'd' }, bind: { kind: 'request', request_id: req, expires_at: w.clock + r.below(120_000) } })
        break
      }
      case 9: w.take({ kind: 'version', object: w.object(card, 'card'), outcome: r.pick(['refused', 'chained', 'void']), code: r.pick(['bad-signature', 'pruned', 'no-key', 'newer-version', 'gap']), object_after: null }); break
      case 10: w.clock += r.below(3_600_000 * 3); w.run(() => {}); break
      default: w.card(card, { title: 'newer', object_version: 1, schema_version: r.pick([3, 2, 'x']) }); break
    }
    const d = firstDiff(mirror, { ...w.model, _builder: undefined })
    assert.equal(d, null, `seed ${seed}, after ${steps.join(' ')}: the page's copy is not the model:\n${d}`)
  }
  const warm = modelFromCache([...store].filter(([key]) => !key.startsWith('tl/')))
  withDesk(warm)
  project(warm, emptyChange(), w.clock)
  const expected = structuredClone({ ...w.model, _builder: undefined })
  for (const t of expected.timelines.values()) { t.items = new Map(); t.loaded_down_to = Infinity; t.window_open = false }
  // (the session list and what has left the Desk are no records the cache keeps: each run's core makes them anew, by
  //  that run's clock and presence)
  const d = firstDiff({ ...warm, session_list: undefined, off_mind: undefined }, { ...expected, session_list: undefined, off_mind: undefined })
  assert.equal(d, null, `seed ${seed}, after ${steps.join(' ')}: the model from the cache differs:\n${d}`)
}

test('the web model under random stories: its copy and its cache stay the model', () => {
  const cases = Number(process.env.FUZZ_CASES ?? 60)
  const first = Number(process.env.FUZZ_SEED ?? Math.floor(Math.random() * 2 ** 31))
  for (let k = 0; k < cases; k++) oneStory(first + k)
})
