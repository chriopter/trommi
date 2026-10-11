// session-mode.test.mjs: the switch "All | Questions n" in a session's one-line heading (a phone: its own row under
// the top line; session.mjs sessionFilters): two real links, the one shown marked, the number of questions waiting in it; it is not beside the composer any more.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import { sessionFilters } from '../../../app/web/public/session.mjs'

const s = n => ({ id: 'ada', fresh: Array.from({ length: n }, (_, i) => ({ id: `c${i}` })) })

test('the chat: All is marked, Questions links to ?only=questions and carries the number waiting', () => {
  const html = String(sessionFilters(s(3), ''))
  assert.match(html, /<nav class="session-mode" id="session-filters-ada" aria-label="Show">/)
  assert.match(String(sessionFilters(s(3), '', '', { phone: true })), /<nav class="session-mode is-phone" id="session-filters-phone-ada" aria-label="Show">/)
  assert.match(html, /<a data-nav href="\/chat\/ada" aria-current="page">All<\/a>/)
  assert.match(html, /<a data-nav href="\/chat\/ada\?only=questions"[^>]*>Questions<b aria-label="3 waiting">3<\/b><\/a>/)
})

test('the questions: Questions is marked; without a question waiting no number', () => {
  const html = String(sessionFilters(s(0), '', 'questions'))
  assert.match(html, /<a data-nav href="\/chat\/ada">All<\/a>/)
  assert.match(html, /href="\/chat\/ada\?only=questions" aria-current="page"[^>]*>Questions<\/a>/)
  assert.doesNotMatch(html, /<b /)
})
