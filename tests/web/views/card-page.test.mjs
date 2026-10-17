// card-page.test.mjs: a card's page draws the app core's CardView (card.mjs cardPage) and turns what is pressed into
// the core's actions (card.mjs register: POST /cards/<id>/<what>). The demo room's view model comes from the core
// itself (core/bindings/wasm/pkg, Config.demo), focused on one card; nothing of the room is decided by the page.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import { register } from '../../../app/web/public/card.mjs'
import { html, raw } from '../../../app/web/public/ui.mjs'

const REPO = new URL('../../../', import.meta.url)
const binding = await import(new URL('core/bindings/wasm/pkg/trommi-core.js', REPO).href)
await binding.init(fs.readFileSync(new URL('core/bindings/wasm/pkg/trommi_core_wasm_bg.wasm', REPO)))
const fixture = JSON.parse(fs.readFileSync(new URL('demo/data/fixture.json', REPO), 'utf8'))
/** The demo room's view model, focused on the card with that number. */
function vmOf(number) {
  const core = new binding.AppCore(), now = Date.parse('2026-10-14T12:00:00Z')
  const send = ev => core.handle(now, { json: JSON.stringify(ev), blobs: [] })
  send({ type: 'start', config: { platform: 'web', version: 'demo', seed: 1, utc_offset_min: 0, demo: fixture } })
  send({ type: 'focus', focus: { screen: { screen: 'card', card_id: String(number) }, visible: true } })
  return JSON.parse(core.viewJson())
}
/** The card's routes on a stand-in board: GET draws a page, POST returns what it did. */
function board(vm) {
  const gets = [], posts = [], acts = []
  const stream = (a, target) => html`<turbo-stream action="${a}" target="${target}"></turbo-stream>`
  register({ stream, vm: () => vm, act: a => acts.push(a), fileUrl: () => null, attach: async () => 'local-1', toast: o => raw(`<toast>${JSON.stringify(o)}</toast>`),
    get: (p, h) => gets.push([p, h]), post: (p, h) => posts.push([p, h]),
    page: (req, res, o) => { res.page = o }, redirect: (res, to) => { res.to = to }, notFound: (req, res) => { res.missing = true }, sendStream: (req, res, b, code = 200) => { res.stream = String(b); res.code = code }, done: res => { res.done = true } })
  const run = async (list, path, form = null) => {
    const url = new URL(path, 'http://x'), res = {}
    for (const [p, h] of list) { const match = p.exec(url.pathname); if (match && (await h({ req: { headers: { referer: 'http://x/' } }, res, url, match, form })) !== false) return res }
    return null
  }
  return { acts, get: path => run(gets, path).then(r => String(r.page.main)), post: (path, fields) => { const f = new FormData(); for (const [k, v] of Object.entries(fields)) f.append(k, v); return run(posts, path, f) } }
}
const cardOf = vm => Object.values(vm.cards)[0]

test('an open decision: its options post /decide, the duck, What?? and the reverse card stand beside them', async () => {
  const vm = vmOf(43), card = cardOf(vm), main = await board(vm).get('/card/43')
  assert.equal(card.answer.mode, 'options')
  for (const o of card.options) assert.match(main, new RegExp(`formaction="/cards/${card.id}/decide" name="key" value="${o.key}"`))
  for (const way of ['trust', 'what', 'revise']) assert.match(main, new RegExp(`formaction="/cards/${card.id}/${way}"`))
  assert.match(main, /data-key="centered" data-controller="circles"/, 'the picture on the stage names its option')
  assert.doesNotMatch(main, new RegExp(`${card.place.says.en}`), 'no meta line: its place is the ⋯\'s questions before and after')
  assert.match(main, /<h1 class="tc-title"[^>]*>[^<]*<\/h1><nav class="tc-rails tc-tools"/, 'the star and the ⋯ beside the title')
  for (const m of card.menu) assert.match(main, new RegExp(`<span>${m.label.en}</span>|class="cardclip-copy"`), `${m.key} in the ⋯`)
  assert.match(main, new RegExp(`class="tc-more-item is-next" role="menuitem" data-nav href="/card/${card.place.next}"`))
  assert.doesNotMatch(main, /class="tc-rail tc-back"|tc-fromline|tc-helper|class="tc-meta"|class="final-sign"/, 'no X, no from line, no helper chip, no meta line, no tick on the tiles')
  const feed = /<section class="tc-feed"[^>]*>([\s\S]*?)<\/section>\s*<form class="tc-chat"/.exec(main)[1]
  assert.ok([...feed.matchAll(/^<(?:article|div|section|p|form)\b([^>]*)>/gm)].every(m => / id="/.test(m[1])), 'every row of the comments has an id (the morph keeps them)')
  assert.doesNotMatch(main, /undefined|\[object |NaN/)
})

test('a card without a menu, an empty one or a working one: the page still stands', async () => {
  for (const change of [c => { delete c.menu }, c => { c.menu = [] }, c => { c.menu = null }, c => { c.answer = { mode: 'working', label: { en: 'Being worked on' } } }]) {
    const vm = vmOf(43), card = cardOf(vm)
    change(card)
    const main = await board(vm).get('/card/43')
    assert.match(main, /class="tc-title"/)
    assert.match(main, /class="tc-back tc-sr"/, 'Esc still has its way back')
    assert.doesNotMatch(main, /undefined|\[object |NaN|Being worked on/)
  }
})

test('answered here (?decided=1): what was said, the way back, and the next question of CardView.after', async () => {
  const vm = vmOf(5), card = cardOf(vm), main = await board(vm).get('/card/5?decided=1')
  assert.equal(card.answer.mode, 'said')
  assert.match(main, new RegExp(`<span class="tc-opt-label">${card.answer.label.en}</span>`))
  assert.match(main, new RegExp(`formaction="/cards/${card.id}/reopen"`))
  assert.match(main, new RegExp(`class="tc-next-go" data-nav href="/card/${card.after.next.number}"`))
  assert.doesNotMatch(await board(vm).get('/card/5'), /tc-next-go/, 'only right after the answer')
})

test('handed back: In revision, and Take back posts /takeback', async () => {
  const vm = vmOf(41), card = cardOf(vm), main = await board(vm).get('/card/41')
  assert.equal(card.answer.mode, 'revising')
  assert.match(main, new RegExp(`formaction="/cards/${card.id}/takeback"`))
})

test('the forms become the core\'s actions; a Desk row gets a toast with Undo, the card\'s page goes on', async () => {
  const vm = vmOf(43), card = cardOf(vm), b = board(vm)
  const row = await b.post(`/cards/${card.id}/decide`, { key: 'split', stay: '1' })
  assert.deepEqual(b.acts.at(-1), { do: 'answer', card_id: card.id, choices: ['split'], attachments: [], option_notes: {}, marks: [] })
  assert.match(row.stream, new RegExp(`"action":"/cards/${card.id}/reopen"`))
  await b.post(`/cards/${card.id}/trust`, { stay: '1' })
  assert.deepEqual(b.acts.at(-1), { do: 'answer', card_id: card.id, choices: card.options.filter(o => o.recommended).map(o => o.key), trusted: true })
  const page = await b.post(`/cards/${card.id}/decide`, { key: 'centered', page: '1', 'note-centered': 'warmer' })
  assert.deepEqual(b.acts.at(-1).option_notes, { centered: 'warmer' })
  assert.match(page.stream, /action="visit" target="\/card\/43\?decided=1"/)
  const later = await b.post(`/cards/${card.id}/snooze`, { page: '1' })
  assert.match(later.stream, new RegExp(`action="visit" target="/card/${card.place.next}"`))
  await b.post(`/cards/${card.id}/what`, { page: '1' })
  assert.deepEqual(b.acts.at(-1), { do: 'hand_back', card_id: card.id, rework: false, marks: [] })
  await b.post(`/cards/${card.id}/more`, {})
  assert.deepEqual(b.acts.at(-1), { do: 'show_more', of: { list: 'comments', card: card.id } })
  const empty = await b.post(`/cards/${card.id}/message`, { note: ' ', stay: '1' })
  assert.equal(empty.code, 422, 'nothing to send')
})
