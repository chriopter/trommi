// off-hint.browser.mjs: the Desk has no floating stacks (demo room, ?mock=1, five cards starred). Starred and
// running cards stand at the head of the section Off your mind, in the Desk's scroll as on the iPhone: one row each
// with the agent's drawing (in its turning ring while it runs), the title and "agent · status line"; the core says
// which (DeskView.starred, OffHintView.running and running_more). Nothing of the Desk is fixed over its rows but the
// selection bar and the knock strips, which are hidden at rest.
// As a 1440 px window and as a phone.
//   node tests/web/views/off-hint.browser.mjs      (needs Chromium; serves the app from its sources itself)
import net from 'node:net'
import path from 'node:path'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { launchChromium } from '../../../app/web/dev/cdp.mjs'

const here = path.dirname(fileURLToPath(import.meta.url))
const repo = path.join(here, '..', '..', '..')
const sleep = ms => new Promise(r => setTimeout(r, ms))
const SIZES = [{ name: '1440 wide', width: 1440, height: 900, mobile: false }, { name: 'phone', width: 390, height: 844, mobile: true }]

/** Stars five cards of the room (run in the page). */
const STAR = `(async () => {
  const cards = trommi.vm().desk.stack.slice(0, 5)
  for (const id of cards) trommi.act({ do: 'star', card_id: id, on: true })
  await new Promise(r => setTimeout(r, 500))
  return cards.length
})()`
/** What stands where (run in the page). */
const PROBE = `(() => {
  const seen = el => { const r = el.getBoundingClientRect(), c = getComputedStyle(el); return c.display !== 'none' && c.visibility !== 'hidden' && r.width > 0 && r.height > 0 }
  const d = trommi.vm().desk, list = document.querySelector('#desk-list'), end = document.querySelector('#desk-end')
  const rows = sel => [...document.querySelectorAll(sel)].map(a => { const r = a.getBoundingClientRect(); return { left: r.left, right: r.right, inList: list.contains(a), title: a.querySelector('.oym-run-words b')?.textContent.trim() ?? '', line: a.querySelector('small')?.textContent.trim() ?? '', ring: Boolean(a.querySelector('.oym-run-mark .agent-ring')), drawing: Boolean(a.querySelector('.oym-run-mark .agent-avatar')), running: a.hasAttribute('data-running') } })
  const fixed = [...document.querySelectorAll('#desk-layer *, #inbox *')].filter(el => seen(el) && ['fixed', 'sticky'].includes(getComputedStyle(el).position)).map(el => el.id || el.className)
  return { vw: innerWidth, starredVm: d.starred?.count ?? 0, runningVm: d.off?.running?.length ?? 0, more: d.off?.running_more?.en ?? null,
    starred: rows('#oym-starred .oym-run'), running: rows('#oym-running .oym-run'), moreShown: document.querySelector('#oym-running .oym-run-more')?.textContent ?? null,
    headFirst: Boolean(end) && end.querySelector('.end-divider')?.compareDocumentPosition(end.querySelector('.oym-runs') ?? end) === Node.DOCUMENT_POSITION_FOLLOWING,
    dock: Boolean(document.querySelector('#desk-dock, .oym-dock, .dock-stack')), fixed, page: document.scrollingElement.scrollWidth - innerWidth }
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
  for (const size of SIZES) {
    await page.send('Emulation.setDeviceMetricsOverride', { width: size.width, height: size.height, deviceScaleFactor: 1, mobile: size.mobile })
    await page.send('Page.navigate', { url: 'about:blank' })
    await page.send('Page.navigate', { url: `${APP}/?mock=1` })
    for (let i = 0; i < 100; i++) { if (await ev("document.readyState === 'complete' && document.documentElement.hasAttribute('data-ready') && Boolean(document.querySelector('#desk-list'))").catch(() => false)) break; await sleep(100) }
    await sleep(700)
    const bad = []
    const starred = await ev(STAR)
    if (starred !== 5) bad.push(`${starred} cards to star, not 5`)
    const p = await ev(PROBE)
    if (p.dock) bad.push('a docked stack is still there')
    if (p.fixed.length) bad.push(`fixed over the Desk: ${p.fixed.join(', ')}`)
    if (p.starredVm !== 5 || p.starred.length !== 5) bad.push(`starred: the core says ${p.starredVm}, the Desk shows ${p.starred.length}`)
    if (!p.runningVm) bad.push('the demo room has no running card')
    if (p.running.length !== p.runningVm) bad.push(`running: the core says ${p.runningVm}, the Desk shows ${p.running.length}`)
    if ((p.more ?? null) !== (p.moreShown ?? null)) bad.push(`"more running": the core says ${p.more}, the Desk ${p.moreShown}`)
    if (!p.headFirst) bad.push('the cards do not stand under the heading Off your mind')
    for (const r of [...p.starred, ...p.running]) {
      if (!r.inList) bad.push(`${r.title}: not in the Desk's scroll`)
      if (!r.title || !r.line) bad.push(`a row without its title or line (${r.title} / ${r.line})`)
      if (!r.drawing) bad.push(`${r.title}: no drawing of its agent`)
      if (r.running !== r.ring) bad.push(`${r.title}: the turning ring ${r.ring ? 'on a card that does not run' : 'missing'}`)
      if (r.left < 0 || r.right > p.vw + 1) bad.push(`${r.title}: lies at ${Math.round(r.left)}..${Math.round(r.right)}`)
    }
    if (p.running.some(r => !r.running)) bad.push('a running row without data-running')
    if (p.page > 0) bad.push(`the page scrolls ${p.page}px sideways`)
    console.log(`${bad.length ? 'FAIL' : 'ok  '} ${size.name}: ${p.starred.length} starred, ${p.running.length} running in Off your mind${bad.length ? `; ${bad.join('; ')}` : ''}`)
    if (bad.length) failed.push(size.name)
  }
} finally {
  await browser.close()
  server.kill()
}
process.exit(failed.length ? 1 : 0)
