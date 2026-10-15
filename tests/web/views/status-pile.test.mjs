// status-pile.test.mjs: the status chips under a chat never scroll sideways (the owner, 13 October 2026): what does
// not fit lies on a stack "+N" at the row's end, listed under the row on a tap (session.mjs status-lines), on the
// iPhone the same (SessionScreen.swift SessionStatus).
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'

const read = p => fs.readFileSync(new URL(`../../../${p}`, import.meta.url), 'utf8')

test('the row does not scroll sideways; the chips past its end lie on the stack', () => {
  const css = read('app/web/public/session.css'), src = read('app/web/public/session.mjs')
  const row = css.match(/^\.status-chips \{[^}]*\}/m)?.[0] ?? ''
  assert.doesNotMatch(row, /overflow-x: auto/)
  assert.match(src, /const over = all \? \[\] : chips\.filter\(c => c\.getBoundingClientRect\(\)\.right > right - room\)/)
  assert.match(src, /this\.pile\.innerHTML = `<span class="status-pile-n">\+\$\{over\.length\}<\/span>`/)
  assert.doesNotMatch(src, /trommi-chips/, 'no switch between proposals any more')
})
