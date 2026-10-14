// join-row.test.mjs: a new session of an agent's own machine that asks to join (self-join, spec/v1.md 12.4) is a row
// of the sidebar, where sessions live (vm.account.asks, sidebar.mjs joinRow): what the vouching agent says it is,
// with Allow and Deny, above New Agent.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import { joinRow, sidebarRows } from '../../../app/web/public/sidebar.mjs'

const ask = { nonce: 'ab12', says: { key: 'ask.says', en: 'game · desktop · claude-max' } }

test('an ask is a row with what it says, Allow and Deny', () => {
  const row = String(joinRow(ask))
  assert.match(row, /class="agent-row join-row" id="join-ab12" data-state="ask"/)
  assert.match(row, /<strong>game · desktop · claude-max<\/strong>/)
  assert.match(row, /action="\/vouch\/ab12"[^]*name="allow" value="yes"[^]*>Allow</)
  assert.match(row, /name="allow" value="no"[^]*>Deny</)
})

test('the rows stand in the sidebar\'s list of sessions, above New Agent', () => {
  const vm = { sidebar: { sessions: [] }, account: { step: 'none', asks: [ask] } }
  const html = String(sidebarRows(vm, ''))
  assert.ok(html.indexOf('id="join-ab12"') >= 0 && html.indexOf('id="join-ab12"') < html.indexOf('id="sidebar-invite"'))
})
