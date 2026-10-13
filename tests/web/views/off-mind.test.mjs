// off-mind.test.mjs: the Desk's foot (the owner's picks of 13 October 2026, "zwei Stapel wie HEY", drawn as "nur zwei
// Reiter"): two stacks docked at the bottom, his starred cards and the cards still running, each a small tab with its
// count that fans its cards upward, and the section Off your mind as a short hinted list.
// Which card lies where is the client core's (core/client/src/off_mind.rs, the patch's off_mind; its rules are tested
// there): the page only reads it, keeps the desk in view, and draws (desk.mjs offMind, deskDock, offHint).
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import { SKELETON, demo, readFixture } from './demo-room.mjs'
import { deskDock, offHint, offMind } from '../../../app/web/public/desk.mjs'

const read = f => fs.readFileSync(new URL(`../../../${f}`, import.meta.url), 'utf8')

/** A page model as desk.mjs reads it: two sessions on the desk in view, one on another desk. */
function page(off) {
  const agent = (id, here) => ({ id, device_id: `k-${id}`, name: id.toUpperCase(), hue: 100, mark: id, here })
  const everyone = [agent('a', true), agent('b', true), agent('far', false)]
  const cards = ['s1', 's2', 'r1', 'r2', 'r3', 'p1', 'c1', 'c2', 'c3', 'c4', 'x1'].map(id => ({ id, number: id, title: `Card ${id}`, agent: id === 'x1' ? 'far' : id === 'r2' ? 'b' : 'a', status: id === 'c2' ? 'shredded' : 'done' }))
  return {
    state: { off_mind: off }, everyone, byAgent: new Map(everyone.map(a => [a.id, a])), byCard: new Map(cards.map(c => [c.id, c])),
    onDesk: a => Boolean(a?.here),
  }
}
const OFF = {
  starred: [{ object_id: 's1', at: 9, running: true }, { object_id: 'x1', at: 8, running: false }, { object_id: 's2', at: 5, running: false }],
  running: [{ object_id: 'r1', at: 30, label: 'Reworking' }, { object_id: 'r2', at: 20, label: '' }, { object_id: 'r3', at: 10, label: '' }],
  put_off: ['p1'],
  closed: { 'k-a': 40, 'k-b': 2, 'k-far': 500 },
  recent: [{ object_id: 'c1', place: 'done', at: 4 }, { object_id: 'c2', place: 'trash', at: 3 }, { object_id: 'c3', place: 'done', at: 2 }, { object_id: 'c4', place: 'done', at: 1 }],
}

test('the page reads the core\'s lists in their order and keeps the desk in view', () => {
  const o = offMind(page(OFF))
  assert.deepEqual(o.starred.map(e => e.card.id), ['s1', 's2'], 'the other desk\'s starred card is not here')
  assert.deepEqual(o.running.map(e => e.card.id), ['r1', 'r2', 'r3'], 'the core\'s order, nothing sorted here')
  assert.equal(o.closed, 42, 'the closed cards of this desk\'s sessions, counted by the core')
  const src = read('app/web/public/desk.mjs')
  assert.doesNotMatch(src, /pinStarred|offPile|pileWords|PILE_TOP|trommi-oym|card\.starred\)\.sort/, 'no order or pile rule of the page\'s own')
  assert.doesNotMatch(src.slice(src.indexOf('export function offMind'), src.indexOf('// ---- the page /stacks/off')), /\.sort\(/)
})

test('the dock: two tabs, each its sign, word and count, no card at rest; the fan holds the cards, the running one leads to the complete list', () => {
  const out = String(deskDock(page(OFF), ''))
  assert.match(out, /<div class="oym-dock" id="desk-dock" data-controller="oym-dock">/)
  const [star, run] = out.split('<details class="dock-stack"').slice(1)
  // closed: the tab is the whole summary, a sign, the word, the count
  const tab = part => part.match(/<summary class="dock-tab" aria-label="([^"]*)">(.*?)<\/summary>/s)
  assert.match(star, /^ name="desk-dock" data-k="star" data-action="toggle->oym-dock#toggled">/, 'one stack open at a time (details of one name)')
  assert.equal(tab(star)[1], '2 starred: show them')
  assert.match(tab(star)[2], /^<svg[^>]*class="sketch[^>]*>.*<\/svg><b>Starred<\/b><em>2<\/em>$/s)
  assert.equal(tab(run)[1], '3 running: show them')
  assert.match(tab(run)[2], /^<svg[^>]*agent-ring.*<\/svg><b>Running<\/b><em>3<\/em>$/s)
  for (const part of [star, run]) assert.doesNotMatch(tab(part)[2], /dock-card|Card /, 'no card and no title on a tab')
  // fanned: every card of the stack, in the core's order, each a link to its card
  const fan = part => part.split('<div class="dock-fan">')[1]
  assert.deepEqual([...fan(star).matchAll(/<a class="dock-card" data-nav href="([^"]*)"/g)].map(m => m[1].split('/').pop()), ['s1', 's2'])
  assert.deepEqual([...fan(run).matchAll(/<a class="dock-card" data-nav href="([^"]*)"/g)].map(m => m[1].split('/').pop()), ['r1', 'r2', 'r3'], 'the count is what the fan shows')
  assert.match(fan(star), /Card s1<\/b><small>A · running<\/small>/, 'a starred card that runs says so, in the starred stack')
  assert.match(fan(run), /Card r1<\/b><small>A · Reworking<\/small>/)
  assert.match(fan(run), /Card r2<\/b><small>B · on it<\/small>/)
  assert.match(fan(run), /<a class="dock-all" data-nav href="\/stacks\/off">All running and done/)
  assert.doesNotMatch(fan(star), /dock-all/, 'only the running stack leads to the complete list')
  assert.doesNotMatch(run, /Card s1/, 'a starred card is not in the running stack')
  // one stack alone, and none
  assert.equal(String(deskDock(page({ ...OFF, starred: [] }), '')).split('<details').length - 1, 1)
  assert.match(String(deskDock(page({ ...OFF, starred: [], running: [] }), '')), /id="desk-dock" data-controller="oym-dock" hidden>/)
  assert.match(String(deskDock(page(null), '')), / hidden>/, 'no list from the core yet: nothing')
})

test('the section is a hinted list: three rows, put off first, and the real counts', () => {
  const out = String(offHint(page(OFF), ''))
  assert.match(out, /<section id="desk-end" class="endlist oym-hint"/)
  assert.deepEqual([...out.matchAll(/class="oym-hint-row" data-g="(\w+)"[^>]*>.*?<b>(.*?)<\/b>/g)].map(m => `${m[1]} ${m[2]}`), ['later Card p1', 'done Card c1', 'trash Card c2'])
  assert.match(out, /<span class="oym-hint-more">1 put off · 42 done<\/span>/)
  assert.match(String(offHint(page({ ...OFF, put_off: [], closed: {}, recent: [] }), '')), /<section id="desk-end" class="endlist" hidden>/)
})

test('the dock stands in the Desk\'s layer, so not on a card\'s page; a window-wide thing never tilts', () => {
  const src = read('app/web/public/desk.mjs'), css = read('app/web/public/desk.css')
  assert.match(src, /<div class="desk-layer" id="desk-layer">\$\{deskDock\(model, base\)\}/)
  assert.match(src, /stream\('replace', 'desk-dock', now\.dock\)/)
  const tilted = css.split('}').filter(r => /\brotate:/.test(r) && /dock|oym|pile/.test(r)).map(r => r.slice(0, r.lastIndexOf('{')).split('{').pop().trim())
  assert.deepEqual(tilted.filter(sel => !/^\.dock-fan > \.dock-card/.test(sel)), [], 'only the small fan cards of a fixed width tilt')
  assert.match(css, /\.dock-card \{[^}]*width: min\(300px, calc\(100vw - 60px\)\);/)
  // the look before the tabs is gone: no container, no loose sign beside a card, no top card with edges behind it
  assert.doesNotMatch(css + src, /dock-icon|dock-top|dock-label|data-look|trommi-dock/)
  assert.doesNotMatch(css.match(/\n\.oym-dock \{[^}]*\}/)[0], /padding|border|background|backdrop-filter|box-shadow/)
  assert.doesNotMatch(css, /oym-sheet|pile-pile|pile-wrap|is-slim|oym-fold|star-stack/)
  assert.doesNotMatch(read('app/web/public/sidebar.mjs') + read('app/web/public/sidebar.css'), /star-stack|starStack|ss-card/, 'the corner star stack is gone: the starred cards are the dock\'s')
})

test('the demo room makes the same lists: a starred running card leaves the running stack', async () => {
  const client = new demo.MockClient(demo.variantOf(readFixture(SKELETON), '1'), { simulate: false })
  const open = [...client.model.cards.values()].filter(c => c.object_state === 'open').slice(0, 2)
  assert.equal(open.length, 2, 'the skeleton room has two open cards')
  client.changed(c => { for (const k of open) { k.in_revision = { by: 'me', envelope_number: k.envelope_number }; c.cards.add(k.object_id) } })
  const runs = id => client.model.off_mind.running.some(r => r.object_id === id)
  assert.ok(runs(open[0].object_id) && runs(open[1].object_id), 'handed back: both run')
  await client.star(open[0].object_id, true)
  assert.ok(!runs(open[0].object_id) && runs(open[1].object_id), 'starred: out of the running stack')
  assert.deepEqual(client.model.off_mind.starred.map(s => [s.object_id, s.running]), [[open[0].object_id, true]])
})

test('the iPhone reads the same lists and draws the same dock', () => {
  const desk = read('app/ios/TrommiClient/Sources/TrommiClient/Desk.swift'), view = read('app/ios/TrommiApp/Sources/TrommiApp/MoreScreens.swift')
  assert.match(read('app/ios/TrommiClient/Sources/TrommiClient/BoardMirror.swift'), /if p\["off_mind"\]\.object != nil \{ offMind = p\["off_mind"\] \}/)
  assert.match(desk, /public func offMind\(_ v: View\) -> OffMind/)
  assert.match(view, /struct DeskDock: View/)
  assert.match(view, /private func tab\(_ key: String, _ cards: \[DeskModel\.OffMind\.Entry\], word: String\)/, 'a tab per stack: sign, word, count')
  assert.doesNotMatch(view.slice(view.indexOf('struct DeskDock'), view.indexOf('// ---- Artifacts')), /ultraThinMaterial|behind/, 'no container, no top card with edges behind it')
  assert.doesNotMatch(view + desk, /pinStarred|OffPileView|RunningStack|StarStack\b|offPile/)
})
