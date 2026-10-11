// status-lines.test.mjs: the status lines at the end of a chat (session.mjs sessionStatus, session.css): on a phone one
// line each, the title and the time; a tap opens the detail and the way to its question below it, a second shuts it.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'

const read = f => fs.readFileSync(new URL(`../../../app/web/public/${f}`, import.meta.url), 'utf8')

test('each line is a row with what a tap opens: the detail and "Open the question"', () => {
  const src = read('session.mjs')
  assert.match(src, /class="status-row" data-key="\$\{t\.agent\}:\$\{t\.id\}"/)
  assert.match(src, /data-action="click->status-lines#toggle"/)
  assert.match(src, /<div class="status-more">[\s\S]*class="status-open" data-nav href="\$\{href\}">Open the question →<\/a>/)
  assert.match(src, /controller\('status-lines'[\s\S]*mq\('\(max-width: 860px\)'\)\.matches\) return/, 'on a wide screen a line stays a link')
})

test('on a phone the detail is hidden in the line and shown only in an open row', () => {
  const css = read('session.css')
  const phone = css.slice(css.indexOf('.status-more { display: none; }'))
  assert.match(phone, /@media \(max-width: 860px\)[\s\S]*\.status-line \.status-detail \{ display: none; \}[\s\S]*\.status-row\[data-open\] \.status-more \{ display: grid;/)
})
