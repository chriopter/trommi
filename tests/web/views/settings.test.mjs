// settings.test.mjs: the parts Settings' pages are built from (ui.mjs settingsPage, setRow, the pen's icons): a page
// under a page has its way back to that page; Devices and Account (auth.mjs) draw their rows with icons that exist.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import { settingsPage, setRow, sketchSvg } from '../../../app/web/public/ui.mjs'

const AUTH = fs.readFileSync(new URL('../../../app/web/public/auth.mjs', import.meta.url), 'utf8')

test('a page under Settings goes back to Settings; a page under a page goes back to that page, by its name', () => {
  assert.match(String(settingsPage('Theme', '')), /<a class="set-back" href="\/settings" data-nav>[\s\S]*<span>Settings<\/span>/)
  const sub = String(settingsPage('Password', '', { back: '/settings/account', backWord: 'Account' }))
  assert.match(sub, /<a class="set-back" href="\/settings\/account" data-nav>[\s\S]*<span>Account<\/span>/)
  assert.match(String(settingsPage('Settings', '', { back: false })), /^(?![\s\S]*set-back)/)
})

test('every pen icon Settings asks for is drawn (none falls back to an empty picture)', () => {
  const names = [...new Set([...AUTH.matchAll(/\bsk\('([a-z-]+)'/g)].map(m => m[1]))]
  assert.ok(names.includes('laptop') && names.includes('terminal') && names.includes('lock') && names.includes('key'), names.join())
  for (const name of names) assert.match(sketchSvg(name), /<path /, `sk('${name}') draws something`)
})

test('a way in of the account is a row with its own page', () => {
  const row = String(setRow({ href: '/settings/account/password', icon: '', word: 'Password', detail: 'set', id: 'settings-way-password' }))
  assert.match(row, /href="\/settings\/account\/password"[^>]*id="settings-way-password"/)
  for (const way of ['password', 'passkeys', 'kit', 'email']) assert.ok(AUTH.includes(`${way}: '`), `the page ${way} is one of Account's`)
})
