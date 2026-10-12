// demo-way.test.mjs: the start page offers the demo as its third way, right under Create account and Log in
// (auth.mjs welcome, DEMO_WAY), not only at the page's foot.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'

const src = fs.readFileSync(new URL('../../../app/web/public/auth.mjs', import.meta.url), 'utf8')

test('the start page: Create account, Log in, then Try the demo in the same stack; no second demo link at its foot', () => {
  const start = src.split('\n').find(l => l.includes('id="way-create"') && l.includes('id="way-login"'))
  assert.ok(start, 'the start page\'s ways')
  assert.match(start, /id="way-login">Log in<\/button>\$\{DEMO_WAY\}<\/div>/)
  assert.match(start, /demo: false/)
  assert.match(src, /const DEMO_WAY = html`<a href="\/\?mock=1" id="ob-demo" class="ob-demo-way"/)
})
