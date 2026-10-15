// overflow.browser.mjs: no screen of the demo is wider than a small phone. Every state of demo/data/screens.json
// (the list /screens shows) is opened in headless Chromium as a 360 px wide phone (touch, mobile viewport, the
// insets of a phone without a notch) and checked: the page does not scroll sideways, and no text, button, field or
// picture reaches past the screen's right or left edge unless an ancestor clips or scrolls it (a strip of thumbnails
// that scrolls sideways is fine; a card cut off at the edge is not).
//   node tests/web/views/overflow.browser.mjs      (needs Chromium; serves the app from its sources itself)
//   TROMMI_OVERFLOW_ONLY=<id,id> checks only these states.
import fs from 'node:fs'
import net from 'node:net'
import path from 'node:path'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { launchChromium } from '../../../app/web/dev/cdp.mjs'

const here = path.dirname(fileURLToPath(import.meta.url))
const repo = path.join(here, '..', '..', '..')
const sleep = ms => new Promise(r => setTimeout(r, ms))
const WIDTH = 360, HEIGHT = 740
const UA = 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1'

const only = (process.env.TROMMI_OVERFLOW_ONLY ?? '').split(',').filter(Boolean)
const states = JSON.parse(fs.readFileSync(path.join(repo, 'demo', 'data', 'screens.json'), 'utf8')).states
  .filter(s => s.web && (!only.length || only.includes(s.id)))
/** The address of a state, as /screens opens it. */
const urlOf = ({ web }) => `${web.path}${web.path.includes('?') ? '&' : '?'}mock=${encodeURIComponent(web.mock ?? '1')}${web.state ? `&state=${encodeURIComponent(web.state)}` : ''}`

/** What reaches past the screen's edges, in the page (run there). */
const PROBE = `(() => {
  const vw = innerWidth, out = { page: Math.max(document.scrollingElement.scrollWidth, document.body.scrollWidth) - vw, past: [] }
  const shown = el => { for (let e = el; e; e = e.parentElement) { const c = getComputedStyle(e); if (c.display === 'none' || c.visibility === 'hidden' || +c.opacity < .05) return false } return true }
  // (an ancestor that clips or scrolls sideways keeps what is in it: a strip of thumbnails, a cut title. Not one as wide
  //  as the screen: that is the page's own scroller, and what it cuts off at the screen's edge is lost)
  const held = el => { for (let e = el.parentElement; e && e !== document.documentElement; e = e.parentElement) { const c = getComputedStyle(e); if (c.overflowX !== 'visible' || c.contain.includes('paint')) { const r = e.getBoundingClientRect(); if (r.left <= 1 && r.right >= vw - 1) continue; return r.left >= -1 && r.right <= vw + 1 } if (c.position === 'fixed') break } return false }
  const own = el => [...el.childNodes].some(n => n.nodeType === 3 && n.textContent.trim())
  for (const el of document.querySelectorAll('body *')) {
    if (!(own(el) || el.matches('button, a[href], input, textarea, select, img, video'))) continue
    const r = el.getBoundingClientRect()
    if (r.width < 1 || r.height < 1 || r.bottom <= 0 || r.top >= innerHeight) continue
    if (r.right <= vw + 1 && r.left >= -1) continue
    const c = getComputedStyle(el)
    if (c.position === 'fixed' && (r.left >= vw || r.right <= 0)) continue   // (a drawer parked off the screen)
    if (!shown(el) || held(el) || el.closest('[inert], [aria-hidden="true"], .curl-back')) continue
    const fold = el.closest('details:not([open])')   // (the inside of a closed details is not drawn)
    if (fold && !fold.querySelector(':scope > summary')?.contains(el)) continue
    out.past.push({ el: el.tagName.toLowerCase() + (el.id ? '#' + el.id : '') + [...el.classList].slice(0, 2).map(c => '.' + c).join(''), text: (el.innerText || el.getAttribute('aria-label') || '').trim().replace(/\\s+/g, ' ').slice(0, 40), left: Math.round(r.left), right: Math.round(r.right) })
  }
  out.past = out.past.slice(0, 8)
  return out
})()`

async function freePort() {
  return new Promise((resolve, reject) => { const s = net.createServer(); s.unref(); s.on('error', reject); s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => resolve(port)) }) })
}

const port = await freePort()
const server = spawn(process.execPath, [path.join(repo, 'app', 'web', 'dev', 'serve.mjs'), String(port)], { stdio: ['ignore', 'pipe', 'inherit'] })
await new Promise((resolve, reject) => { server.stdout.on('data', d => { if (String(d).includes('http://')) resolve() }); server.on('exit', c => reject(new Error(`the app server ended (${c})`))) })
const APP = `http://127.0.0.1:${port}`
const browser = await launchChromium({ width: WIDTH, height: HEIGHT })
const failed = []
try {
  const page = await browser.page()
  await page.send('Runtime.enable'); await page.send('Page.enable')
  const ev = async x => { const r = await page.send('Runtime.evaluate', { expression: x, awaitPromise: true, returnByValue: true }); if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text); return r.result.value }
  await page.send('Emulation.setTouchEmulationEnabled', { enabled: true, maxTouchPoints: 5 })
  await page.send('Emulation.setUserAgentOverride', { userAgent: UA, platform: 'iPhone' })
  await page.send('Emulation.setDeviceMetricsOverride', { width: WIDTH, height: HEIGHT, deviceScaleFactor: 2, mobile: true, screenOrientation: { type: 'portraitPrimary', angle: 0 } })
  try { await page.send('Emulation.setSafeAreaInsetsOverride', { insets: { top: 20, bottom: 0, left: 0, right: 0 } }) } catch {}
  for (const s of states) {
    await page.send('Page.navigate', { url: 'about:blank' })
    await page.send('Page.navigate', { url: APP + urlOf(s) })
    for (let i = 0; i < 100; i++) { if (await ev("document.readyState === 'complete' && (document.documentElement.hasAttribute('data-ready') || !!document.getElementById('room-screen'))").catch(() => false)) break; await sleep(100) }
    await sleep(s.web.state ? 1200 : 700)
    let got
    try { got = await ev(PROBE) } catch (err) { got = { page: 0, past: [{ el: 'probe', text: err.message.slice(0, 120) }] } }
    // A missing demo object used to draw an empty/error page that still passed every layout check.
    const content = s.screen === 'Card' ? '#cardpage .tc-title' : s.screen === 'Chat' ? '#session #chat .chat-pane' : null
    const contentReady = !content || await ev(`Boolean(document.querySelector(${JSON.stringify(content)})?.textContent.trim())`)
    const ok = got.page <= 0 && !got.past.length && contentReady
    console.log(`${ok ? 'ok  ' : 'FAIL'} ${s.id}${ok ? '' : `: ${!contentReady ? 'screen content missing; ' : ''}${got.page > 0 ? `the page scrolls ${got.page}px sideways; ` : ''}${got.past.map(p => `${p.el} "${p.text}" ${p.left}..${p.right}`).join('; ')}`}`)
    if (!ok) failed.push(s.id)
  }
} finally {
  await browser.close()
  server.kill()
}
console.log(`${states.length - failed.length} of ${states.length} states fit ${WIDTH}px${failed.length ? `; too wide: ${failed.join(', ')}` : ''}`)
process.exit(failed.length ? 1 : 0)
