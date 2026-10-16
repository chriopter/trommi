// goals-fold.test.mjs: the team's goals fold with ONE toggle (the owner, 13 October 2026: "nur ein Auf- und
// Zuklappen, kein 'Hide goals'"): folded, the first line; open, every line (desk.mjs goalsWords, controller "goals").
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import { goalsWords } from '../../../app/web/public/desk.mjs'

const read = f => fs.readFileSync(new URL(`../../../${f}`, import.meta.url), 'utf8')
/** A GoalsView of n lines, as the core makes it (views.d.ts). */
const goals = n => ({ team_id: 'main', lines: Array.from({ length: n }, (_, i) => `goal ${i + 1}`), foldable: n > 1, folded: false,
  ...(n > 1 ? { show_all: { key: 'desk.goals.show_all', en: `Show all ${n} goals` }, toggle: { key: 'desk.goals.fold', en: 'Show the first goal only' } } : {}) })

test('open: every line and one chevron; no "Hide goals", no "+N more"', () => {
  const out = String(goalsWords(goals(12)))
  assert.equal(out.match(/class="dg-line"/g).length, 12, 'all twelve lines are drawn')
  assert.equal(out.match(/class="desk-goals-toggle"/g).length, 1, 'one toggle')
  assert.match(out, /data-action="goals#toggle" aria-expanded="true"/)
  assert.doesNotMatch(out, /Hide goals|desk-goals-more|desk-goals-fold|desk-goals-unfold|more<\/button>/)
})

test('folded: the toggle says so; the CSS shows the first line alone', () => {
  const out = String(goalsWords(goals(3), true))
  assert.match(out, /aria-expanded="false" aria-label="Show all 3 goals"/)
  const css = read('app/web/public/desk.css')
  assert.match(css, /\.desk-goals\.is-folded \.dg-line:nth-child\(n\+2\) \{ display: none; \}/)
  assert.doesNotMatch(css, /nth-child\(n\+6\)/, 'open shows every line, not the first five')
})

test('one line or none: nothing to fold, no toggle', () => {
  assert.doesNotMatch(String(goalsWords(goals(1))), /desk-goals-toggle/)
  assert.doesNotMatch(String(goalsWords(goals(0))), /desk-goals-toggle/)
})
