// status-lines.test.mjs: the running work at the end of a chat (session.mjs sessionStatus, session.css): one row of
// small chips (mark, title, age), never two rows; a tap on any chip opens it under the row: its whole text, how it
// stands, and the way to its card when it stands for one.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'

const read = f => fs.readFileSync(new URL(`../../../app/web/public/${f}`, import.meta.url), 'utf8')

test('the lines are one row of chips; a tap on any opens it: its whole text, how it stands, its card', () => {
  const src = read('session.mjs')
  assert.match(src, /<div class="status-chips" role="list">/)
  assert.doesNotMatch(src, /<a class="status-chip"/, 'a chip with a card opens too, it does not leave the page at once')
  assert.match(src, /<button type="button" class="status-chip"[^\n]*data-href="\$\{cardPath\(l\.card, s, base\)\}"[^\n]*data-action="status-lines#toggle"/)
  assert.match(src, /<div class="status-open" id="status-open-\$\{s\.id\}" hidden><\/div>/)
  assert.match(src, /<p class="status-open-text"><b>\$\{d\.label\}<\/b>\$\{d\.detail \? html`: \$\{d\.detail\}` : ''\}<\/p>/)
  assert.match(src, /<a class="status-open-card" data-nav href="\$\{d\.href\}">Open card/)
  assert.doesNotMatch(src, /stripMode|\?strip=/, 'no trial switch left')
})

test('the row never wraps and never scrolls sideways (what does not fit lies on a stack: status-pile.test.mjs)', () => {
  const css = read('session.css')
  assert.match(css, /^\.status-chips \{ display: flex;[^}]*overflow: visible;/m)
  assert.match(css, /^\.status-chip \{[^}]*flex: none;[^}]*max-width: 16rem;/m)
  assert.match(css, /^\.status-chip b \{ overflow: hidden; text-overflow: ellipsis; white-space: nowrap;/m)
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

test('the iPhone: a tap on any chip opens it with its card; a stale line greyed and still', () => {
  const swift = fs.readFileSync(new URL('../../../app/ios/TrommiApp/Sources/TrommiApp/SessionScreen.swift', import.meta.url), 'utf8')
  assert.match(swift, /Button \{\n\s*withAnimation\(\.easeOut\(duration: 0\.18\)\) \{ shown = on \? nil : key \}/)
  assert.match(swift, /Text\("Open card"\)/)
  assert.match(swift, /\$0\.state == "stale" && now - min\(now, \$0\.updated\) < 24 \* 60 \* 60 \* 1000/)
})
