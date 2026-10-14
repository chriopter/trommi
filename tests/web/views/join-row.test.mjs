// join-row.test.mjs: a new session of an agent's own machine that asks to join (self-join, spec/v1.md 12.4) is a row
// of the sidebar, where sessions live (vm.account.asks, sidebar.mjs joinRow): its name, host and who vouched, with
// Allow and Deny, above New Agent; once allowed it says "joining…" until its session's own row stands.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import { joinRow, sidebarRows } from '../../../app/web/public/sidebar.mjs'

const ask = { nonce: 'ab12', name: 'game', folder: '~/game', host: 'desktop', kind: 'Claude', vouched_by: 'trommi', joining: false, says: { key: 'join.ask', en: 'New Claude session in ~/game on desktop, vouched for by trommi' } }

test('an ask is a row with what it says, Allow and Deny', () => {
  const row = String(joinRow(ask))
  assert.match(row, /class="agent-row join-row" id="join-ab12" data-state="ask"/)
  assert.match(row, /<strong>game<\/strong><small class="join-line">wants to join · desktop<\/small><small class="join-by">vouched by trommi<\/small>/)
  assert.match(row, /title="New Claude session in ~\/game on desktop, vouched for by trommi"/)
  assert.match(row, /action="\/vouch\/ab12"[^]*name="allow" value="yes"[^]*>Allow</)
  assert.match(row, /name="allow" value="no"[^]*>Deny</)
})

test('an allowed ask says it is joining and asks nothing more', () => {
  const row = String(joinRow({ ...ask, joining: true }))
  assert.match(row, /data-state="joining"[^]*joining…/)
  assert.doesNotMatch(row, />Allow</)
})

test('the rows stand in the sidebar\'s list of sessions, above New Agent', () => {
  const vm = { sidebar: { sessions: [] }, account: { step: 'none', asks: [ask] } }
  const html = String(sidebarRows(vm, ''))
  assert.ok(html.indexOf('id="join-ab12"') >= 0 && html.indexOf('id="join-ab12"') < html.indexOf('id="sidebar-invite"'))
})
