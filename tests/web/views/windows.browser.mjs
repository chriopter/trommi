// Several tabs of one browser profile are windows of one core (one SharedWorker, one device): each on its own page,
// what one does shows in the other, the device held once; the last tab closing frees it, and a reload opens it
// again. Real production bundle, real IndexedDB and Web Locks, against a real hub with a made-up account
// (TROMMI_HUB_BIN; without it the test is skipped).
//   TROMMI_HUB_BIN=target/debug/trommi-hub node tests/web/views/windows.browser.mjs
import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import fs from 'node:fs'
import net from 'node:net'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { launchChromium } from '../../../app/web/dev/cdp.mjs'
import { spawnHub } from '../hub-process.mjs'

const BIN = process.env.TROMMI_HUB_BIN
if (!BIN) { console.log('windows: SKIPPED (TROMMI_HUB_BIN is not set)'); process.exit(0) }
const sleep = ms => new Promise(r => setTimeout(r, ms))
const data = fs.mkdtempSync(path.join(os.tmpdir(), 'trommi-windows-'))
const serverPort = await new Promise(resolve => { const s = net.createServer(); s.listen(0, '127.0.0.1', () => { const port = s.address().port; s.close(() => resolve(port)) }) })
const server = spawn(process.execPath, [fileURLToPath(new URL('../../../app/web/dev/serve.mjs', import.meta.url)), String(serverPort), '--prod'], { stdio: ['ignore', 'pipe', 'inherit'] })
const APP = `http://127.0.0.1:${serverPort}`
const hub = await spawnHub(BIN, { HUB_HOST: '127.0.0.1', HUB_DATA: data, HUB_QUIET: '1', HUB_LOGIN_THROTTLE: 'off', HUB_ORIGINS: APP, PATH: process.env.PATH ?? '' })
const exceptions = []
let browser
/** A tab of the one browser, and what a test asks of it. */
async function tab() {
  const t = await browser.tab()
  const p = t.session
  await p.send('Page.enable'); await p.send('Runtime.enable')
  p.on('Runtime.exceptionThrown', e => exceptions.push(e.exceptionDetails.exception?.description ?? e.exceptionDetails.text))
  p.on('Runtime.consoleAPICalled', e => { if (e.type === 'error') console.error('page console:', e.args.map(a => a.value ?? a.description).join(' ')) })
  const ev = async expression => { const r = await p.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true }); if (r.exceptionDetails) throw Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text); return r.result.value }
  const state = () => ev(`JSON.stringify({ at: location.pathname, phase: window.trommi?.vm?.().phase, text: document.body.innerText.slice(0, 160) })`).catch(e => String(e))
  const until = async (expression, label, tries = 1200) => { for (let n = 0; n < tries; n++) { if (await ev(expression).catch(() => false)) return; await sleep(100) } throw Error(`Timed out: ${label}; ${await state()}`) }
  const open = async url => { await p.send('Page.navigate', { url }); await sleep(200); await until(`document.documentElement.hasAttribute('data-ready')`, `${url} drawn`) }
  const live = to => until(`document.documentElement.hasAttribute('data-ready') && location.pathname === '${to}' && window.trommi.vm().phase === 'live'`, `${to}, live`)
  return { t, p, ev, until, open, live }
}
const FORM = (id, fields) => `(() => { const f = document.getElementById('${id}'); ${Object.entries(fields).map(([k, v]) => `f.querySelector('input[name=${k}]').value = ${JSON.stringify(v)};`).join(' ')} f.querySelector('button[type=submit]').click(); return true })()`
const held = w => w.ev(`navigator.locks.query().then(x => x.held.filter(l => l.name === 'trommi-core:trommi').length)`)
try {
  for (let n = 0; ; n++) { try { if ((await fetch(`${hub.url}/healthz`)).ok) break } catch {} if (n > 300) throw Error('the hub is not up'); await sleep(100) }
  await new Promise((resolve, reject) => { server.stdout.on('data', d => { if (String(d).includes('http://')) resolve() }); server.once('exit', c => reject(Error(`server exited ${c}`))) })
  browser = await launchChromium({ width: 1280, height: 900 })

  // A new account in the first tab, its Desk.
  const a = await tab()
  await a.open(`${APP}/create?way=password&hub=${encodeURIComponent(hub.url)}`)
  await a.until(`!!document.querySelector('#create-form input[name=email]')`, 'the form to create an account')
  await a.ev(FORM('create-form', { email: 'windows@example.invalid', password: 'a long enough password' }))
  await a.until(`location.pathname === '/kit' && !!document.querySelector('#kit-show')`, 'the Emergency Kit')
  await a.ev(`document.querySelector('#kit-show').click(), true`)
  await a.until(`!!document.querySelector('#kit-done:not([disabled])')`, 'the kit counted as kept')
  await a.ev(`document.querySelector('#kit-done').click(), true`)
  await a.until(`location.pathname === '/' && !!document.querySelector('#inbox') && window.trommi.vm().phase === 'live'`, 'the Desk, live')

  // A second tab: the same device at once (no "open in another tab"), on a page of its own.
  const b = await tab()
  // (the hub of this test: a tab keeps the hub it was opened with; a person's tabs all use the one hub)
  await b.open(`${APP}/settings?hub=${encodeURIComponent(hub.url)}`)
  await b.live('/settings')
  assert.equal(await b.ev(`!!document.querySelector('#use-here')`), false, 'no busy screen')
  assert.equal(await held(b), 1, 'one core holds the device for both tabs')
  await sleep(500)
  assert.equal(await a.ev('location.pathname'), '/', 'the first tab stays on its Desk')
  assert.equal(await a.ev(`Object.keys(window.trommi.vm().settings ?? {}).length > 0 && !!document.querySelector('#inbox')`), true, 'the first tab still draws its Desk')

  // What one tab does shows in the other.
  await b.ev(`window.trommi.act({ do: 'add_team', name: 'Second window' }), true`)
  await a.until(`JSON.stringify(window.trommi.vm().sidebar ?? {}).includes('Second window')`, 'the team made in tab two shows in tab one')
  await b.until(`JSON.stringify(window.trommi.vm().sidebar ?? {}).includes('Second window')`, 'and in tab two')
  await a.ev(`window.trommi.act({ do: 'add_team', name: 'First window' }), true`)
  await b.until(`JSON.stringify(window.trommi.vm().sidebar ?? {}).includes('First window')`, 'the team made in tab one shows in tab two')

  // One tab closes: the other goes on; the last one's reload opens the device again.
  await a.t.close()
  await sleep(500)
  await b.ev(`window.trommi.act({ do: 'add_team', name: 'After close' }), true`)
  await b.until(`JSON.stringify(window.trommi.vm().sidebar ?? {}).includes('After close')`, 'the remaining tab still works')
  await b.p.send('Page.reload', { ignoreCache: true }); await sleep(200)
  await b.live('/settings')
  assert.equal(await held(b), 1)
  assert.deepEqual(exceptions, [])
  console.log('windows: two tabs share one core, each on its own page; what one does shows in the other; closing and reloading keep it working')
} finally {
  await browser?.close()
  server.kill()
  hub.child.kill()
  await hub.exited
  fs.rmSync(data, { recursive: true, force: true })
}
