// session-model.test.mjs: a session's heading says the model it runs on, quiet beside the dot (the owner, 13 October
// 2026): "Opus 5.5", "GPT-6.1". The short label is the client core's (core/client/src/sessions.rs model_label, tested
// there); web and iPhone only draw it. The demo room makes the same label.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'

const read = p => fs.readFileSync(new URL(`../../../${p}`, import.meta.url), 'utf8')

test('the heading draws the core\'s label beside the dot; the full id is its tip', () => {
  const src = read('app/web/public/session.mjs')
  assert.match(src, /\$\{sessionDot\(a\)\}\$\{a\.model_label \? html`<span class="head-model" title="\$\{a\.model\}">\$\{a\.model_label\}<\/span>` : ''\}/)
  assert.match(read('app/web/public/app.mjs'), /model_label: r\.model_label \?\? ''/, 'the label comes with the core\'s session row')
  assert.match(read('app/web/public/session.css'), /\.head-model \{[^}]*color: var\(--muted\)/)
})

test('the demo room makes the core\'s label', () => {
  const src = read('app/web/public/demo/demo.mjs')
  const fn = new Function(`${src.slice(src.indexOf('const demoModelLabel'), src.indexOf('/**', src.indexOf('const demoModelLabel')))}; return demoModelLabel`)()
  assert.equal(fn('claude-opus-5-5'), 'Opus 5.5')
  assert.equal(fn('claude-sonnet-4-5-20250929'), 'Sonnet 4.5')
  assert.equal(fn('gpt-6.1'), 'GPT-6.1')
  assert.equal(fn('gpt-5.1-codex'), 'GPT-5.1 Codex')
  assert.equal(fn('o4-mini'), 'o4-mini')
})

test('the iPhone shows the same label beside the name', () => {
  assert.match(read('app/ios/TrommiApp/Sources/TrommiApp/SessionScreen.swift'), /if !a\.modelLabel\.isEmpty \{ Text\(a\.modelLabel\)/)
  assert.match(read('app/ios/TrommiClient/Sources/TrommiClient/Desk.swift'), /a\.modelLabel = r\["model_label"\]\.string \?\? ""/)
})
