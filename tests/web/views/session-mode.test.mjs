// session-mode.test.mjs: the switch "All | Questions n" in a session's one-line heading (a phone: its own row under
// the top line; session.mjs sessionFilters): two real links, the one shown marked, the number of questions waiting
// (SessionView.open) in it. (session.mjs imports app.mjs and the core: read as source here.)
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
const src = fs.readFileSync(new URL('../../../app/web/public/session.mjs', import.meta.url), 'utf8')

test('All links to the talk, Questions to /questions with the number waiting; the one shown is marked', () => {
  const fn = src.slice(src.indexOf('export function sessionFilters'), src.indexOf('\n}\n', src.indexOf('export function sessionFilters')))
  assert.match(fn, /const on = mode === 'questions', n = s\.open/)
  assert.match(fn, /<a data-nav href="\$\{sessionPath\(s, base\)\}"\$\{on \? '' : raw\(' aria-current="page"'\)\}>All<\/a>/)
  assert.match(fn, /<a data-nav href="\$\{sessionPath\(s, base\)\}\/questions"\$\{on \? raw\(' aria-current="page"'\) : ''\}/)
  assert.match(fn, /Questions\$\{n \? html`<b aria-label="\$\{n\} waiting">\$\{n\}<\/b>` : ''\}/)
})

test('the questions page is its own address; the old ?only=questions leads there', () => {
  assert.match(src, /t\.get\(\/\^\\\/chat\\\/\(\[\^\/\+\]\+\)\\\/\(questions\|files\)\$\//)
  assert.match(src, /if \(url\.searchParams\.get\('only'\) === 'questions'\) return t\.redirect\(res, `\$\{BASE\}\/chat\/\$\{match\[1\]\}\/questions`\)/)
})
