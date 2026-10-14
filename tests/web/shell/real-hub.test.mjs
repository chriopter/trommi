// app/web/core/shell.ts against the REAL hub's binary, with the app core's real binding (core/bindings/wasm/pkg):
// an account is made with its room through the core, the view folds to the Desk, live; a second slot signs in with
// the password and reaches the same room; a line written in a session reaches the hub.
//
//   TROMMI_HUB_BIN=/path/to/trommi-hub node --test tests/web/shell/real-hub.test.mjs
// Without TROMMI_HUB_BIN, or without the binding built (core/bindings/wasm/build.sh), everything here is SKIPPED.
import test from 'node:test'
import assert from 'node:assert/strict'
import { mkdtemp, rm, readFile } from 'node:fs/promises'
import { existsSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { spawnHub } from '../hub-process.mjs'
import { CoreShell, memorySpaces } from '../../../app/web/core/shell.ts'
import { foldView, emptyView } from '../../../core/bindings/wasm/js/views-fold.js'

const BIN = process.env.TROMMI_HUB_BIN
const PKG = new URL('../../../core/bindings/wasm/pkg/', import.meta.url)
const skip = !BIN ? 'TROMMI_HUB_BIN is not set' : !existsSync(new URL('trommi_core_wasm_bg.wasm', PKG)) ? 'the binding is not built' : false
if (skip) console.warn(`\nSKIPPED: the web shell against the real hub did not run: ${skip}.\n`)

const PASSWORD = 'correct horse battery staple 42'

async function realHub() {
  const data = await mkdtemp(join(tmpdir(), 'trommi-shell-test-'))
  const env = { HUB_HOST: '127.0.0.1', HUB_DATA: data, HUB_QUIET: '1', HUB_LOGIN_THROTTLE: 'off', PATH: process.env.PATH ?? '' }
  const { child, url } = await spawnHub(BIN, env)
  for (let i = 0; !(await fetch(`${url}/healthz`).then(r => r.ok, () => false)); i++) {
    if (i > 200) throw new Error('the hub did not start')
    await new Promise(r => setTimeout(r, 50))
  }
  return { url, stop: async () => { child.kill(); await rm(data, { recursive: true, force: true }) } }
}

async function binding() {
  const b = await import(new URL('trommi-core.js', PKG).href)
  await b.init(await readFile(new URL('trommi_core_wasm_bg.wasm', PKG)))
  return b
}

/** One app: a core, its shell over memory spaces, the view it folds. */
function app(b, hub) {
  let view = emptyView()
  const waiters = []
  const shell = new CoreShell({
    core: new b.AppCore(), ...memorySpaces(),
    host: {
      render: delta => { view = foldView(view, delta); for (const w of [...waiters]) if (w.want(view)) { waiters.splice(waiters.indexOf(w), 1); w.done(view) } },
      log: (level, text) => { if (level === 'error') console.error('core:', text) },
    },
  })
  const until = (what, want, ms = 60_000) => want(view) ? Promise.resolve(view) : new Promise((done, fail) => {
    const t = setTimeout(() => fail(new Error(`no view with ${what}; phase ${view.phase}, account ${JSON.stringify(view.account)}`)), ms)
    waiters.push({ want, done: v => { clearTimeout(t); done(v) } })
  })
  shell.send({ type: 'start', config: { platform: 'web', version: '2.0.0', hub, seed: Math.floor(Math.random() * 2 ** 40), utc_offset_min: 0 } })
  return { shell, until, view: () => view, act: action => shell.send({ type: 'action', action }) }
}

test('an account is made through the core, and a second device signs in to it', { skip, timeout: 180_000 }, async () => {
  const b = await binding()
  const hub = await realHub()
  try {
    const one = app(b, hub.url)
    await one.until('the sign-in', v => v.phase === 'signed_out')
    one.act({ do: 'account', action: { step: 'found', email: 'ada@example.org', password: PASSWORD } })
    const kept = await one.until('the kit', v => v.account?.step === 'keep_kit')
    assert.equal(kept.account.kit.length, 12)
    await one.until('live', v => v.phase === 'live')
    one.act({ do: 'account', action: { step: 'kit_kept' } })
    await one.until('the kit put away', v => v.account?.step === 'none')

    const two = app(b, hub.url)
    await two.until('the sign-in', v => v.phase === 'signed_out')
    two.act({ do: 'account', action: { step: 'sign_in', email: 'ada@example.org', password: PASSWORD } })
    await two.until('live after the sign-in', v => v.phase === 'live')
    assert.equal(two.view().account?.problem ?? null, null)

    // the account deleted from the second device: a wrong password is said, the right one deletes it and signs out
    two.act({ do: 'account', action: { step: 'delete_account', password: 'not the password at all' } })
    await two.until('the wrong password said', v => v.account?.problem?.key === 'account.wrong_login')
    two.act({ do: 'account', action: { step: 'delete_account', password: PASSWORD } })
    await two.until('signed out after the deletion', v => v.phase === 'signed_out' && v.account?.step === 'found')
    const three = app(b, hub.url)
    await three.until('the sign-in', v => v.phase === 'signed_out')
    three.act({ do: 'account', action: { step: 'sign_in', email: 'ada@example.org', password: PASSWORD } })
    await three.until('no such account', v => v.account?.problem?.key === 'account.wrong_login')
    three.shell.close()
    one.shell.close()
    two.shell.close()
  } finally { await hub.stop() }
})
