// live-answer.test.mjs: the answer an agent is writing (spec/v1.md 7.5, SessionView.live_answer): under the working face,
// an agent's bubble that grows, a caret at its end, hidden from screen readers (the message is read once it is there);
// nothing while the core gives none.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'

const read = f => fs.readFileSync(new URL(`../../../app/web/public/${f}`, import.meta.url), 'utf8')

test('the answer as it is written stands under the face, as the agent\'s bubble', () => {
  const src = read('session.mjs')
  assert.match(src, /\$\{workingFace\(s, base\)\}\$\{liveAnswer\(s\)\}\$\{wakeLine\(s, base\)\}/)
  assert.match(src, /<article class="msg msg-agent msg-live" id="live-answer-\$\{s\.id\}" aria-hidden="true"><div class="msg-col"><div class="bubble bubble-agent"><div class="rich">\$\{words\(s\.live_answer\)\}<\/div>/)
  assert.match(src, /: html`<span id="live-answer-\$\{s\.id\}" hidden><\/span>`/)
  assert.match(fs.readFileSync(new URL('../../../core/bindings/wasm/js/views.d.ts', import.meta.url), 'utf8'), /live_answer\?: string/)
})

test('a caret at its end, still when motion is reduced', () => {
  const css = read('session.css')
  assert.match(css, /\.msg-live \.rich > :last-child::after \{ content: ""/)
  assert.match(css, /@media \(prefers-reduced-motion: reduce\) \{ \.msg-live \.rich > :last-child::after \{ animation: none; \} \}/)
})
