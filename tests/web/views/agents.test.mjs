// agents.test.mjs: Settings · Agents (app/web/public/agents.mjs, ui.mjs agentsOf): the sessions grouped into the
// concrete agents, a program on a computer as the connector reports it (its device register: platform, host, folder;
// the session's model where no platform is said), the main sessions of each as the branches of its tree, the helpers
// counted beside them; and the page drawn from that.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import { agentsOf, programOf } from '../../../app/web/public/ui.mjs'
import { agentsMain } from '../../../app/web/public/agents.mjs'

const member = (platform, host, folder = '') => ({ device_role: 'agent', platform, host, folder })
const members = new Map([
  ['d1', member('claude-code', 'laptop', '~/code/web')], ['d2', member('claude-code', 'laptop', '~/code/web/tests')],
  ['d3', member('codex', 'laptop', '~/code/api')], ['d4', member(null, 'build-01', '~/srv/etl')],
  ['h1', { device_role: 'human', platform: 'web', host: null, folder: null }],
])
const session = (id, device, more = {}) => ({ id, name: id, agent_device_id: device, online: true, seen: 0, model: '', ...more })
const everyone = [
  session('web', 'd1'), session('tests', 'd2', { parent: 'web' }), session('lint', 'd2', { parent: 'web', online: false }),
  session('api', 'd3'), session('etl', 'd4', { model: 'gpt-6.1', online: false, seen: 5 }),
  session('old', 'd1', { archived: true }), session('mine', 'h1', { own: true }),
]

test('the program: what the connector says, else the session\'s model', () => {
  assert.equal(programOf({ platform: 'claude-code' }, {}), 'Claude Code')
  assert.equal(programOf({ platform: 'codex' }, { model: 'claude-opus' }), 'Codex')
  assert.equal(programOf({ platform: null }, { model: 'gpt-6.1-sol' }), 'Codex')
  assert.equal(programOf(undefined, { model: 'claude-opus-5-5' }), 'Claude Code')
  assert.equal(programOf(undefined, { model: '' }), 'Agent')
})

test('sessions grouped into agents by program and computer; main sessions with their helpers counted; archived and own sessions left out', () => {
  const groups = agentsOf(everyone, members, { me: 'h1' })
  assert.deepEqual(groups.map(g => [g.program, g.host, g.sessions.map(a => a.id)]), [
    ['Claude Code', 'laptop', ['web', 'tests', 'lint']],
    ['Codex', 'build-01', ['etl']],
    ['Codex', 'laptop', ['api']],
  ])
  const laptop = groups[0]
  assert.deepEqual(laptop.tops.map(t => [t.a.id, t.helpers, t.folder]), [['web', 2, '~/code/web']])
  assert.deepEqual(laptop.devices, ['d1', 'd2'], 'removing it removes the agent devices of all its sessions')
  assert.equal(laptop.online, true)
  assert.equal(groups[1].online, false)
  assert.equal(groups[1].seen, 5)
})

test('the page: one card per agent, "<program> on <computer>", its sessions as branches that open the session, Remove behind "…"', () => {
  const page = String(agentsMain(agentsOf(everyone, members), '', () => 'Personal'))
  assert.match(page, /<h2>Agents<\/h2>/)
  assert.equal((page.match(/class="set-group ag-card"/g) ?? []).length, 3)
  assert.match(page, /<b>Claude Code <span>on laptop<\/span><\/b>/)
  assert.match(page, /href="\/chat\/web" data-nav>/)
  assert.doesNotMatch(page, /href="\/chat\/tests"/, 'a helper is no branch of its own: it is counted')
  assert.match(page, /\+2 helpers/)
  assert.match(page, /Personal desk · <span class="room-mono">~\/code\/web<\/span>/)
  assert.match(page, /action="\/agents\/remove"[\s\S]*?name="key" value="Claude Code · laptop"[\s\S]*?Remove from This Account/)
  assert.match(page, /Invite an Agent…/)
})

test('an agent whose computer is not reported is named so; no agent yet says so', () => {
  const page = String(agentsMain(agentsOf([session('x', 'dx')], new Map([['dx', member('claude-code', '')]])), '', () => 'Personal'))
  assert.match(page, /Claude Code <span>on an unnamed computer<\/span>/)
  assert.match(String(agentsMain([], '', () => '')), /No agent yet\./)
})
