// Real production bundle, actual IndexedDB/Web Locks, no hub or account. Styles are checked as painted.
import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import net from 'node:net'
import { fileURLToPath } from 'node:url'
import { launchChromium } from '../../../app/web/dev/cdp.mjs'
const sleep = ms => new Promise(r => setTimeout(r, ms))
const serverPort = await new Promise(resolve => { const s = net.createServer(); s.listen(0, '127.0.0.1', () => { const port = s.address().port; s.close(() => resolve(port)) }) })
const server = spawn(process.execPath, [fileURLToPath(new URL('../../../app/web/dev/serve.mjs', import.meta.url)), String(serverPort), '--prod'], { stdio: ['ignore', 'pipe', 'inherit'] })
let browser
const APP = `http://127.0.0.1:${serverPort}`
try {
  await new Promise((resolve, reject) => { server.stdout.on('data', d => { if (String(d).includes('http://')) resolve() }); server.once('exit', c => reject(Error(`server exited ${c}`))) })
  browser = await launchChromium({ width: 1280, height: 900 })
  const exceptions = []
  const ev = async (p, expression) => { const r = await p.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true }); if (r.exceptionDetails) throw Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text); return r.result.value }
  const until = async (p, expression, label) => { for (let n = 0; n < 300; n++) { if (await ev(p, expression).catch(() => false)) return; await sleep(100) } throw Error(`Timed out: ${label}; ${await ev(p, 'JSON.stringify({text:document.body.innerText,vm:window.trommi?.vm(),early:!!globalThis.__trommiCore})')}`) }
  const prepare = async p => { await p.send('Runtime.enable'); p.on('Runtime.consoleAPICalled', e => { if(e.type === 'error') console.error('page console:', e.args.map(a=>a.value ?? a.description).join(' ')) }); p.on('Runtime.exceptionThrown', e => exceptions.push(e.exceptionDetails.exception?.description ?? e.exceptionDetails.text)); await p.send('Network.enable'); await p.send('Network.setBlockedURLs', { urls: ['*://hub.trommi.com/*', '*://127.0.0.1:8890/*'] }) }
  const ready = p => until(p, `document.documentElement.hasAttribute('data-ready') && !document.querySelector('#use-here') && document.body.innerText.includes('Create account')`, 'single usable account screen')
  const open = async p => { await prepare(p); await p.send('Page.navigate', { url: APP }); await ready(p) }
  let first = await browser.tab()
  // A slow worker must keep the styled loading page, then paint the first actual view; never race a 3 s timer.
  await first.session.send('Page.addScriptToEvaluateOnNewDocument', { source: `globalThis.Worker = class extends Worker { postMessage(message, transfer) { if(message?.t === 'start') setTimeout(() => super.postMessage(message, transfer), 4100); else super.postMessage(message, transfer) } }` })
  await open(first.session)
  for (let n = 0; n < 2; n++) { await first.session.send('Page.reload', { ignoreCache: true }); await sleep(100); await ready(first.session) }
  await first.close(); first = await browser.tab(); await open(first.session)
  const second = await browser.tab(); await prepare(second.session); await second.session.send('Page.navigate', { url: APP })
  await until(second.session, `!!document.querySelector('#use-here')`, 'a genuinely live other tab')
  await sleep(3300) // Startup must never repaint the stopped core's empty model over this page.
  const styles = await ev(second.session, `(async () => { await document.fonts.ready; const h=document.querySelector('.room-title'), b=document.querySelector('#use-here'), main=document.querySelector('main');return {font:getComputedStyle(h).fontFamily,loaded:document.fonts.check('800 42px "Bricolage Grotesque"'),width:main.getBoundingClientRect().width,viewport:innerWidth,x:document.querySelector('.room-card').getBoundingClientRect().x,buttonPadding:parseFloat(getComputedStyle(b).paddingTop),radius:parseFloat(getComputedStyle(b).borderRadius),brand:!!document.querySelector('.brand-mark')} })()`)
  assert.match(styles.font, /Bricolage Grotesque/); assert.equal(styles.loaded, true)
  assert.equal(styles.width, 1280); assert.ok(styles.x > 200); assert.ok(styles.buttonPadding >= 14); assert.ok(styles.radius >= 20); assert.equal(styles.brand, true)
  await ev(second.session, `document.querySelector('#use-here').click()`); await sleep(100); await ready(second.session)
  await until(first.session, `document.body.innerText.includes('Trommi moved to another tab')`, 'old owner stopped')
  await first.close(); await second.close()
  // A stranded lock with no live Trommi page must not lock a person out. The revoked holder cannot write.
  const ghost = await browser.tab(); await prepare(ghost.session); await ghost.session.send('Page.navigate', { url: APP + '/help.html' })
  await until(ghost.session, `typeof navigator.locks !== 'undefined' && location.origin === '${APP}'`, 'ghost origin')
  await ev(ghost.session, `navigator.locks.request('trommi-core:trommi', () => { globalThis.ghostHeld = true; return new Promise(() => {}) }).catch(() => {}); true`)
  await until(ghost.session, 'globalThis.ghostHeld === true', 'stranded lock held')
  const reopened = await browser.tab(); await open(reopened.session)
  const held = await ev(reopened.session, `navigator.locks.query().then(x=>x.held.filter(l=>l.name==='trommi-core:trommi').length)`)
  assert.equal(held, 1)
  await reopened.close(); await ghost.close()
  assert.deepEqual(exceptions, [], 'no startup or ownership exceptions')
  console.log('ownership: reload twice, close/reopen, live-owner check, styled busy page, working takeover and stranded-lock recovery passed')
} finally { await browser?.close(); server.kill(); await new Promise(resolve => { if (server.exitCode !== null) resolve(); else server.once('exit', resolve) }) }
