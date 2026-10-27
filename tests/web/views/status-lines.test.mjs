// status-lines.test.mjs: the running work at the end of a chat (session.mjs workingFace and statusRow, session.css),
// the design "Helfer links" (27 October 2026): under "<agent> is working" and its dots, one row per status line (no
// box, no tooltip, no click needed) in columns: its sign, who works on it, the label and the grey detail in what is
// left, the age at the right; the row is the way to its card. The core's "4 more" / "Show fewer" for the rest, the
// core's "12 steps" at the right, which shows the turn's trail, and the wakeup in the same columns. The core orders the
// lines (what waits on him first), says how many stand open, and has every word (views/session.rs: status_shown,
// status_more, status_less, StatusLineView who, age and link, WorkingView steps; views/wakeups.rs WakeLineView).
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'

const read = f => fs.readFileSync(new URL(`../../../app/web/public/${f}`, import.meta.url), 'utf8')
const core = () => fs.readFileSync(new URL('../../../core/client/src/views/session.rs', import.meta.url), 'utf8')

test('the core opens the first lines, the shell draws them a row each and the rest behind the core "4 more"', () => {
  const src = read('session.mjs')
  assert.match(src, /open = lines\.slice\(0, s\.status_shown \?\? 0\), rest = lines\.slice\(open\.length\)/)
  assert.match(src, /<div class="status-rows" role="list">\$\{open\.map\(l => statusRow\(l, s, base\)\)\}<\/div>/)
  assert.match(src, /<div class="status-rows" id="status-more-\$\{s\.id\}" data-part="more" role="list" hidden>\$\{rest\.map\(l => statusRow\(l, s, base\)\)\}<\/div><button type="button" class="status-pile" data-opens="more" data-more="\$\{s\.status_more\.en\}" data-less="\$\{s\.status_less\?\.en \?\? s\.status_more\.en\}"/)
  assert.match(src, /const word = btn\.querySelector\('\.status-pile-word'\), say = open \? btn\.dataset\.less : btn\.dataset\.more/)
  assert.doesNotMatch(src, /<details class="working-face"|title="Details"|status-chip|status-open|status-helper|status-card/, 'no fold-out, no tooltip, no chip, no link word')
  const rs = core()
  assert.match(rs, /status_lines\.sort_by_key\(\|l\| \(rank\(&l\.state\), std::cmp::Reverse\(l\.updated_ms\)\)\);/)
  assert.match(rs, /"decision" => 0,\s+"working" => 1,/)
  assert.match(rs, /const CALM_LINES: usize = 3;/)
  assert.match(rs, /format!\("\{behind\} more"\)/)
})

test('a row: sign, who, label, detail, the core age; the row is the way to its card', () => {
  const src = read('session.mjs')
  assert.match(src, /\$\{lineMark\(l\)\}<span class="status-who">\$\{l\.who\}<\/span><span class="status-line"><b class="status-label">\$\{l\.label\}<\/b>\$\{l\.detail \? html`<span class="status-detail">\$\{l\.detail\}<\/span>` : ''\}\$\{l\.ran \? html`<span class="status-ran">\$\{l\.ran\.en\}<\/span>` : ''\}<\/span>/)
  assert.match(src, /<span class="status-age" aria-hidden="true">\$\{l\.age\.en\}<i>\$\{l\.card \? '›' : ''\}<\/i><\/span>\$\{agoSpan\(l\.updated_ms, 'offscreen'\)\}/)
  assert.match(src, /<div class="status-item" role="listitem">\$\{l\.card \? html`<a class="status-row" data-state="\$\{l\.state\}" data-nav href="\$\{cardPath\(l\.card, s, base\)\}">/)
})

test('a helper\'s line: what it is at in the detail\'s place and the core\'s "running 50m" / "finished" after it; waiting on helpers, the face is still', () => {
  const src = read('session.mjs'), css = read('session.css'), rs = core()
  assert.match(src, /\$\{l\.ran \? html`<span class="status-ran">\$\{l\.ran\.en\}<\/span>` : ''\}/)
  assert.match(css, /\.status-ran \{ flex: none;[^}]*white-space: nowrap; \}/, 'never cut')
  assert.match(rs, /"line\.helper\.running",\s+format!\("running \{\}", span\.en\)/)
  assert.match(rs, /say\("line\.helper\.finished", "finished", &\[\]\)/)
  // (the turn is over, helpers run: the core's words where the face stands, no dots, the rows under it as ever)
  assert.match(src, /const waits = !s\.working && s\.waits_on_helpers/)
  assert.match(src, /const w = s\.working \?\? \{ says: s\.waits_on_helpers \}/)
  assert.match(src, /\$\{waits \? '' : html`<span class="dots" aria-hidden="true">/)
  assert.match(src, /avatar\(\{ \.\.\.whoOf\(s\), online: true \}, \{ working: !waits \}\)/)
  assert.match(rs, /"session\.waiting_helpers"/)
})

test('the wakeup in the rows\' columns: the core\'s short time, its note, "and 2 more"; no sentence made here', () => {
  const src = read('session.mjs')
  assert.match(src, /<a class="wake-line" id="wake-line-\$\{s\.id\}" data-nav href="\$\{base\}\/wakeups" aria-label="\$\{w\.says\.en\}">/)
  assert.match(src, /<span class="wake-at">\$\{w\.at\.en\}<\/span><span class="wake-note">\$\{w\.note\}\$\{w\.more \? html` <i>\$\{w\.more\.en\}<\/i>` : ''\}<\/span>/)
  assert.match(read('session.css'), /^\.wake-note \{[^}]*grid-column: 3 \/ 5;[^}]*text-overflow: ellipsis;/m)
})

test('the steps in words at the right of "… is working"; a click shows the trail', () => {
  const src = read('session.mjs')
  assert.match(src, /<button type="button" class="working-steps"[^\n]*data-action="working-face#steps">\$\{w\.steps\.en\}/)
  assert.match(src, /<div class="working-details" id="working-trail-\$\{s\.id\}" data-part="steps" hidden>/)
  assert.match(read('session.css'), /^\.working-steps \{[^}]*margin-left: auto;/m)
})

test('a row never wraps: the columns line up, the label has the first claim, the detail gives way', () => {
  const css = read('session.css')
  assert.match(css, /^\.status-row, \.status-pile, \.wake-line \{[^}]*grid-template-columns: 16px 6rem minmax\(0, 1fr\) 3rem;[^}]*white-space: nowrap;/m)
  assert.match(css, /^\.status-label \{[^}]*flex: 0 1 auto;[^}]*text-overflow: ellipsis;/m)
  assert.match(css, /^\.status-detail \{[^}]*flex: 1 1 0; min-width: 0; overflow: hidden; text-overflow: ellipsis;/m)
})

test('a line nobody updated for two hours is stale (the core says which lines show): greyed, a still ring', () => {
  // (the core decides stale, core/client/src/status.rs line_state)
  assert.match(fs.readFileSync(new URL('../../../core/client/src/status.rs', import.meta.url), 'utf8'), /pub const LINE_STALE_MS: u64 = 2 \* 60 \* 60_000;/)
  const src = read('session.mjs'), css = read('session.css')
  assert.match(src, /l\.state === 'stale' \? html`<i class="status-still"/)
  assert.match(css, /\.status-row:is\(\[data-state="later"\], \[data-state="done"\], \[data-state="stale"\]\) \.status-label \{ color: var\(--muted\); \}/)
})
