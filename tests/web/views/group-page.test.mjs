// group-page.test.mjs: the group page (desk.mjs deskMain) draws the app core's GroupView and nothing of its own: the
// group's name, faces and ⋯ menu, a chip per chat (one chosen: its "Open chat ›"), the questions under "Waits for
// you", what is only to read below under "To read", and Blitz over every group. The demo room's view model comes from
// the core itself (core/bindings/wasm/pkg, Config.demo).
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import { deskMain } from '../../../app/web/public/desk.mjs'

const REPO = new URL('../../../', import.meta.url)
const binding = await import(new URL('core/bindings/wasm/pkg/trommi-core.js', REPO).href)
await binding.init(fs.readFileSync(new URL('core/bindings/wasm/pkg/trommi_core_wasm_bg.wasm', REPO)))
const fixture = JSON.parse(fs.readFileSync(new URL('demo/data/fixture.json', REPO), 'utf8'))
/** The demo room's view model on the group page, with this focus besides. */
function vmOf(focus = {}) {
  const core = new binding.AppCore(), now = Date.parse('2026-10-14T12:00:00Z')
  const send = ev => core.handle(now, { json: JSON.stringify(ev), blobs: [] })
  send({ type: 'start', config: { platform: 'web', version: 'demo', seed: 1, utc_offset_min: 0, demo: fixture } })
  send({ type: 'focus', focus: { screen: { screen: 'group' }, visible: true, ...focus } })
  return JSON.parse(core.viewJson())
}

test('the head, the chips and the two parts, as the core made them', () => {
  const g = vmOf().group
  const main = String(deskMain(g, ''))
  assert.match(main, new RegExp(`<h2 class="group-name"[^>]*>${g.head.name.en}</h2>`))
  for (const m of g.head.menu) assert.ok(main.includes(m.label.en), `the ⋯ menu has ${m.label.en}`)
  assert.match(main, /href="\/knowledge[^"]*"/)
  assert.match(main, /data-action="group-name#rename"/)
  assert.equal(main.match(/class="group-chip(?: [^"]*)?"/g)?.length, g.chips.length + (g.more ? 1 : 0))
  assert.match(main, /class="group-chip is-all is-on"/, 'All is chosen')
  assert.match(main, /Waits for you<span>\d+<\/span>/)
  assert.equal(main.match(/class="inbox-row group-read"/g)?.length ?? 0, g.reads.length)
  assert.ok(g.questions.length > 0 && main.includes('id="sec-questions"'))
  assert.match(main, /class="desk-blitz"[\s\S]*all groups/, 'Blitz walks every group')
  assert.doesNotMatch(main, /desk-tile|desk-hello|All teams/, 'no tiles, no greeting, no home')
})

test('a chip chosen: only its chat, with its way into the chat', () => {
  const all = vmOf().group
  const chat = all.chips.find(c => c.session_id)
  const g = vmOf({ group_chat: chat.slug }).group
  assert.ok(g.stack.length > 0 && g.stack.length < all.stack.length, 'fewer cards')
  assert.ok(g.questions.every(r => r.from.session_id === chat.session_id))
  const main = String(deskMain(g, ''))
  assert.match(main, /<span class="group-chip is-on"/)
  assert.match(main, new RegExp(`class="group-chip-go" data-nav href="/chat/${chat.slug}">Open chat ›</a>`))
  assert.match(main, /class="group-chip is-all" data-nav href="\/"/, 'All clears the filter')
})

test('the folded rail: no counts on its rows, one quiet dot where a chat waits', () => {
  const sb = vmOf({ rail_folded: true }).sidebar
  const rows = [...(sb.stacks ?? []).flatMap(s => s.sessions ?? []), ...(sb.root ?? [])]
  assert.ok(sb.folded && rows.length)
  assert.ok(rows.every(r => !r.cards && !r.open && ['', 'waiting', 'stopped'].includes(r.signal ?? '')))
  assert.ok((sb.stacks ?? []).every(s => !s.cards))
  assert.ok(rows.some(r => r.signal === 'waiting'))
})
