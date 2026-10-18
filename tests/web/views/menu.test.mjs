// menu.test.mjs: the Trommi menu (sidebar.mjs menuDoors, the wide screen's and the phone's one component) carries
// Log out: a menu entry under Demo that goes to /logout, the page that asks first (the same one Settings · Account's
// Log Out opens), with its own pen-drawn icon.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import { topbar } from '../../../app/web/public/sidebar.mjs'
import { readFileSync } from 'node:fs'

const vm = { sidebar: { teams: [{ id: 'main', name: { en: 'Personal' }, waiting: 0, selected: true, knocks: false }], sessions: [], badge: 0, team: 'main', place: { en: 'Personal' }, waiting: 0, knocking: 0, stopped: 0, working: 0, unheard: 0 }, settings: { logout: { title: { en: 'Log out' } } }, notes: {} }
const menu = () => String(topbar(vm, '', false)).match(/<nav class="sidedoors" id="brand-doors"[\s\S]*?<\/nav>/)[0]

test('the menu has Log out, a menu entry to the /logout page', () => {
  const row = menu().match(/<a [^>]*id="menu-logout"[^>]*>[\s\S]*?<\/a>/)?.[0]
  assert.ok(row, 'a Log out row in the menu')
  assert.match(row, /role="menuitem"/, 'reached by the menu\'s arrows and closing it like Settings')
  assert.match(row, /href="\/logout"/, 'the same page as Settings · Account\'s Log Out (it asks first)')
  assert.match(row, /data-nav/, 'by the app\'s own navigation')
  assert.match(row, /<svg [^>]*class="sketch"[\s\S]*<path /, 'a drawn icon')
  assert.match(row, /<span>Log out<\/span>/)
})

test('Log out stands under Demo and above the row of small buttons', () => {
  const m = menu(), at = id => m.indexOf(`id="${id}"`)
  assert.ok(at('menu-settings') < at('demo-toggle') && at('demo-toggle') < at('menu-logout') && at('menu-logout') < at('push-toggle'), 'Settings, Demo, Log out, then the small buttons')
})

test('the phone puts Log out with Settings and Demo', async () => {
  const css = (await import('node:fs')).readFileSync(new URL('../../../app/web/public/sidebar.css', import.meta.url), 'utf8')
  assert.match(css, /:is\(\.menu-demo-row, \.menu-logout-row\) \{ order: 3;/, 'the same place in the phone\'s column as Demo')
})

// The number keys 1…9 switch teams by clicking the team's entry in the closed menu (ui.mjs, team.switch): that must
// not leave the "open the menu again after the switch" mark, or the menu comes up and blocks the next key.
test('a team switched by its number key leaves the menu shut; one picked in the open menu keeps it open', async () => {
  const { MenuController } = await import('../../../app/web/public/sidebar.mjs')
  const kept = []
  globalThis.sessionStorage = { setItem: (k, v) => kept.push([k, v]), getItem: () => null, removeItem() {} }
  const team = { closest: sel => (sel === '.menu-team[data-team]' ? team : null) }
  const pick = hidden => MenuController.prototype.chosen.call({ element: { hidden }, close() {} }, { target: team })
  try {
    pick(true)
    assert.deepEqual(kept, [], 'the closed menu stays closed after a number key')
    pick(false)
    assert.deepEqual(kept, [['trommi-menu-keep', '1']], 'a team picked in the open menu still opens it again')
  } finally { delete globalThis.sessionStorage }
})

test('the menu stays as it was: what runs here is on the What\'s new page, not under its row', () => {
  assert.doesNotMatch(menu(), /menu-build/)
  const news = ((readFileSync(new URL('../../../app/web/public/news.mjs', import.meta.url), 'utf8')))
  assert.match(news, /<p class="news-build" id="news-build">\$\{runningLine\(\)\}<\/p>/, 'a small line at the top of What\'s new')
})

test('the release script writes what runs there beside the build, outside the build\'s manifest', async () => {
  const src = readFileSync(new URL('../../../.github/scripts/web_release.mjs', import.meta.url), 'utf8')
  assert.match(src, /'public', 'gen', 'release\.json'\)[\s\S]*version, commit, released/, 'gen/release.json: number, commit, time')
  const app = readFileSync(new URL('../../../app/web/public/app.mjs', import.meta.url), 'utf8')
  assert.match(app, /fetch\('\/gen\/release\.json'[\s\S]*fetch\('\/gen\/build\.txt'/, 'the app asks for it, else for the build\'s commit')
})

// The number keys: 1…9 the teams in the menu's order (the owner, 13 October 2026; there is no view of every team).
test('1…9 switch to the teams in order', async () => {
  const { LAYOUT, teamByNumber } = await import('../../../app/web/public/ui.mjs')
  const entry = LAYOUT.find(g => g.scope === 'app').keys.find(k => k.id === 'team.switch')
  assert.deepEqual(entry.keys, ['1…9'])
  const row = team => ({ dataset: { team } })
  const menu = [row('main'), row('platform'), row('lab')]
  assert.equal(teamByNumber(menu, 1)?.dataset.team, 'main')
  assert.equal(teamByNumber(menu, 3)?.dataset.team, 'lab')
  assert.equal(teamByNumber(menu, 4), null, 'a number past the last team does nothing')
  assert.equal(teamByNumber(menu, 0), null, '0 is no team')
})

test('the team switch lists only the teams, each with what waits, and no row for every team at once', () => {
  const two = { ...vm, sidebar: { ...vm.sidebar, teams: [{ id: 'main', name: { en: 'Fernly' }, waiting: 3, selected: true, knocks: true }, { id: 'lab', name: { en: 'Lab' }, waiting: 0, selected: false, knocks: false }], place: { en: 'Fernly' } } }
  const rows = String(topbar(two, '', false))
  assert.deepEqual([...rows.matchAll(/data-team="([^"]+)" aria-checked/g)].map(m => m[1]), ['main', 'lab'])
  assert.match(rows, /href="\/\?team=lab"/, 'a team is picked by its address')
  assert.match(rows, /<b>Fernly<\/b><i class="menu-n">3<\/i>/, 'its waiting count')
  assert.doesNotMatch(rows, /is-all|All Desks|desk=all/i)
})
