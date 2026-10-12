// invite.test.mjs: what an agent invite asks the human to paste (ui.mjs connectPrompt, connectCommand; auth.mjs's
// clipboard): first a plain prompt with the link, which works without the plugin's slash command, then the command.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import { connectCommand, connectPrompt } from '../../../app/web/public/ui.mjs'

const AUTH = fs.readFileSync(new URL('../../../app/web/public/auth.mjs', import.meta.url), 'utf8')
const LINK = 'https://app.trommi.com/join#v1.a.b.c.d'

test('the prompt is plain words with the link in quotes; the slash command carries the same link', () => {
  assert.equal(connectPrompt(LINK), `Connect to Trommi with this invite link: '${LINK}'`)
  assert.ok(!connectPrompt(LINK).startsWith('/'), 'no slash command: it works in any agent with the connector')
  assert.equal(connectCommand(LINK), `/trommi:connect '${LINK}'`)
  // the end-to-end runs take the link out of what the page shows by its quotes
  assert.equal(/'(http\S+\/join#v1\.[^']+)'/.exec(connectPrompt(LINK))?.[1], LINK)
})

test('the invite page shows the prompt as the big line and the slash command as the other way', () => {
  const step = AUTH.slice(AUTH.indexOf('In your project folder, start claude'))
  const big = step.indexOf("copyLine(connectPrompt(inv.link), 'Copy', 'connect'"), alt = step.indexOf('altLine(connectCommand(inv.link))')
  assert.ok(big > 0, 'the prompt is the connect line')
  assert.ok(alt > big, 'the slash command comes after it')
  assert.ok(!AUTH.includes("copyLine(connectCommand("), 'the slash command is no big line of its own')
})
