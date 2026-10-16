// agents.test.mjs: Settings · Agents (app/web/public/agents.mjs) draws the app core's agents (SettingsView.agent_list,
// AgentView; grouping, counts and words are the core's, core/client/src/views/settings.rs): one card per agent, its
// main sessions as the branches of its tree, Remove behind "…".
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import { agentsMain } from '../../../app/web/public/agents.mjs'

const T = en => ({ key: 'k', en })
const agent = (program, host, more = {}) => ({
  key: `${program} · ${host}`, program, host, name: T(`${program} on ${host || 'an unnamed computer'}`), online: true, state: T('connected'), seen_ms: 0,
  count: T('1 session, 2 helpers'), total: 3, devices: ['d1', 'd2'], remove: T(`${program} on ${host} and its 3 sessions can read nothing new after this.`),
  sessions: [{ id: 'k1', slug: 'web', name: T('Web'), mark: 'draw:leaf', team: T('Personal team'), folder: '~/code/web', helpers: 2, helpers_says: T('+2 helpers'), online: true, state: T('connected') }],
  ...more,
})

test('the page: one card per agent, "<program> on <computer>", its sessions as branches that open the session, Remove behind "…"', () => {
  const page = String(agentsMain({ agent_list: [agent('Claude Code', 'laptop'), agent('Codex', 'build-01', { online: false, state: T('away · last seen'), seen_ms: 5 })], agents_lead: T('2 agents on 2 computers · 2 sessions') }, ''))
  assert.match(page, /<h2>Agents<\/h2>/)
  assert.match(page, /2 agents on 2 computers · 2 sessions/)
  assert.equal((page.match(/class="set-group ag-card"/g) ?? []).length, 2)
  assert.match(page, /<b>Claude Code <span>on laptop<\/span><\/b>/)
  assert.match(page, /href="\/chat\/web" data-nav>/)
  assert.match(page, /\+2 helpers/)
  assert.match(page, /Personal team · <span class="room-mono">~\/code\/web<\/span>/)
  assert.match(page, /away · last seen <span class="ago" data-ts="5">/)
  assert.match(page, /action="\/agents\/remove"[\s\S]*?name="key" value="Claude Code · laptop"[\s\S]*?can read nothing new after this\.[\s\S]*?Remove from This Account/)
  assert.match(page, /Invite an Agent…/)
})

test('an agent whose computer is not reported is named so; no agent yet says so', () => {
  assert.match(String(agentsMain({ agent_list: [agent('Claude Code', '')] }, '')), /Claude Code <span>on an unnamed computer<\/span>/)
  assert.match(String(agentsMain({}, '')), /No agent yet\./)
})

test('the account of an agent: plan, e-mail, its meters as the core says them', () => {
  const page = String(agentsMain({ agent_list: [agent('Codex', 'laptop', { usage: { plan: 'ChatGPT Pro', email: 'a@b.c', meters: [{ key: 'weekly', label: 'week', long: 'This week', used: 84, hot: true }], facts: [T('2 full resets')] } })] }, ''))
  assert.match(page, /<span class="use-plan">ChatGPT Pro<\/span><span class="use-mail">a@b\.c<\/span>/)
  assert.match(page, /<span class="use-l">This week<\/span><span class="use-bar is-hot is-wide"[^>]*><i style="--used:84%"><\/i><\/span><b class="is-hot">84 %<\/b>/)
  assert.match(page, /<span class="use-fact">2 full resets<\/span>/)
})
