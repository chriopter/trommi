// status-lines.test.mjs: the running work at the end of a chat (session.mjs sessionStatus, session.css): one calm line
// (the drawing, "… is working" and its dots, the status line its agent set last, the core's "+N" for the rest, which a
// tap lists under the line); a tap on a line opens it under the line: its whole text, how it stands, and the way to
// its card when it stands for one. The core orders the lines and counts the rest (views/session.rs status_more).
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'

const read = f => fs.readFileSync(new URL(`../../../app/web/public/${f}`, import.meta.url), 'utf8')

test('one line: the first of the core lines beside the dots, the rest behind the core "+N"', () => {
  const src = read('session.mjs')
  assert.match(src, /const lines = s\.status_lines \?\? \[\], \[first, \.\.\.rest\] = lines/)
  assert.match(src, /\$\{statusChip\(first, s, base\)\}/)
  assert.match(src, /\$\{s\.status_more \? html`<button type="button" class="status-pile"[^\n]*data-action="status-lines#more">\$\{s\.status_more\.en\}<\/button>`/)
  assert.match(src, /<div class="status-more" id="status-more-\$\{s\.id\}" role="list" hidden>\$\{rest\.map\(l => statusChip\(l, s, base\)\)\}<\/div>/)
  assert.doesNotMatch(src, /status-chips|getBoundingClientRect\(\)\.right/, 'no row of chips measured any more')
  assert.match(fs.readFileSync(new URL('../../../core/client/src/views/session.rs', import.meta.url), 'utf8'), /status_lines\.sort_by_key\(\|l\| std::cmp::Reverse\(l\.updated_ms\)\);/)
})

test('a tap on any line opens it: its whole text, how it stands, its card', () => {
  const src = read('session.mjs')
  assert.doesNotMatch(src, /<a class="status-chip"/, 'a line with a card opens too, it does not leave the page at once')
  assert.match(src, /<button type="button" class="status-chip"[^\n]*data-href="\$\{cardPath\(l\.card, s, base\)\}"[^\n]*data-action="status-lines#toggle"/)
  assert.match(src, /<div class="status-open" id="status-open-\$\{s\.id\}" hidden><\/div>/)
  assert.match(src, /<p class="status-open-text"><b>\$\{d\.label\}<\/b>\$\{d\.detail \? html`: \$\{d\.detail\}` : ''\}<\/p>/)
  assert.match(src, /<a class="status-open-card" data-nav href="\$\{d\.href\}">Open card/)
})

test('the line never wraps: the title gives way, the words before it do not', () => {
  const css = read('session.css')
  assert.match(css, /^\.status-chip \{[^}]*flex: 0 1 auto; min-width: 0;/m)
  assert.match(css, /^\.status-chip b \{ overflow: hidden; text-overflow: ellipsis; white-space: nowrap;/m)
  assert.match(css, /^\.working-says \{ flex: none; white-space: nowrap; \}/m)
})

test('the age is short, the words kept for a screen reader', async () => {
  const { agoShort } = await import('../../../app/web/public/ui.mjs')
  const now = Date.now()
  assert.equal(agoShort(now - 4 * 60000, now), '4m')
  assert.equal(agoShort(now - 3 * 3600000, now), '3h')
  const src = read('session.mjs')
  assert.match(src, /<span class="status-age" aria-hidden="true" data-ts="\$\{l\.updated_ms\}">\$\{agoShort\(l\.updated_ms\)\}<\/span>\$\{agoSpan\(l\.updated_ms, 'offscreen'\)\}/)
})

test('a line nobody updated for two hours is stale (the core says which lines show): greyed, a still ring', async () => {
  // (the core decides stale, core/client/src/status.rs line_state)
  assert.match(fs.readFileSync(new URL('../../../core/client/src/status.rs', import.meta.url), 'utf8'), /pub const LINE_STALE_MS: u64 = 2 \* 60 \* 60_000;/)
  const src = read('session.mjs'), css = read('session.css')
  assert.match(src, /l\.state === 'stale' \? html`<i class="status-still"/)
  assert.match(css, /\.status-chip\[data-state="stale"\] \{ color: var\(--muted\); \}/)
})
