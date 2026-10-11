// usage.test.mjs: an agent's account and usage (ui.mjs usageMeters, usageMark, usageFull; app.mjs BoardState
// carries it onto each session of the agent; session.mjs and agents.mjs show it). The connector writes it into its
// main session's profile as `account` (connector/src/usage.rs).
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import { emptyModel } from '../../../app/web/core/model-shape.ts'
import { sessionOf } from '../../../app/web/core/model.ts'
import { BoardState } from '../../../app/web/public/app.mjs'
import { usageMark, usageFull, usageMeters, usagePlan, usageReset, agentsOf } from '../../../app/web/public/ui.mjs'
import { agentsMain } from '../../../app/web/public/agents.mjs'

process.env.TZ = 'UTC'
const NOW = Date.UTC(2026, 9, 11, 8, 30)   // Sun 11 Oct 2026 08:30
const claude = { kind: 'claude', plan: 'max', email: 'ada@example.org', five_hour: { used: 62, resets_at: NOW + 313 * 60_000 }, weekly: { used: 84, resets_at: NOW + 3 * 86_400_000 } }
const codex = { kind: 'codex', plan: 'prolite', email: 'ada@work.example', weekly: { used: 41, resets_at: NOW + 4 * 86_400_000 }, credits: 2400, full_resets: 1 }

test('the meters: Claude Code the 5-hour window and the week, Codex the week; a window that started again is left out', () => {
  assert.deepEqual(usageMeters(claude, NOW).map(m => [m.label, m.used, m.reset]), [['5 h', 62, '13:43'], ['week', 84, 'Wed 14 Oct']])
  assert.deepEqual(usageMeters(codex, NOW).map(m => m.label), ['week'])
  assert.deepEqual(usageMeters({ ...claude, five_hour: { used: 90, resets_at: NOW - 1 } }, NOW).map(m => m.label), ['week'])
  assert.deepEqual(usageMeters({ weekly: { used: 130 } }, NOW).map(m => [m.used, m.reset]), [[100, '']])
  assert.deepEqual(usageMeters(null), [])
  assert.equal(usageReset(NOW + 60_000, NOW), '08:31')
})

test('the plan tag names the program\'s company and the plan', () => {
  assert.equal(usagePlan(claude), 'Claude Max')
  assert.equal(usagePlan(codex), 'ChatGPT Pro Lite')
  assert.equal(usagePlan({ kind: 'claude', plan: 'ultra' }), 'Claude Ultra')
})

test('the mark in the heading: two rings one inside the other, the week outside, 5 h inside; hot from 80 %', () => {
  const html = String(usageMark(claude, { now: NOW }))
  assert.equal((html.match(/class="use-track"/g) ?? []).length, 2, 'two tracks')
  const fills = [...html.matchAll(/<path d="M16\.00 (\d+\.\d+) A([\d.]+) [^"]*" class="use-fill( is-hot)?"/g)].map(m => [m[2], Boolean(m[3])])
  assert.deepEqual(fills, [['13', true], ['7.6', false]], 'the week (84 %, hot) on the outer ring, 5 h (62 %) inside')
  assert.match(html, /<b class="">62 %<\/b> 5 h<\/span><span><b class="is-hot">84 %<\/b> week/)
  assert.match(html, /title="5-hour window: 62 % used, resets 13:43 · This week: 84 % used, resets Wed 14 Oct"/)
  assert.doesNotMatch(html, /ada@example\.org/, 'no e-mail in the heading')
  const mini = String(usageMark(claude, { mini: true, now: NOW }))
  assert.match(mini, /class="use-rings is-mini"/)
  assert.doesNotMatch(mini, /use-words/, 'a phone: the mark alone')
  assert.equal(String(usageMark({ kind: 'claude', plan: 'max' }, { now: NOW })), '', 'no meters, nothing')
  assert.equal(String(usageMark(null)), '')
})

test('the session heading is one line: name, the connection as a dot, the switch, the usage, More; no second line', () => {
  const src = fs.readFileSync(new URL('../../../app/web/public/session.mjs', import.meta.url), 'utf8')
  assert.match(src, /<header class="pane-title">\$\{sessionWho\(s, base\)\}\$\{sessionFilters\(s, base, mode\)\}\$\{sessionUsage\(s\)\}<\/header>\$\{sessionFilters\(s, base, mode, \{ phone: true \}\)\}/)
  assert.match(src, /\$\{sessionHeadEdit\(a, base, \{ stay: true \}\)\}\$\{sessionDot\(a\)\}/)
  assert.doesNotMatch(src, /class="pane-now"/, 'the quiet line under the name is gone')
})

test('full, on the agent\'s card: the plan tag and the e-mail, one row per meter with its reset, Codex\'s credits and resets', () => {
  const html = String(usageFull(claude, NOW))
  assert.match(html, /<span class="use-plan">Claude Max<\/span><span class="use-mail">ada@example\.org<\/span>/)
  assert.match(html, /<div class="use-row"><span class="use-l">5-hour window<\/span><span class="use-bar is-wide"[^>]*>.*?<b class="">62 %<\/b><small>resets 13:43<\/small><\/div>/)
  assert.match(html, /This week.*?<b class="is-hot">84 %<\/b><small>resets Wed 14 Oct<\/small>/)
  assert.doesNotMatch(html, /use-facts/)
  const cx = String(usageFull(codex, NOW))
  assert.match(cx, /ChatGPT Pro Lite/)
  assert.match(cx, /<p class="use-facts"><span class="use-fact">2,400 credits<\/span><span class="use-fact">1 full reset<\/span><\/p>/)
  assert.equal(String(usageFull({ kind: 'claude' })), '', 'nothing without a plan or an e-mail')
})

test('every session of an agent carries its main session\'s account; a helper of its own device its main\'s', () => {
  const m = emptyModel()
  m.room.my_device_id = 'ee'.repeat(16)
  const D1 = 'd1'.repeat(16), D2 = 'd2'.repeat(16), S1 = '51'.repeat(16), S2 = '52'.repeat(16), S3 = '53'.repeat(16)
  for (const d of [D1, D2]) m.members.set(d, { device_id: d, device_role: 'agent', platform: 'claude-code', host: 'laptop', folder: '~/w', is_active: true, is_online: true })
  const s = (id, device, profile, parent = null) => Object.assign(sessionOf(m, id), { group_id: `g${id}`, agent_device_id: device, agent_device_ids: [device], is_online: true, parent_session_id: parent, creator_device_id: parent ? device : null, profile })
  s(S1, D1, { is_main: true, account: claude })
  s(S2, D1, { is_main: false, parent_session: S1 }, S1)        // a helper session of the same connector
  s(S3, D2, { is_main: false, parent_session: S1 }, S1)        // a helper that is a device of its own
  const agents = new BoardState({ model: m }).update().agents
  for (const a of agents) assert.deepEqual(a.account, claude, a.id)
  const page = String(agentsMain(agentsOf(agents, m.members), '', () => 'Personal'))
  assert.equal((page.match(/class="use-full"/g) ?? []).length, 1)
  assert.match(page, /<\/div>\s*<div class="use-full">[\s\S]*?<\/div><ul class="ag-tree">/, 'between the head and the tree')
})
