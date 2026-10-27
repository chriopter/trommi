// status-lines.test.mjs: the running work at the end of a chat (session.mjs workingFace and statusRow, session.css),
// the design "Lose Zeilen": under "<agent> is working" and its dots, a loose row per status line (no box, no tooltip,
// no click needed): its sign, label, grey detail, age and "Card ›"; the core's "+N" for the rest and the core's
// "12 steps" at the right, which shows the turn's trail. The core orders the lines, says how many stand open, and has
// every word (views/session.rs: status_shown, status_more, StatusLineView age and link, WorkingView steps).
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'

const read = f => fs.readFileSync(new URL(`../../../app/web/public/${f}`, import.meta.url), 'utf8')
const core = () => fs.readFileSync(new URL('../../../core/client/src/views/session.rs', import.meta.url), 'utf8')

test('the core opens the first lines, the shell draws them a row each and the rest behind the core "+N"', () => {
  const src = read('session.mjs')
  assert.match(src, /open = lines\.slice\(0, s\.status_shown \?\? 0\), rest = lines\.slice\(open\.length\)/)
  assert.match(src, /<div class="status-rows" role="list">\$\{open\.map\(l => statusRow\(l, s, base\)\)\}<\/div>/)
  assert.match(src, /data-action="working-face#more">\$\{s\.status_more\.en\}<\/button><div class="status-rows" id="status-more-\$\{s\.id\}" data-part="more" role="list" hidden>\$\{rest\.map/)
  assert.doesNotMatch(src, /<details class="working-face"|title="Details"|status-chip|status-open/, 'no fold-out, no tooltip, no line to open')
  const rs = core()
  assert.match(rs, /status_lines\.sort_by_key\(\|l\| std::cmp::Reverse\(l\.updated_ms\)\);/)
  assert.match(rs, /const CALM_LINES: usize = 3;/)
})

test('a row: sign, label, detail, the core age and link word, the way to its card', () => {
  const src = read('session.mjs')
  assert.match(src, /const statusRow = \(l, s, base\) => html`<div class="status-row" role="listitem" data-state="\$\{l\.state\}">\$\{lineMark\(l\)\}/)
  assert.match(src, /<span class="status-detail">\$\{l\.detail\}<\/span>/)
  assert.match(src, /<span class="status-age" aria-hidden="true">\$\{l\.age\.en\}<\/span>\$\{agoSpan\(l\.updated_ms, 'offscreen'\)\}/)
  assert.match(src, /<a class="status-card" data-nav href="\$\{cardPath\(l\.card, s, base\)\}">\$\{l\.link\.en\}/)
})

test('the steps in words at the right of "… is working"; a click shows the trail', () => {
  const src = read('session.mjs')
  assert.match(src, /<button type="button" class="working-steps"[^\n]*data-action="working-face#steps">\$\{w\.steps\.en\}/)
  assert.match(src, /<div class="working-details" id="working-trail-\$\{s\.id\}" data-part="steps" hidden>/)
  assert.match(read('session.css'), /^\.working-steps \{[^}]*margin-left: auto;/m)
})

test('a row never wraps on a wide screen: the detail gives way', () => {
  const css = read('session.css')
  assert.match(css, /^\.status-row \{[^}]*grid-template-columns: 16px auto minmax\(0, 1fr\) auto auto;/m)
  assert.match(css, /^\.status-detail \{[^}]*text-overflow: ellipsis; white-space: nowrap;/m)
})

test('a line nobody updated for two hours is stale (the core says which lines show): greyed, a still ring', () => {
  // (the core decides stale, core/client/src/status.rs line_state)
  assert.match(fs.readFileSync(new URL('../../../core/client/src/status.rs', import.meta.url), 'utf8'), /pub const LINE_STALE_MS: u64 = 2 \* 60 \* 60_000;/)
  const src = read('session.mjs'), css = read('session.css')
  assert.match(src, /l\.state === 'stale' \? html`<i class="status-still"/)
  assert.match(css, /\.status-row:is\(\[data-state="later"\], \[data-state="done"\], \[data-state="stale"\]\) b \{ color: var\(--muted\); \}/)
})
