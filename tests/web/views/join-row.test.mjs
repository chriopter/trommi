// join-row.test.mjs: a new session of an agent's own machine that asks to join (self-join, spec/v1.md 12.4) is a row
// of the sidebar, where sessions live (app.mjs joinsOf, sidebar.mjs joinRow): its folder's name, "wants to join ·
// <host>", Allow and Deny on the row; allowed, "joining…" until its session is in; denied or in, no row.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import { folderName, joinsOf } from '../../../app/web/public/app.mjs'
import { joinRow, sidebarRows } from '../../../app/web/public/sidebar.mjs'

const reg = value => ({ value })
const ask = { name: 'f3a9', folder: '~/git/game', host: 'desktop', account: 'acct', account_label: 'claude-max', vouched_by: 'crown' }
const room = (registers, invites = new Map()) => ({
  invites,
  sessions: new Map([['s1', { session_id: 's1', is_active: true, stale: false, parent_session_id: null, group_id: 'g1', settings: { name: 'Crown' }, registers: new Map(registers) }]]),
})

test('the folder\'s last part names the session', () => {
  assert.equal(folderName('~/git/game'), 'game')
  assert.equal(folderName('/home/x/git/game/'), 'game')
  assert.equal(folderName(''), '')
})

test('an ask nobody answered is a row with the folder, the host, Allow and Deny', () => {
  const joins = joinsOf(room([['join_ask/ab12', reg(ask)]]))
  assert.deepEqual(joins.map(j => [j.nonce, j.state, j.name, j.host]), [['ab12', 'ask', 'game', 'desktop']])
  const row = String(joinRow(joins[0]))
  assert.match(row, /class="agent-row join-row" id="join-ab12" data-state="ask"/)
  assert.match(row, /<strong>game<\/strong>/)
  assert.match(row, /wants to join · desktop/)
  assert.match(row, /title="New Claude session in ~\/git\/game on desktop, vouched for by Crown"/)
  assert.match(row, /action="\/vouch\/ab12"[^]*name="allow" value="yes"[^]*>Allow</)
  assert.match(row, /name="allow" value="no"[^]*>Deny</)
})

test('allowed: "joining…" without buttons until the session is in; denied or in: no row', () => {
  const invites = new Map([['inv1', { invite_state: 'adding' }]])
  const allowed = joinsOf(room([['join_ask/ab12', reg(ask)], ['join_link/ab12', reg({ link: 'x', invite_id: 'inv1' })]], invites))
  assert.equal(allowed[0].state, 'joining')
  const row = String(joinRow(allowed[0]))
  assert.match(row, /joining…/)
  assert.doesNotMatch(row, /Allow|Deny/)
  invites.get('inv1').invite_state = 'joined'
  assert.deepEqual(joinsOf(room([['join_ask/ab12', reg(ask)], ['join_link/ab12', reg({ link: 'x', invite_id: 'inv1' })]], invites)), [])
  assert.deepEqual(joinsOf(room([['join_ask/ab12', reg(ask)], ['join_link/ab12', reg({ refused: 'denied' })]])), [])
})

test('the rows stand in the sidebar\'s list of sessions, above New Agent', () => {
  const model = { units: [], all: false, joins: joinsOf(room([['join_ask/ab12', reg(ask)]])) }
  const html = String(sidebarRows(model, ''))
  assert.ok(html.indexOf('id="join-ab12"') >= 0 && html.indexOf('id="join-ab12"') < html.indexOf('id="sidebar-invite"'))
})
