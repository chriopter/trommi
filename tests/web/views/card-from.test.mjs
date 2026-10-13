// card-from.test.mjs: who a card comes from stands above its title, on its page and in Blitz (the owner, 13 October
// 2026): the session's drawing and its path, desk › main session › itself. The path is the client core's
// (client/src/sessions.rs `path`, tested there); the page only draws it. The demo room makes the same path.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import { demo } from './demo-room.mjs'

const read = p => fs.readFileSync(new URL(`../../../${p}`, import.meta.url), 'utf8')

test('the line above the title draws the core\'s path, the name in bold, a link to the session', () => {
  const src = read('app/web/public/card.mjs')
  assert.match(src, /\$\{from \? fromLine\(from, base\) : ''\}<h1 class="tc-title"/)
  assert.match(src, /const path = a\.path\?\.length \? a\.path : \[a\.name\], name = path\.at\(-1\)/)
  assert.match(src, /\$\{path\.slice\(0, -1\)\.map\(p => html`<span>\$\{p\}<\/span><i aria-hidden="true">›<\/i>`\)\}<b>\$\{name\}<\/b><\/a>/)
  assert.doesNotMatch(src, /trommi-from|FROM_LOOKS|fromHead/, 'no switch between proposals any more')
  assert.doesNotMatch(src, /<p class="tc-meta">\$\{from/, 'the sender is not said twice under the title')
  assert.match(read('app/web/public/app.mjs'), /desk: r\.desk, path: r\.path \?\? \[\],/, 'the path comes with the core\'s session row')
})

test('the demo room makes the same path: desk (two or more), main session, itself', () => {
  const m = { members: new Map(), sessions: new Map(), cards: new Map(), permissions: new Map(), timelines: new Map(), human: { desks: { main: { name: 'Work' }, lab: { name: 'Lab', created_at: 5 } }, session_settings: new Map(), crown: null } }
  const s = (id, more = {}) => ({ session_id: id, agent_device_id: `d-${id}`, is_active: true, is_online: true, profile: { is_main: !more.parent, ...(more.parent ? { parent_session: more.parent } : {}) }, card_ids: [], open_card_ids: [] })
  for (const x of [s('core'), s('kern', { parent: 'core' })]) m.sessions.set(x.session_id, x)
  m.human.session_settings.set('core', { name: 'Core', desk: 'lab' }); m.human.session_settings.set('kern', { name: 'App-Kern' })
  const rows = demo.demoSessions(m)
  const by = n => rows.find(r => r.name === n)
  assert.deepEqual(by('Core')?.path, ['Lab', 'Core'])
  assert.deepEqual(by('App-Kern')?.path, ['Lab', 'Core', 'App-Kern'])
})

test('the iPhone draws the same line from the core\'s path, in Blitz too', () => {
  const swift = read('app/ios/TrommiApp/Sources/TrommiApp/CardScreen.swift')
  assert.match(swift, /if let a = a \{ fromLine\(a\) \}\n\s*Text\(c\.title\)/)
  assert.match(read('app/ios/TrommiClient/Sources/TrommiClient/Desk.swift'), /a\.path = r\["path"\]\.array\?\.compactMap\(\\\.string\) \?\? \[\]/)
  assert.match(read('app/ios/TrommiApp/Sources/TrommiApp/MoreScreens.swift'), /CardScreen\(cardId: id, walk: true\)/, 'Blitz shows the card page itself')
})
