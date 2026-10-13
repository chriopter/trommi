// link-hint.test.mjs: what the board says about a Claude Code session that hears the board only on its next step
// (app.mjs linkOf): no restart and no flag. Under the plugin, board events reach it through the Trommi monitor, and
// the old hint named a server (server:trommi) that does not exist there. And trommi.com shows both ways to connect
// that the invite page shows: the plain prompt and the slash command.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import { ASLEEP_MS, linkOf } from '../../../app/web/public/app.mjs'
import { connectCommand, connectPrompt } from '../../../app/web/public/ui.mjs'

const now = 1_800_000_000_000
// (the link's state as the client core hands it in the session list: client/src/status.rs)
const session = (idle, client = '') => ({ name: 'web', online: true, client, link: { hears: 'oncall', attached: true, last_call_at: now - idle, since: now - idle },
  link_view: { state: idle >= ASLEEP_MS && client !== 'codex' ? 'asleep' : 'oncall', since: now - idle, reason: '' } })

test('a session that hears on its next step: no restart, no flag', () => {
  const soon = linkOf(session(60_000), now), asleep = linkOf(session(ASLEEP_MS + 60_000), now)
  assert.equal(soon.state, 'oncall')
  assert.equal(soon.fix, null, 'nothing to do while it works')
  assert.equal(asleep.state, 'asleep')
  assert.equal(asleep.fix.code, '', 'only: type anything in its terminal')
  assert.match(asleep.fix.say, /type anything in its terminal/)
  for (const l of [soon, asleep]) assert.doesNotMatch(JSON.stringify(l), /dangerously|development-channels|server:trommi|--resume/)
})

test('no page of the app or the site asks for the development-channels flag', () => {
  for (const f of ['app/web/public/app.mjs', 'app/web/public/ui.mjs', 'site/public/index.html']) {
    assert.doesNotMatch(fs.readFileSync(new URL(`../../../${f}`, import.meta.url), 'utf8'), /dangerously-load-development-channels/, f)
  }
})

test('trommi.com shows the plain prompt and the slash command, as the invite page does', () => {
  const site = fs.readFileSync(new URL('../../../site/public/index.html', import.meta.url), 'utf8').replace(/<[^>]+>/g, '')
  const LINK = '\u0000'
  const [before, after] = connectPrompt(LINK).split(LINK)
  assert.ok(site.includes(`${before}the invite link${after}`), 'the plain prompt')
  const [cb, ca] = connectCommand(LINK).split(LINK)
  assert.ok(site.includes(`${cb}the invite link${ca}`), 'the slash command')
})
