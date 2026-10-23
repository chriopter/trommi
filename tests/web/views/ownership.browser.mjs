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
  const prepare = async p => { await p.send('Page.enable'); await p.send('Runtime.enable'); p.on('Runtime.consoleAPICalled', e => { if(e.type === 'error') console.error('page console:', e.args.map(a=>a.value ?? a.description).join(' ')) }); p.on('Runtime.exceptionThrown', e => exceptions.push(e.exceptionDetails.exception?.description ?? e.exceptionDetails.text)); await p.send('Network.enable'); await p.send('Network.setBlockedURLs', { urls: ['*://hub.trommi.com/*', '*://127.0.0.1:8890/*'] }) }
  const ready = p => until(p, `document.documentElement.hasAttribute('data-ready') && !document.querySelector('#use-here, #trommi-opening') && document.body.innerText.includes('Create account')`, 'single usable account screen')
  const open = async p => { await prepare(p); await p.send('Page.navigate', { url: APP }); await ready(p) }
  let first = await browser.tab()
  // A slow worker must keep the styled loading page, then paint the first actual view; never race a 3 s timer.
  const delayed = await first.session.send('Page.addScriptToEvaluateOnNewDocument', { source: `globalThis.trommiTestDelayedWorker = true; { const hold = send => function (message, transfer) { if (message?.t === 'start') globalThis.trommiTestReleaseWorker = () => send.call(this, message, transfer); else send.call(this, message, transfer) }; Worker.prototype.postMessage = hold(Worker.prototype.postMessage); MessagePort.prototype.postMessage = hold(MessagePort.prototype.postMessage) }` })
  await prepare(first.session); await first.session.send('Page.navigate', { url: APP })
  await until(first.session, `document.readyState === 'complete' && typeof globalThis.trommiTestReleaseWorker === 'function' && !!document.getElementById('trommi-opening')`, 'styled loading screen before the delayed worker starts')
  // Loading assets may take longer than a fixed worker delay on a busy machine.
  // Hold start until the page was inspected, beyond the former 3 s fallback timer.
  await sleep(3300)
  const loading = await ev(first.session, `({width:document.getElementById('trommi-opening').getBoundingClientRect().width,font:getComputedStyle(document.querySelector('#trommi-opening h1')).fontFamily})`)
  assert.equal(loading.width, 1280); assert.match(loading.font, /Bricolage Grotesque/)
  await ev(first.session, 'globalThis.trommiTestReleaseWorker(); true')
  await first.session.send('Page.removeScriptToEvaluateOnNewDocument', { identifier: delayed.identifier })
  await ready(first.session)
  assert.equal(await ev(first.session, 'globalThis.trommiTestDelayedWorker'), true, 'the slow-worker injection ran')
  for (let n = 0; n < 2; n++) { await first.session.send('Page.reload', { ignoreCache: true }); await sleep(100); await ready(first.session) }
  await first.close(); first = await browser.tab(); await open(first.session)
  // A second tab is a second window of the same core (SharedWorker): usable at once, the device held once.
  const second = await browser.tab(); await open(second.session)
  assert.equal(await ev(second.session, `navigator.locks.query().then(x=>x.held.filter(l=>l.name==='trommi-core:trommi').length)`), 1, 'one core holds the device for both tabs')
  await sleep(1000); await ready(first.session)
  // A tab whose browser has no SharedWorker (Chrome on Android) runs its own core: one tab at a time, as before.
  const third = await browser.tab(); await prepare(third.session)
  await third.session.send('Page.addScriptToEvaluateOnNewDocument', { source: 'delete globalThis.SharedWorker' })
  await third.session.send('Page.navigate', { url: APP })
  await until(third.session, `!!document.querySelector('#use-here')`, 'a genuinely live other tab')
  await sleep(3300) // Startup must never repaint the stopped core's empty model over this page.
  const styles = await ev(third.session, `(async () => { await document.fonts.ready; const h=document.querySelector('.room-title'), b=document.querySelector('#use-here'), main=document.querySelector('main');return {font:getComputedStyle(h).fontFamily,loaded:document.fonts.check('800 42px "Bricolage Grotesque"'),width:main.getBoundingClientRect().width,viewport:innerWidth,x:document.querySelector('.room-card').getBoundingClientRect().x,buttonPadding:parseFloat(getComputedStyle(b).paddingTop),radius:parseFloat(getComputedStyle(b).borderRadius),brand:!!document.querySelector('.brand-mark')} })()`)
  assert.match(styles.font, /Bricolage Grotesque/); assert.equal(styles.loaded, true)
  assert.equal(styles.width, 1280); assert.ok(styles.x > 200); assert.ok(styles.buttonPadding >= 14); assert.ok(styles.radius >= 20); assert.equal(styles.brand, true)
  await ev(third.session, `document.querySelector('#use-here').click()`); await sleep(100); await ready(third.session)
  await until(first.session, `document.body.innerText.includes('Trommi moved to another tab')`, 'old owner stopped')
  await until(second.session, `document.body.innerText.includes('Trommi moved to another tab')`, 'its other window stopped too')
  await first.close(); await second.close(); await third.close()
  // A stranded lock with no live Trommi page must not lock a person out. The revoked holder cannot write.
  const ghost = await browser.tab(); await prepare(ghost.session); await ghost.session.send('Page.navigate', { url: APP + '/help.html' })
  await until(ghost.session, `typeof navigator.locks !== 'undefined' && location.origin === '${APP}'`, 'ghost origin')
  await ev(ghost.session, `navigator.locks.request('trommi-core:trommi', () => { globalThis.ghostHeld = true; return new Promise(() => {}) }).catch(() => {}); true`)
  await until(ghost.session, 'globalThis.ghostHeld === true', 'stranded lock held')
  const reopened = await browser.tab(); await open(reopened.session)
  const held = await ev(reopened.session, `navigator.locks.query().then(x=>x.held.filter(l=>l.name==='trommi-core:trommi').length)`)
  assert.equal(held, 1)
  await reopened.close(); await ghost.close()
  const demo = await browser.tab(); await prepare(demo.session)
  await demo.session.send('Page.addScriptToEvaluateOnNewDocument', { source:`(() => {
    const Native = IntersectionObserver
    globalThis.deskRefs = []
    // (one team of the demo room is about as long as the Desk's window: draw only the first four rows at once)
    globalThis.TROMMI_DESK_WINDOW = 4
    globalThis.IntersectionObserver = class extends Native {
      observe(el) { if (el.matches('.inbox-row[data-later]')) deskRefs.push(new WeakRef(el)); super.observe(el) }
    }
  })()` })
  await demo.session.send('Page.navigate', { url: APP + '/?mock=1' })
  await until(demo.session, `!!document.querySelector('#desk-list .inbox-fan img[src^="blob:"]') && document.querySelector('#desk-list .inbox-fan img').naturalWidth > 0`, 'first Desk thumbnails load before visiting a card')
  const rowRefs = await ev(demo.session, `deskRefs.length`)
  assert.ok(rowRefs > 8, 'the actual Desk must have observed offscreen placeholders')
  await ev(demo.session, `window.trommi.router.visit('/card/'+window.trommi.vm().desk.rows[0].id).then(()=>true)`)
  await sleep(500)
  await demo.session.send('HeapProfiler.collectGarbage')
  const retainedRows = await ev(demo.session, `deskRefs.filter(r=>{const n=r.deref();return n && !n.isConnected}).length`)
  console.log(`Desk observer memory: ${retainedRows} detached placeholders retained out of ${rowRefs} observations`)
  assert.equal(retainedRows, 0, 'leaving the actual Desk must release its offscreen observed nodes')
  const changedRows = await ev(demo.session, `(() => {
    const router = window.trommi.router
    const opts = main => ({ title:'List replacement fixture', view:'settings', sidebar:false, main })
    router.paint('/fixture', opts('<main><div id="changing-list"><div id="row-a">old</div></div></main>'))
    const old = document.getElementById('row-a')
    router.paint('/fixture', opts('<main><div id="changing-list"><section id="row-a">new</section><div id="row-b">second</div></div></main>'), { morph:true })
    return { ids:[...document.getElementById('changing-list').children].map(n=>n.id), tag:document.getElementById('row-a').tagName, oldConnected:old.isConnected }
  })()`)
  assert.deepEqual(changedRows, { ids:['row-a','row-b'], tag:'SECTION', oldConnected:false }, 'replacing the first keyed row while the list grows must keep a valid insertion cursor')
  await demo.close()
  assert.deepEqual(exceptions, [], 'no startup or ownership exceptions')
  console.log('ownership: reload twice, close/reopen, two tabs on one core, a tab without SharedWorker, styled busy page, working takeover and stranded-lock recovery passed')
} finally { await browser?.close(); server.kill(); await new Promise(resolve => { if (server.exitCode !== null) resolve(); else server.once('exit', resolve) }) }
