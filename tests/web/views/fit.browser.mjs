// fit.browser.mjs: on a wide screen the app fits the window: the page itself never scrolls, only its panes do (the
// owner, 16 October 2026: "Seitenhöhe passt nicht", the chat's composer and the sidebar's foot cut at the bottom).
// The Desk and a session with helpers of the demo room (?mock=1), at 1440×900 and 1280×720: the root is no taller
// than the window, and after what may scroll it from a script (a row scrolled into view, the composer focused, the
// root or the body scrolled by hand) it stands at 0 again, the composer and the sidebar's foot whole in the window.
//   node tests/web/views/fit.browser.mjs      (needs Chromium; serves the app from its sources itself)
import net from 'node:net'
import path from 'node:path'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { launchChromium } from '../../../app/web/dev/cdp.mjs'

const here = path.dirname(fileURLToPath(import.meta.url))
const repo = path.join(here, '..', '..', '..')
const sleep = ms => new Promise(r => setTimeout(r, ms))
const PAGES = [['desk', '/'], ['session with helpers', '/chat/9aa575fae2c64ece4ae4ab1b783673da']]
const SIZES = [[1440, 900], [1280, 720]]

/** Measures, pushes the root and the body as a script could, and measures again (run in the page). */
const PROBE = `(async () => {
  const frame = () => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)))
  const root = document.scrollingElement, body = document.body
  const whole = sel => { const el = document.querySelector(sel); if (!el || !el.getClientRects().length) return null; const r = el.getBoundingClientRect(); return r.top >= -0.5 && r.bottom <= innerHeight + 0.5 }
  const tall = root.scrollHeight - innerHeight
  for (const el of [...document.querySelectorAll('#session .dock, #desk-list .inbox-row')].slice(-1)) el.scrollIntoView({ block: 'end' })
  document.querySelector('#session textarea')?.focus()
  root.scrollTop = 60; body.scrollTop = 60; scrollTo(0, 60)
  await frame(); await new Promise(r => setTimeout(r, 50)); await frame()
  return { tall, root: root.scrollTop, body: body.scrollTop, composer: whole('#session .dock'), foot: whole('.side-foot') }
})()`

async function freePort() {
  return new Promise((resolve, reject) => { const s = net.createServer(); s.unref(); s.on('error', reject); s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => resolve(port)) }) })
}

const port = await freePort()
const server = spawn(process.execPath, [path.join(repo, 'app', 'web', 'dev', 'serve.mjs'), String(port)], { stdio: ['ignore', 'pipe', 'inherit'] })
await new Promise((resolve, reject) => { server.stdout.on('data', d => { if (String(d).includes('http://')) resolve() }); server.on('exit', c => reject(new Error(`the app server ended (${c})`))) })
const APP = `http://127.0.0.1:${port}`
const browser = await launchChromium({ width: 1440, height: 900 })
const failed = []
try {
  const page = await browser.page()
  await page.send('Runtime.enable'); await page.send('Page.enable')
  const ev = async x => { const r = await page.send('Runtime.evaluate', { expression: x, awaitPromise: true, returnByValue: true }); if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text); return r.result.value }
  for (const [width, height] of SIZES) {
    for (const [name, where] of PAGES) {
      await page.send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false })
      await page.send('Page.navigate', { url: 'about:blank' })
      await page.send('Page.navigate', { url: `${APP}${where}?mock=1` })
      for (let i = 0; i < 100; i++) { if (await ev("document.readyState === 'complete' && document.documentElement.hasAttribute('data-ready') && Boolean(document.querySelector('#agents .agent-row'))").catch(() => false)) break; await sleep(100) }
      await sleep(800)
      const got = await ev(PROBE)
      const bad = []
      if (got.tall > 0) bad.push(`the page is ${got.tall}px taller than the window`)
      if (got.root || got.body) bad.push(`the page scrolled (root ${got.root}, body ${got.body})`)
      if (got.foot === false) bad.push('the sidebar foot is cut')
      if (name !== 'desk' && got.composer !== true) bad.push(`the composer is ${got.composer === null ? 'missing' : 'cut'}`)
      const id = `${name} ${width}×${height}`
      console.log(`${bad.length ? 'FAIL' : 'ok  '} ${id}${bad.length ? `: ${bad.join('; ')}` : ''}`)
      if (bad.length) failed.push(id)
    }
  }
} finally {
  await browser.close()
  server.kill()
}
if (failed.length) { console.log(`${failed.length} failed: ${failed.join(', ')}`); process.exit(1) }
console.log(`the app fits the window in ${SIZES.length * PAGES.length} cases`)
