// foot-level.browser.mjs: at the foot of the Desk (demo room, ?mock=1) the sections Off your mind and Artifacts stand
// level when they sit side by side: heading against heading, the first row (a starred or running card, else a put-off or closed one) against the first tile. Checked at two
// window widths, with and without the list of work before them (#desk-ip), since each used to bring its own space.
//   node tests/web/views/foot-level.browser.mjs      (needs Chromium; serves the app from its sources itself)
import net from 'node:net'
import path from 'node:path'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { launchChromium } from '../../../app/web/dev/cdp.mjs'

const here = path.dirname(fileURLToPath(import.meta.url))
const repo = path.join(here, '..', '..', '..')
const sleep = ms => new Promise(r => setTimeout(r, ms))
const SIZES = [{ name: '1440 wide', width: 1440, height: 1000 }, { name: '1100 wide', width: 1100, height: 800 }]

/** The tops of both headings and of what stands first under each (run in the page). */
const PROBE = `(() => {
  const top = s => { const el = document.querySelector(s); return el ? Math.round(el.getBoundingClientRect().top * 10) / 10 : null }
  const left = s => document.querySelector(s)?.getBoundingClientRect().left ?? null
  const measure = () => ({ offHead: top('#desk-end .end-head'), artHead: top('#desk-artifacts .end-head'), offRow: top('#desk-end :is(.oym-run, .oym-hint-row)'), artTile: top('#desk-artifacts .df-thumb'), beside: left('#desk-artifacts') > left('#desk-end') + 50 })
  const plain = measure()
  const ip = document.querySelector('#desk-ip'), was = ip?.hidden
  if (ip) ip.hidden = !was
  const other = measure()
  if (ip) ip.hidden = was
  return { plain, other, hadIp: Boolean(ip) }
})()`

async function freePort() {
  return new Promise((resolve, reject) => { const s = net.createServer(); s.unref(); s.on('error', reject); s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => resolve(port)) }) })
}

const port = await freePort()
const server = spawn(process.execPath, [path.join(repo, 'app', 'web', 'dev', 'serve.mjs'), String(port)], { stdio: ['ignore', 'pipe', 'inherit'] })
await new Promise((resolve, reject) => { server.stdout.on('data', d => { if (String(d).includes('http://')) resolve() }); server.on('exit', c => reject(new Error(`the app server ended (${c})`))) })
const APP = `http://127.0.0.1:${port}`
const browser = await launchChromium({ width: 1440, height: 1000 })
const failed = []
try {
  const page = await browser.page()
  await page.send('Runtime.enable'); await page.send('Page.enable')
  const ev = async x => { const r = await page.send('Runtime.evaluate', { expression: x, awaitPromise: true, returnByValue: true }); if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text); return r.result.value }
  for (const size of SIZES) {
    await page.send('Emulation.setDeviceMetricsOverride', { width: size.width, height: size.height, deviceScaleFactor: 1, mobile: false })
    await page.send('Page.navigate', { url: 'about:blank' })
    await page.send('Page.navigate', { url: `${APP}/?mock=1` })
    for (let i = 0; i < 100; i++) { if (await ev("document.readyState === 'complete' && document.documentElement.hasAttribute('data-ready') && Boolean(document.querySelector('#desk-artifacts .df-thumb'))").catch(() => false)) break; await sleep(100) }
    await sleep(700)
    const got = await ev(PROBE)
    const bad = []
    if (!got.hadIp) bad.push('no #desk-ip on the Desk')
    for (const [state, m] of [['as it is', got.plain], ['with the list before it switched', got.other]]) {
      if (m.offHead == null || m.artHead == null || m.offRow == null || m.artTile == null) { bad.push(`${state}: a section is missing ${JSON.stringify(m)}`); continue }
      if (!m.beside) bad.push(`${state}: the sections do not sit side by side`)
      if (Math.abs(m.offHead - m.artHead) > 1) bad.push(`${state}: the headings start at ${m.offHead} and ${m.artHead}`)
      if (Math.abs(m.offRow - m.artTile) > 1) bad.push(`${state}: the first row starts at ${m.offRow}, the first tile at ${m.artTile}`)
    }
    console.log(`${bad.length ? 'FAIL' : 'ok  '} ${size.name}: headings at ${got.plain.offHead} / ${got.plain.artHead}, first row and tile at ${got.plain.offRow} / ${got.plain.artTile}${bad.length ? `; ${bad.join('; ')}` : ''}`)
    if (bad.length) failed.push(size.name)
  }
} finally {
  await browser.close()
  server.kill()
}
process.exit(failed.length ? 1 : 0)
