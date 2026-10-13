// status-lines.test.mjs: the running work at the end of a chat (session.mjs sessionStatus, session.css): one row of
// small chips (mark, title, age), never two rows; a chip with a question opens it, one without shows its detail under
// the row on a tap.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'

const read = f => fs.readFileSync(new URL(`../../../app/web/public/${f}`, import.meta.url), 'utf8')

test('the lines are one row of chips: a question opens, the rest show their detail on a tap', () => {
  const src = read('session.mjs')
  assert.match(src, /<div class="status-chips" role="list">/)
  assert.match(src, /<a class="status-chip" role="listitem" data-state="\$\{t\.state\}" data-nav href="\$\{questionPath\(card, base\)\}"/)
  assert.match(src, /<button type="button" class="status-chip"[^`]*data-action="status-lines#toggle"/)
  assert.match(src, /<p class="status-detail-line" hidden><\/p>/)
  assert.doesNotMatch(src, /stripMode|\?strip=/, 'no trial switch left')
})

test('the row never wraps: on a phone it scrolls sideways', () => {
  const css = read('session.css')
  assert.match(css, /^\.status-chips \{ display: flex;[^}]*overflow-x: auto;/m)
  assert.match(css, /^\.status-chip \{[^}]*flex: none;[^}]*max-width: 16rem;/m)
  assert.match(css, /^\.status-chip b \{ overflow: hidden; text-overflow: ellipsis; white-space: nowrap;/m)
})

test('the age is short, the words kept for a screen reader', async () => {
  const { agoShort } = await import('../../../app/web/public/ui.mjs')
  const now = Date.now()
  assert.equal(agoShort(now - 4 * 60000, now), '4m')
  assert.equal(agoShort(now - 3 * 3600000, now), '3h')
  const src = read('session.mjs')
  assert.match(src, /<span class="status-age" aria-hidden="true" data-ts="\$\{t\.updated\}">\$\{agoShort\(t\.updated\)\}<\/span>\$\{agoSpan\(t\.updated, 'offscreen'\)\}/)
})

test('only what still runs shows: stale and gone lines do not, a done one for a while', () => {
  const src = read('session.mjs')
  assert.match(src, /const shownLines = \(tasks, now = Date\.now\(\)\) => tasks\.filter\(t => \['working', 'decision', 'later'\]\.includes\(t\.state\) \|\| \(t\.state === 'done' && now - \(t\.updated \?\? 0\) < WORKING_WINDOW\)\)/)
})
