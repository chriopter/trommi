// group-page.test.mjs: the group page (desk.mjs deskMain) draws the app core's GroupView and nothing of its own: the
// group's name and ⋯ menu (no faces, no chips), how much is open, one block per chat whose head is the link into the
// chat, its questions and then what is only to read, the calm word when nothing is open, and Blitz over every group. The demo room's view model comes from
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

test('the head, then one block per chat, as the core made them', () => {
  const g = vmOf().group
  const main = String(deskMain(g, ''))
  assert.match(main, new RegExp(`<h2 class="group-name"[^>]*>${g.head.name.en}</h2>`))
  for (const m of g.head.menu) assert.ok(main.includes(m.label.en), `the ⋯ menu has ${m.label.en}`)
  assert.match(main, /href="\/knowledge[^"]*"/)
  assert.match(main, /data-action="group-name#rename"/)
  assert.doesNotMatch(main, /group-chip|group-faces/, 'no chips, no faces: the sidebar lists the chats')
  assert.ok(g.summary && main.includes(`<p class="group-summary">${g.summary.en}</p>`))
  assert.ok(g.blocks.length > 0)
  assert.equal(main.match(/<section class="group-block"/g)?.length, g.blocks.length)
  for (const k of g.blocks) assert.ok(main.includes(`href="/chat/${encodeURIComponent(k.slug || k.session_id)}" aria-label="${k.says.en}"`), `${k.name.en}: its head opens the chat`)
  const reads = g.blocks.reduce((n, k) => n + (k.reads ?? []).length, 0)
  assert.equal(main.match(/class="inbox-row group-read"/g)?.length ?? 0, reads)
  assert.deepEqual(g.stack, g.blocks.flatMap(k => [...(k.questions ?? []), ...(k.reads ?? [])].map(r => r.id)))
  assert.match(main, /class="desk-blitz"[\s\S]*all groups/, 'Blitz walks every group')
  assert.doesNotMatch(main, /desk-tile|desk-hello|All teams/, 'no tiles, no greeting, no home')
})

test('nothing open: the calm word, where else a question waits, what was answered lately', () => {
  const g = vmOf().group
  const calm = { ...g, blocks: [], stack: [], summary: undefined, empty: { title: { en: 'Nothing waits for you in Fernly.' }, line: { en: 'Fernly Website is at work. What they ask lands here.' }, elsewhere: { team_id: 'lab', mark: 'draw:cloud', says: { en: '1 question waits in Lab' } }, lately_says: { en: 'You answered lately' }, lately: [{ id: 'c1', session_id: 's1', slug: 'web', name: { en: 'web' }, mark: 'draw:cloud', title: 'Ship it?', choice: { en: 'Ship today' }, answered_ms: 1, age: { en: '2h' } }] } }
  const main = String(deskMain(calm, ''))
  assert.match(main, /<h3>Nothing waits for you in Fernly.<\/h3><p>Fernly Website is at work/)
  assert.match(main, /class="group-elsewhere" data-nav href="\/\?team=lab"[\s\S]*1 question waits in Lab/)
  assert.match(main, /You answered lately[\s\S]*href="\/chat\/web">web<\/a>[\s\S]*href="\/card\/c1">Ship it\?<\/a><span class="group-lately-choice">Ship today<\/span>/)
})

test('the folded rail: no counts on its rows, one quiet dot where a chat waits', () => {
  const sb = vmOf({ rail_folded: true }).sidebar
  const rows = [...(sb.stacks ?? []).flatMap(s => s.sessions ?? []), ...(sb.root ?? [])]
  assert.ok(sb.folded && rows.length)
  assert.ok(rows.every(r => !r.cards && !r.open && ['', 'waiting', 'stopped'].includes(r.signal ?? '')))
  assert.ok((sb.stacks ?? []).every(s => !s.cards))
  assert.ok(rows.some(r => r.signal === 'waiting'))
})
