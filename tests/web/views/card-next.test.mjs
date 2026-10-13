// card-next.test.mjs: a decision made on a card's own page keeps the card open, decided, and offers the next open
// question and Close (card.mjs nextStep); in Blitz (?walk=1) the answer goes straight on to the next card.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import { ANSWERS, register } from '../../../app/web/public/card.mjs'
import { stream } from '../../../app/web/public/app.mjs'

const src = fs.readFileSync(new URL('../../../app/web/public/card.mjs', import.meta.url), 'utf8')

test('decided on its own page, not in Blitz: back to the card, with the question that followed it', () => {
  assert.match(src, /if \(ANSWERS\.includes\(what\) && !form\.has\('walk'\) && form\.has\('page'\)\) \{[\s\S]*?return redirect\(res, to\(card, fromSession \? home : BASE, said, 'decided=1', next \? `next=\$\{next\}` : ''\)\)/)
  assert.match(src, /<input type="hidden" name="page" value="1">/, 'the card page\'s form says where it was sent from')
})

test('in Blitz the answer still goes on to the next card', () => {
  const stay = src.indexOf("ANSWERS.includes(what) && !form.has('walk') && form.has('page')"), walk = src.indexOf("if (form.has('walk') || (form.has('next') && what === 'revise'))")
  assert.ok(stay > 0 && walk > stay, 'the walk keeps its own way on')
})

test('the decided card offers the next question, or says there is none, and Close', () => {
  assert.match(src, /<nav class="tc-next" aria-label="What next">/)
  assert.match(src, /<span>Next question<\/span><small>\$\{next\.title\}<\/small>/)
  assert.match(src, /That was the last open question\./)
  assert.match(src, /<a class="tc-next-close" data-nav href="\$\{home\}">Close<\/a>/)
})

test('which question comes next is the client core\'s, not the page\'s', () => {
  // (client/src/model/walk.rs next_open_after, through the worker: the Desk's order, the desk in view)
  assert.match(src, /const next = await Promise\.resolve\(hub\.nextOpenAfter\?\.\(id, deskScope\(model\(\)\)\)\)/)
  // (while it still waits; else the first that waits on this desk in the core's order)
  assert.match(src, /model\.state\.desk_order \?\? \[\]\)\.find\(x => waits\.has\(x\)\)/)
  assert.doesNotMatch(src, /const nextOpen = [^\n]*walk\[0\]/)
})

// ---- every kind of answer (the owner, 13 October 2026: "Next question wird nicht immer angezeigt") ----

globalThis.location ??= new URL('https://app.example/')
function routes() {
  const got = { live: null, posts: [] }
  const t = { BASE: '', hub: {}, model: () => ({}), stream, says: () => {}, redirect: () => {}, differs: (a, b) => String(a) !== String(b),
    get: () => {}, post: (re, fn) => got.posts.push(fn), live: (name, cfg) => { if (name === 'card') got.live = cfg }, page: () => {}, wantsStream: () => false }
  register(t)
  return got
}
const card = (id, more = {}) => ({ id, number: 1, agent: 'a1', kind: 'decision', status: 'open', title: `Card ${id}`, options: [{ key: 'x', label: 'X' }, { key: 'y', label: 'Y' }], choices: [], attachments: [], ...more })
/** The answer column the live stream draws for a page just answered (decided=1), for `answered`, with `waiting` open. */
function liveAnswer(answered, waiting = []) {
  const { live } = routes()
  const all = [answered, ...waiting]
  const m = { byCard: new Map(all.map(c => [c.id, c])), byAgent: new Map([['a1', { id: 'a1', name: 'Web', hue: 1 }]]), fresh: waiting, reads: [], state: { assets: [], messages: [], tasks: [], desk_order: all.map(c => c.id) }, agents: [], everyone: [] }
  const client = { params: new URLSearchParams(`card=${answered.id}&decided=1${waiting[0] ? `&next=${waiting[0].id}` : ''}`) }
  const taken = live.take(m, [client])
  return String([...taken.values()][0].answer)
}

const KINDS = [
  ['a decision', card('d1', { status: 'decided', choice: 'x', choices: ['x'], decided: 1 })],
  ['several options', card('m1', { multiple: true, status: 'decided', choices: ['x', 'y'], decided: 1 })],
  ['an info, Got it', card('i1', { kind: 'info', status: 'done', read: true, options: [], decided: 1 })],
  ['a permission, Allow', card('p1', { kind: 'permission', status: 'decided', choice: 'allow', choices: ['allow'], options: [{ key: 'allow', label: 'Allow' }, { key: 'deny', label: 'Deny' }], decided: 1 })],
  ['a permission, Deny', card('p2', { kind: 'permission', status: 'decided', choice: 'deny', choices: ['deny'], options: [{ key: 'allow', label: 'Allow' }, { key: 'deny', label: 'Deny' }], decided: 1 })],
  ['shredded', card('s1', { status: 'shredded', decided: 1 })],
  ['ducked', card('t1', { status: 'decided', trusted: true, recommended: 'x', decided: 1 })],
  ['a final option that closes it', card('f1', { status: 'done', choice: 'x', choices: ['x'], settled: true, options: [{ key: 'x', label: 'X', final: true }], decided: 1 })],
]
for (const [name, answered] of KINDS) {
  test(`after ${name}: the next question, through every live update`, () => {
    assert.match(liveAnswer(answered, [card('n1', { title: 'What comes next' })]), /<span>Next question<\/span><small>What comes next<\/small>/)
  })
  test(`after ${name}, the last one: "That was the last open question"`, () => {
    assert.match(liveAnswer(answered), /That was the last open question\./)
  })
}

test('every answer that settles a card on its page leads back to it with its next step (decided=1)', () => {
  for (const way of ['decide', 'trust', 'close', 'shred']) assert.ok(ANSWERS.includes(way), way)
  assert.match(src, /if \(ANSWERS\.includes\(what\) && !form\.has\('walk'\) && form\.has\('page'\)\) \{[\s\S]*?'decided=1'/)
})

test('a page not just answered draws no next step', () => {
  const { live } = routes()
  const c = card('d1', { status: 'decided', choice: 'x', choices: ['x'] })
  const m = { byCard: new Map([[c.id, c]]), byAgent: new Map([['a1', { id: 'a1', name: 'Web', hue: 1 }]]), fresh: [], reads: [], state: { assets: [], messages: [], tasks: [] } }
  assert.doesNotMatch(String([...live.take(m, [{ params: new URLSearchParams('card=d1') }]).values()][0].answer), /tc-next/)
})

test('the iPhone offers the next step after every kind of answer, not in Blitz', () => {
  const swift = fs.readFileSync(new URL('../../../app/ios/TrommiApp/Sources/TrommiApp/CardScreen.swift', import.meta.url), 'utf8')
  assert.match(swift, /if answeredHere && !walk \{ nextStep\(c\) \}/, 'no kind of card is left out')
  assert.match(swift, /\.onChange\(of: c\.status\) \{ was, now in if was == "open" && now != "open" \{ answeredHere = true \}/, 'any answer that settles the card counts')
})
