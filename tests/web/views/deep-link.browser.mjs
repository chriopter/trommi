// A signed-in browser opens the page in its address bar: a reload, or a link typed or followed into a fresh page
// load (/chat/<id>, /settings/account), lands on that page and never on the start screen. Real production bundle,
// real IndexedDB and Web Locks, against a real hub (TROMMI_HUB_BIN; without it the test is skipped).
//   TROMMI_HUB_BIN=target/debug/trommi-hub node tests/web/views/deep-link.browser.mjs
import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import fs from 'node:fs'
import net from 'node:net'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { launchChromium } from '../../../app/web/dev/cdp.mjs'
import { spawnHub } from '../hub-process.mjs'

/** A DevTools session on one target by its socket: send(method, params, sessionId) resolves with the result;
 *  attached(): the sessions of the targets it attached to (flattened). */
async function devtools(url) {
  const ws = new WebSocket(url)
  await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = () => reject(Error(`DevTools socket ${url} did not open`)) })
  let seq = 0
  const pending = new Map(), sessions = [], listeners = []
  ws.onmessage = e => {
    const m = JSON.parse(e.data)
    if (m.method === 'Target.attachedToTarget') sessions.push(m.params)
    if (m.method) for (const fn of listeners) fn(m)
    const call = pending.get(m.id); if (!call) return; pending.delete(m.id); if (m.error) call[1](Error(m.error.message)); else call[0](m.result)
  }
  const send = (method, params = {}, sessionId) => new Promise((resolve, reject) => { const id = ++seq; pending.set(id, [resolve, reject]); ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) })) })
  return { send, attached: () => sessions, on: fn => listeners.push(fn), close: () => ws.close() }
}
const BIN = process.env.TROMMI_HUB_BIN
if (!BIN) { console.log('deep-link: SKIPPED (TROMMI_HUB_BIN is not set)'); process.exit(0) }
const sleep = ms => new Promise(r => setTimeout(r, ms))
const data = fs.mkdtempSync(path.join(os.tmpdir(), 'trommi-deep-link-'))
const serverPort = await new Promise(resolve => { const s = net.createServer(); s.listen(0, '127.0.0.1', () => { const port = s.address().port; s.close(() => resolve(port)) }) })
const server = spawn(process.execPath, [fileURLToPath(new URL('../../../app/web/dev/serve.mjs', import.meta.url)), String(serverPort), '--prod'], { stdio: ['ignore', 'pipe', 'inherit'] })
const APP = `http://127.0.0.1:${serverPort}`
const hub = await spawnHub(BIN, { HUB_HOST: '127.0.0.1', HUB_DATA: data, HUB_QUIET: '1', HUB_LOGIN_THROTTLE: 'off', HUB_ORIGINS: APP, PATH: process.env.PATH ?? '' })
let browser
/** A tab of a fresh browser (its own profile: another device), and what a test asks of it. */
async function device() {
  await browser?.close()
  browser = await launchChromium({ width: 1280, height: 900 })
  const { session: p } = await browser.tab()
  await p.send('Page.enable'); await p.send('Runtime.enable')
  p.on('Runtime.exceptionThrown', e => exceptions.push(e.exceptionDetails.exception?.description ?? e.exceptionDetails.text))
  p.on('Runtime.consoleAPICalled', e => { if (e.type === 'error') console.error('page console:', e.args.map(a => a.value ?? a.description).join(' ')) })
  const ev = async expression => { const r = await p.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true }); if (r.exceptionDetails) throw Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text); return r.result.value }
  const state = () => ev(`JSON.stringify({ at: location.pathname, phase: window.trommi?.vm?.().phase, step: window.trommi?.vm?.().account?.step, text: document.body.innerText.slice(0, 160) })`).catch(e => String(e))
  const until = async (expression, label, tries = 1200) => { for (let n = 0; n < tries; n++) { if (await ev(expression).catch(() => false)) return; await sleep(100) } throw Error(`Timed out: ${label}; ${await state()}`) }
  const open = async url => { await p.send('Page.navigate', { url }); await sleep(200); await until(`document.documentElement.hasAttribute('data-ready')`, `${url} drawn`) }
  /** A fresh load of `to` (typed into the address bar, a link followed, a reload) lands on `to`, signed in. */
  const lands = async to => {
    await open(APP + to)
    await until(`window.trommi.vm().phase && window.trommi.vm().phase !== 'starting'`, `${to}: the device read`)
    await sleep(500)
    const s = JSON.parse(await state())
    assert.equal(s.at, to, `a fresh load of ${to} stays there (${JSON.stringify(s)})`)
    assert.notEqual(s.phase, 'signed_out', `${to}: still signed in (${JSON.stringify(s)})`)
  }
  return { p, ev, until, open, lands }
}
const exceptions = []
const FORM = (id, fields) => `(() => { const f = document.getElementById('${id}'); ${Object.entries(fields).map(([k, v]) => `f.querySelector('input[name=${k}]').value = ${JSON.stringify(v)};`).join(' ')} f.querySelector('button[type=submit]').click(); return true })()`
const EMAIL = 'deep-link@example.invalid', PASSWORD = 'a long enough password'
try {
  for (let n = 0; ; n++) { try { if ((await fetch(`${hub.url}/healthz`)).ok) break } catch {} if (n > 300) throw Error('the hub is not up'); await sleep(100) }
  await new Promise((resolve, reject) => { server.stdout.on('data', d => { if (String(d).includes('http://')) resolve() }); server.once('exit', c => reject(Error(`server exited ${c}`))) })

  // A new account with a password, its Emergency Kit kept, its Desk; and a team (a room register from before the
  // second device joins).
  let d = await device()
  await d.open(`${APP}/create?way=password&hub=${encodeURIComponent(hub.url)}`)
  await d.until(`!!document.querySelector('#create-form input[name=email]')`, 'the form to create an account')
  await d.ev(FORM('create-form', { email: EMAIL, password: PASSWORD }))
  await d.until(`location.pathname === '/kit' && !!document.querySelector('#kit-show')`, 'the Emergency Kit')
  await d.ev(`document.querySelector('#kit-show').click(), true`)
  await d.until(`!!document.querySelector('#kit-done:not([disabled])')`, 'the kit counted as kept')
  await d.ev(`document.querySelector('#kit-done').click(), true`)
  await d.until(`location.pathname === '/' && !!document.querySelector('#inbox') && window.trommi.vm().phase === 'live'`, 'the Desk, live')
  await d.ev(`window.trommi.act({ do: 'add_team', name: 'Work' }), true`)
  await d.until(`JSON.stringify(window.trommi.vm().sidebar ?? {}).includes('Work')`, 'the team written')
  await sleep(1500)
  for (const to of ['/settings/account', '/chat/eb86630b214a', '/', '/settings/account']) await d.lands(to)
  await d.p.send('Page.reload', { ignoreCache: true }); await sleep(200)
  await d.until(`document.documentElement.hasAttribute('data-ready') && window.trommi.vm().phase === 'live'`, 'reloaded')
  assert.equal(await d.ev('location.pathname'), '/settings/account', 'a reload keeps the page')

  // The same account on a second browser, logged in with its password (the room joined with the code it opens),
  // its hub slow; the page is loaded again the moment the room is joined, before the sign-in is through.
  d = await device()
  await d.open(`${APP}/login?hub=${encodeURIComponent(hub.url)}`)
  const target = (await (await fetch(`http://127.0.0.1:${browser.port}/json`)).json()).find(t => t.type === 'page' && t.url.includes('/login'))
  const slow = await devtools(target.webSocketDebuggerUrl)
  slow.on(m => { if (m.method === 'Fetch.requestPaused') setTimeout(() => slow.send('Fetch.continueRequest', { requestId: m.params.requestId }, m.sessionId).catch(() => {}), 150) })
  await slow.send('Target.setAutoAttach', { autoAttach: true, waitForDebuggerOnStart: false, flatten: true })
  await slow.send('Fetch.enable', { patterns: [{ urlPattern: `${hub.url}/*` }] })
  await d.until(`!!document.querySelector('#login-form input[name=account]')`, 'the form to log in')
  await d.ev(FORM('login-form', { account: EMAIL, password: PASSWORD }))
  for (let n = 0; n < 6000; n++) { if (await d.ev(`window.trommi.vm().account?.step === 'none' && window.trommi.vm().phase !== 'signed_out'`).catch(() => false)) break; await sleep(5) }
  for (const to of ['/chat/eb86630b214a', '/settings/account']) await d.lands(to)
  // (a sign-in that was cut off asks again on the side, and the Log in finishes it)
  if (await d.ev(`Boolean(window.trommi.vm().account?.resume)`)) {
    await d.until(`!!document.querySelector('#resume-line a[href="/login"]')`, 'the line that asks to log in again')
    await d.ev(`document.querySelector('#resume-line a').click(), true`)
    await d.until(`location.pathname === '/login' && !!document.querySelector('#login-form')`, 'the log in, signed in')
    await d.ev(FORM('login-form', { account: EMAIL, password: PASSWORD }))
    await d.until(`!window.trommi.vm().account?.resume && location.pathname === '/'`, 'the sign-in finished')
    console.log('deep-link: the cut-off sign-in was finished from its line')
  }
  slow.close()
  assert.deepEqual(exceptions, [])
  console.log('deep-link: a reload and fresh loads of deep links stay signed in and on their page, on both devices')
} finally {
  await browser?.close()
  server.kill()
  hub.child.kill()
  await hub.exited
  fs.rmSync(data, { recursive: true, force: true })
}
