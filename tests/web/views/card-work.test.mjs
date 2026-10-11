// card-work.test.mjs: a card its session works on now (card.mjs workOf, workStrip; card.css .tc-work): a strip above
// the title with the session's drawing, "<session> is working on it", its status line and its age, and a pen line.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'

const read = f => fs.readFileSync(new URL(`../../../app/web/public/${f}`, import.meta.url), 'utf8')
const { workOf } = await import('../../../app/web/public/card.mjs')

const NOW = Date.now()
const agent = (over = {}) => ({ id: 'a1', name: 'Design', online: true, ...over })
const modelOf = ({ card, a = agent(), tasks = [] }) => ({ byAgent: new Map([[a.id, a]]), state: { tasks } })
const open = (over = {}) => ({ id: 'c1', agent: 'a1', kind: 'decision', status: 'open', with_agent: NOW - 60_000, ...over })
const line = (over = {}) => ({ agent: 'a1', id: 't', label: 'Card', state: 'working', detail: 'Reworking', card_id: null, updated: NOW - 5_000, ...over })

test('a card handed back is worked on while its session is connected; the line naming it wins', () => {
  const card = open()
  const named = line({ id: 'n', card_id: 'c1', updated: NOW - 9_000 }), other = line({ id: 'o', updated: NOW - 1_000 })
  const w = workOf(card, modelOf({ card, tasks: [other, named] }))
  assert.equal(w.agent.name, 'Design')
  assert.equal(w.line, named)
  assert.equal(w.at, named.updated)
  assert.ok(workOf(card, modelOf({ card })), 'without a status line too')
  assert.equal(workOf(card, modelOf({ card, a: agent({ online: false }) })), null, 'a session that is gone: nothing')
})

test('an answered card only while its session has a working status line', () => {
  const card = open({ status: 'decided', with_agent: null, decided: NOW - 60_000, choice: 'a', options: [{ key: 'a', label: 'A' }] })
  assert.equal(workOf(card, modelOf({ card })), null)
  assert.ok(workOf(card, modelOf({ card, tasks: [line()] })))
  assert.equal(workOf(card, modelOf({ card, tasks: [line({ state: 'done' })] })), null, 'a done line is no work')
})

test('an open card on the Desk and a permission are never "being worked on"', () => {
  assert.equal(workOf(open({ with_agent: null }), modelOf({ card: null, tasks: [line()] })), null)
  assert.equal(workOf(open({ kind: 'permission' }), modelOf({ card: null, tasks: [line()] })), null)
})

test('the strip stands above the title (live with the card\'s lead), its pen stroke runs, and rests without motion', () => {
  const src = read('card.mjs'), css = read('card.css')
  assert.match(src, /\$\{old \? '' : workStrip\(card, model\)\}<h1 class="tc-title"/)
  assert.match(src, /<div class="tc-work" role="status">\$\{avatar\(w\.agent, \{ crown: false, working: true \}\)\}[\s\S]*is working on it/)
  assert.match(css, /\.tc-work-run \.run-pen \{[^}]*animation: tc-work-run/)
  assert.match(css, /prefers-reduced-motion: reduce\) \{ \.tc-work-run \.run-pen \{ animation: none; display: none; \}/)
})
