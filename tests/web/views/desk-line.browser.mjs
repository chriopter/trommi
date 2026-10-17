// desk-line.browser.mjs: the Desk is calm (demo room, ?mock=1). Under the questions stands one quiet line, as on the
// iPhone: Off your mind counted (OffHintView.line) and the artifacts counted (DeskView.artifacts_line), each a link to
// its page; no rows of Off your mind on the Desk, no turned corner, nothing fixed over the rows but the selection bar
// and the knock strips. On a wide window the note is a line under the goals (NotesView.line, the corner's sticky
// steps aside); a phone keeps its Note tab. The page /stacks/off shows the core's groups (OffListView.groups) in order,
// each with its count and its rows.
// As a 1440 px window and as a phone.
//   node tests/web/views/desk-line.browser.mjs      (needs Chromium; serves the app from its sources itself)
import net from 'node:net'
import path from 'node:path'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { launchChromium } from '../../../app/web/dev/cdp.mjs'

const here = path.dirname(fileURLToPath(import.meta.url))
const repo = path.join(here, '..', '..', '..')
const sleep = ms => new Promise(r => setTimeout(r, ms))
const SIZES = [{ name: '1440 wide', width: 1440, height: 900, mobile: false }, { name: 'phone', width: 390, height: 844, mobile: true }]

/** What the Desk shows (run in the page). */
const DESK = `(() => {
  const seen = el => { if (!el) return false; const r = el.getBoundingClientRect(), c = getComputedStyle(el); return c.display !== 'none' && c.visibility !== 'hidden' && r.width > 0 && r.height > 0 }
  const vm = trommi.vm(), d = vm.desk, list = document.querySelector('#desk-list'), end = document.querySelector('#desk-end')
  const link = s => { const a = document.querySelector(s); return a ? { text: a.textContent.trim(), href: new URL(a.href).pathname, inList: list.contains(a) } : null }
  const fixed = [...document.querySelectorAll('#desk-layer *, #inbox *')].filter(el => seen(el) && ['fixed', 'sticky'].includes(getComputedStyle(el).position)).map(el => el.id || el.className)
  const field = document.querySelector('#desk-note-field')
  return { vw: innerWidth, line: d.off?.line?.en ?? null, arts: d.artifacts_line?.en ?? null, off: link('#desk-off'), art: link('#desk-artifacts'), last: list.lastElementChild === end,
    rows: document.querySelectorAll('#desk-end .inbox-row, .oym-run, .oym-hint, .df-card').length, curl: seen(document.querySelector('.curl')), fixed,
    noteLine: vm.notes.line.en, note: seen(document.querySelector('#desk-note')), placeholder: field?.placeholder ?? null, scribble: link('#desk-scribble'),
    corner: seen(document.querySelector('.corner-note-box')), page: document.scrollingElement.scrollWidth - innerWidth }
})()`
/** What the page Off your mind shows against the core's groups (run in the page). */
const OFF = `(() => {
  const g = trommi.vm().off?.groups ?? []
  const shown = [...document.querySelectorAll('#off-end .off-group')].map(s => ({ head: s.querySelector('h3')?.textContent.trim(), ids: [...s.querySelectorAll('.off-row')].map(a => a.id.slice(4)) }))
  return { want: g.map(x => ({ head: \`\${x.says.en} \${x.count}\`, ids: x.ids })), shown, page: document.scrollingElement.scrollWidth - innerWidth }
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
    const p = await ev(DESK)
    if (!p.line || !p.arts) bad.push(`the demo room should have both lines (${p.line} / ${p.arts})`)
    if (p.off?.text !== p.line || p.off?.href !== '/stacks/off' || !p.off?.inList) bad.push(`Off your mind's line: ${JSON.stringify(p.off)}, the core says ${p.line}`)
    if (p.art?.text !== p.arts || p.art?.href !== '/artifacts') bad.push(`the artifacts' line: ${JSON.stringify(p.art)}, the core says ${p.arts}`)
    if (!p.last) bad.push('the line is not the last thing under the questions')
    if (p.rows) bad.push(`${p.rows} rows of Off your mind or Artifacts still on the Desk`)
    if (p.curl) bad.push('the Desk still has its turned corner')
    if (p.fixed.length) bad.push(`fixed over the Desk: ${p.fixed.join(', ')}`)
    if (size.mobile) {
      if (p.note) bad.push('a phone shows the note line (it has its Note tab)')
    } else {
      if (!p.note || p.placeholder !== p.noteLine) bad.push(`the note line: shown ${p.note}, says ${p.placeholder}, the core ${p.noteLine}`)
      if (p.scribble?.href !== '/scribble-board') bad.push(`the Scribble Board beside it: ${JSON.stringify(p.scribble)}`)
      if (p.corner) bad.push('the corner note still stands on the Desk')
    }
    if (p.page > 0) bad.push(`the Desk scrolls ${p.page}px sideways`)
    await page.send('Page.navigate', { url: `${APP}/stacks/off?mock=1` })
    for (let i = 0; i < 100; i++) { if (await ev("document.documentElement.hasAttribute('data-ready') && Boolean(document.querySelector('#off-end .off-group'))").catch(() => false)) break; await sleep(100) }
    await sleep(400)
    const o = await ev(OFF)
    if (!o.want.length) bad.push('the core names no group')
    if (JSON.stringify(o.want) !== JSON.stringify(o.shown)) bad.push(`the groups: the core says ${JSON.stringify(o.want)}, the page shows ${JSON.stringify(o.shown)}`)
    if (o.page > 0) bad.push(`the page Off your mind scrolls ${o.page}px sideways`)
    console.log(`${bad.length ? 'FAIL' : 'ok  '} ${size.name}: "${p.line}" and "${p.arts}" under the questions, ${o.shown.length} groups on the page${bad.length ? `; ${bad.join('; ')}` : ''}`)
    if (bad.length) failed.push(size.name)
  }
} finally {
  await browser.close()
  server.kill()
}
process.exit(failed.length ? 1 : 0)
