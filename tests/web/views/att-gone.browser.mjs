// att-gone.browser.mjs: a picture that did not come (gone from the hub, or a file the page cannot open) leaves its
// place as it was. On the Desk (demo room, ?mock=1) every picture of a row's fan is made to fail: each becomes a tile
// of the picture's own size, the fan stays as wide as before, and the row's title and its answer tabs stay in view,
// clear of the fan. Checked as a desktop window and as a phone.
//   node tests/web/views/att-gone.browser.mjs      (needs Chromium; serves the app from its sources itself)
import net from 'node:net'
import path from 'node:path'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { launchChromium } from '../../../app/web/dev/cdp.mjs'

const here = path.dirname(fileURLToPath(import.meta.url))
const repo = path.join(here, '..', '..', '..')
const sleep = ms => new Promise(r => setTimeout(r, ms))
const SIZES = [{ name: 'desktop', width: 1280, height: 800, mobile: false }, { name: 'phone', width: 390, height: 800, mobile: true }]

/** Breaks every fan's pictures, waits for the tiles, and measures (run in the page). */
const PROBE = `(async () => {
  const rect = el => { const r = el.getBoundingClientRect(); return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width, height: r.height } }
  const fans = [...document.querySelectorAll('.inbox-fan')].filter(f => f.querySelector('img'))
  if (!fans.length) return { error: 'no row with pictures on the Desk' }
  const before = new Map(fans.map(f => [f, rect(f)]))
  let n = 0
  for (const f of fans) for (const img of f.querySelectorAll('img')) img.loading = 'eager', img.src = '/att/' + (++n).toString(16).padStart(32, '0')
  for (let i = 0; i < 50 && fans.some(f => f.querySelector('img')); i++) await new Promise(r => setTimeout(r, 100))
  const out = { rows: [], words: '' }
  for (const f of fans) {
    const row = f.closest('.inbox-row'), title = row.querySelector('.inbox-question'), tabs = row.querySelector('.inbox-when')
    const tiles = [...f.querySelectorAll('.att-gone')]
    const meet = (a, b) => a && b && a.left < b.right - 1 && b.left < a.right - 1 && a.top < b.bottom - 1 && b.top < a.bottom - 1
    const fr = rect(f), tr = rect(title), wr = tabs && getComputedStyle(tabs).display !== 'none' ? rect(tabs) : null
    out.rows.push({ id: row.id, left: f.querySelectorAll('img').length, tiles: tiles.length, tile: tiles.map(t => [Math.round(t.offsetWidth), Math.round(t.offsetHeight), t.classList.contains('is-tile')]), grew: Math.round(fr.width - before.get(f).width), title: Math.round(tr.width), titleHit: meet(fr, tr), tabsHit: meet(fr, wr), past: fr.right > innerWidth + 1 })
  }
  // the words for a file of a newer Trommi
  try { out.words = (await import('/ui.mjs')).attGone(document.createElement('img'), { code: 'newer-version' }).textContent } catch (err) { out.words = err.message }
  return out
})()`

async function freePort() {
  return new Promise((resolve, reject) => { const s = net.createServer(); s.unref(); s.on('error', reject); s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => resolve(port)) }) })
}

const port = await freePort()
const server = spawn(process.execPath, [path.join(repo, 'app', 'web', 'dev', 'serve.mjs'), String(port)], { stdio: ['ignore', 'pipe', 'inherit'] })
await new Promise((resolve, reject) => { server.stdout.on('data', d => { if (String(d).includes('http://')) resolve() }); server.on('exit', c => reject(new Error(`the app server ended (${c})`))) })
const APP = `http://127.0.0.1:${port}`
const browser = await launchChromium({ width: 1280, height: 800 })
const failed = []
try {
  const page = await browser.page()
  await page.send('Runtime.enable'); await page.send('Page.enable')
  const ev = async x => { const r = await page.send('Runtime.evaluate', { expression: x, awaitPromise: true, returnByValue: true }); if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text); return r.result.value }
  for (const size of SIZES) {
    await page.send('Emulation.setDeviceMetricsOverride', { width: size.width, height: size.height, deviceScaleFactor: 1, mobile: size.mobile })
    await page.send('Page.navigate', { url: 'about:blank' })
    await page.send('Page.navigate', { url: `${APP}/?mock=1` })
    for (let i = 0; i < 100; i++) { if (await ev("document.readyState === 'complete' && document.documentElement.hasAttribute('data-ready')").catch(() => false)) break; await sleep(100) }
    await sleep(700)
    const got = await ev(PROBE)
    const bad = []
    if (got.error) bad.push(got.error)
    for (const r of got.rows ?? []) {
      const why = [
        r.left && `${r.left} picture(s) still there`,
        !r.tiles && 'no tile',
        r.tile.some(([w, h, tile]) => !tile || w > 60 || h > 46) && `tiles ${JSON.stringify(r.tile)} are not the pictures' size`,
        r.grew > 2 && `the fan grew ${r.grew}px`,
        r.title < 60 && `the title is ${r.title}px wide`,
        r.titleHit && 'the fan lies over the title',
        r.tabsHit && 'the fan lies over the answer tabs',
        r.past && 'the fan reaches past the screen',
      ].filter(Boolean)
      if (why.length) bad.push(`${r.id}: ${why.join(', ')}`)
    }
    if (!/newer Trommi/.test(got.words ?? '')) bad.push(`a newer file says "${got.words}"`)
    console.log(`${bad.length ? 'FAIL' : 'ok  '} ${size.name}: ${got.rows?.length ?? 0} row(s) with pictures${bad.length ? `; ${bad.join('; ')}` : ''}`)
    if (bad.length) failed.push(size.name)
  }
} finally {
  await browser.close()
  server.kill()
}
process.exit(failed.length ? 1 : 0)
