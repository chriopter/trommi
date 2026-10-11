// desk-no-grow.test.mjs: a Desk row never grows (desk.css): the pointer resting on it or the focus in it shows no more
// text, so no rule under :hover or :focus-within brings the row's body (the line under the title) back.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'

const css = fs.readFileSync(new URL('../../../app/web/public/desk.css', import.meta.url), 'utf8')

test('no hover or focus rule shows a Desk row\'s body', () => {
  const rules = css.replace(/\/\*[\s\S]*?\*\//g, '').split('}').filter(r => /#inbox/.test(r) && /:(hover|focus-within)/.test(r) && /\.inbox-body/.test(r))
  assert.deepEqual(rules, [])
  assert.match(css, /#inbox \.inbox-row \.inbox-body \{ display: none !important; \}/, 'the body stays hidden on the Desk')
})
