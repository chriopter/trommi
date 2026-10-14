// side-foot.browser.mjs: on a wide screen the sidebar's foot (theme and fold buttons, .side-foot) never stands on a
// session row, the last one included, scrolled or not; and a row of a new session that asks to join shows its host
// and who vouched for it whole, its Allow and Deny inside the sidebar (the owner's board, 2026-10-14: Deny under the
// fold button, "wants to …" cut). Demo room (?mock=1) with a join row put last, a window too low for every row,
// open and folded.
//   node tests/web/views/side-foot.browser.mjs      (needs Chromium; serves the app from its sources itself)
import net from 'node:net'
import path from 'node:path'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { launchChromium } from '../../../app/web/dev/cdp.mjs'

const here = path.dirname(fileURLToPath(import.meta.url))
const repo = path.join(here, '..', '..', '..')
const sleep = ms => new Promise(r => setTimeout(r, ms))

/** Puts a join row last in the list, then measures at the list's top and at its end (run in the page). */
const PROBE = `(async () => {
  const { joinRow } = await import('/sidebar.mjs')
  const list = document.querySelector('#agents')
  const row = document.createElement('div')
  row.innerHTML = String(joinRow({ nonce: 'ab12', says: { en: 'valiido on christophers-macbook-pro.local, vouched by valiido.com Article' } }))
  list.append(row.firstElementChild)
  const rect = el => el.getBoundingClientRect()
  const buttons = [...document.querySelectorAll('.side-foot button')].filter(b => getComputedStyle(b).display !== 'none').map(rect)
  const rows = () => [...list.querySelectorAll(':scope > .agent-row, :scope > .agent-invite')].map(rect)
  const hits = () => { const out = []; for (const r of rows()) for (const b of buttons) { const vis = Math.min(r.bottom, rect(list).bottom) - Math.max(r.top, rect(list).top); if (vis > 0 && r.left < b.right && b.left < r.right && Math.max(r.top, rect(list).top) < b.bottom && b.top < Math.min(r.bottom, rect(list).bottom)) out.push([Math.round(r.top), Math.round(b.top)]) } return out }
  // rows behind the foot's strip are hidden by it: a hit counts only where a row shows at the button's place
  const shown = () => { const out = []; for (const b of buttons) { const el = document.elementFromPoint((b.left + b.right) / 2, b.top - 4); if (el?.closest('.agent-row, .agent-invite')) out.push(el.closest('.agent-row, .agent-invite').id || el.className) } return out }
  list.scrollTop = 0
  await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)))
  const top = shown()
  list.scrollTop = list.scrollHeight
  await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)))
  const end = hits()
  const join = list.querySelector('#join-ab12')
  const cut = [...join.querySelectorAll('strong, small')].filter(e => e.scrollWidth > e.clientWidth + 1).map(e => e.textContent)
  const acts = [...join.querySelectorAll('.join-acts button')].map(rect)
  const outside = acts.filter(a => a.right > rect(list).right || a.left < rect(list).left).length
  const text = join.textContent
  return { buttons: buttons.length, overflowing: list.scrollHeight > list.clientHeight, top, end, cut, outside, text }
})()`

async function freePort() {
  return new Promise((resolve, reject) => { const s = net.createServer(); s.unref(); s.on('error', reject); s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => resolve(port)) }) })
}

const port = await freePort()
const server = spawn(process.execPath, [path.join(repo, 'app', 'web', 'dev', 'serve.mjs'), String(port)], { stdio: ['ignore', 'pipe', 'inherit'] })
await new Promise((resolve, reject) => { server.stdout.on('data', d => { if (String(d).includes('http://')) resolve() }); server.on('exit', c => reject(new Error(`the app server ended (${c})`))) })
const APP = `http://127.0.0.1:${port}`
const browser = await launchChromium({ width: 1280, height: 420 })
const failed = []
try {
  const page = await browser.page()
  await page.send('Runtime.enable'); await page.send('Page.enable')
  const ev = async x => { const r = await page.send('Runtime.evaluate', { expression: x, awaitPromise: true, returnByValue: true }); if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text); return r.result.value }
  for (const rail of ['open', 'folded']) {
    await page.send('Emulation.setDeviceMetricsOverride', { width: 1280, height: 420, deviceScaleFactor: 1, mobile: false })
    await page.send('Page.navigate', { url: 'about:blank' })
    await page.send('Page.navigate', { url: `${APP}/?mock=1` })
    for (let i = 0; i < 100; i++) { if (await ev("document.readyState === 'complete' && document.documentElement.hasAttribute('data-ready') && Boolean(document.querySelector('#agents .agent-row'))").catch(() => false)) break; await sleep(100) }
    if (rail === 'folded') await ev("document.documentElement.dataset.rail = 'folded'")
    await sleep(500)
    const got = await ev(PROBE)
    const bad = []
    if (!got.buttons) bad.push('no foot buttons')
    if (!got.overflowing) bad.push('the list does not scroll (the window is not low enough for the case)')
    if (got.top.length) bad.push(`at the top a row shows under a foot button: ${got.top.join(', ')}`)
    if (got.end.length) bad.push(`at the end a row stands under a foot button: ${JSON.stringify(got.end)}`)
    if (rail === 'open') {
      if (got.cut.length) bad.push(`cut: ${got.cut.join(' | ')}`)
      if (!/christophers-macbook-pro\.local/.test(got.text) || !/vouched by valiido\.com Article/.test(got.text)) bad.push(`the host or who vouched is missing: ${got.text}`)
      if (got.outside) bad.push('Allow or Deny stands outside the sidebar')
    }
    console.log(`${bad.length ? 'FAIL' : 'ok  '} ${rail}${bad.length ? `: ${bad.join('; ')}` : ''}`)
    if (bad.length) failed.push(rail)
  }
} finally {
  await browser.close()
  server.kill()
}
process.exit(failed.length ? 1 : 0)
