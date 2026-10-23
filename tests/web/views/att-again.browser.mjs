// att-again.browser.mjs: a picture whose object URL was let go of (releaseUnusedFiles, a newer copy of the file) while
// markup drawn before still names it asks the core for its file again and stands again, instead of saying "no longer
// available" for good. A picture that fails a second time does say so, alone (no "Opening the picture…" under it).
// The demo room (?mock=1), a card with pictures, as a desktop window.
//   node tests/web/views/att-again.browser.mjs      (needs Chromium; serves the app from its sources itself)
import net from 'node:net'
import path from 'node:path'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { launchChromium } from '../../../app/web/dev/cdp.mjs'

const here = path.dirname(fileURLToPath(import.meta.url))
const repo = path.join(here, '..', '..', '..')
const sleep = ms => new Promise(r => setTimeout(r, ms))
const CARD = 'a38afef6dbdd056db8042980f2b83336'   // demo/data: "Which hero for the landing page?", two pictures

const STATE = `({ gone: document.querySelectorAll('main .att-gone').length, shown: [...document.querySelectorAll('main img')].filter(i => i.naturalWidth > 0).length, imgs: document.querySelectorAll('main img').length, waitWords: [...document.querySelectorAll('main .tc-figure')].map(f => getComputedStyle(f, '::before').content).filter(c => c && c !== 'none').length })`
const SHOW = `document.querySelector('main .tc-thumbs')?.scrollIntoView({ block: 'nearest' })`

async function freePort() {
  return new Promise((resolve, reject) => { const s = net.createServer(); s.unref(); s.on('error', reject); s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => resolve(port)) }) })
}
const port = await freePort()
const server = spawn(process.execPath, [path.join(repo, 'app', 'web', 'dev', 'serve.mjs'), String(port)], { stdio: ['ignore', 'pipe', 'inherit'] })
await new Promise((resolve, reject) => { server.stdout.on('data', d => { if (String(d).includes('http://')) resolve() }); server.on('exit', c => reject(new Error(`the app server ended (${c})`))) })
const browser = await launchChromium({ width: 1280, height: 800 })
const bad = []
try {
  const page = await browser.page()
  await page.send('Runtime.enable'); await page.send('Page.enable')
  const ev = async x => { const r = await page.send('Runtime.evaluate', { expression: x, awaitPromise: true, returnByValue: true }); if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text); return r.result.value }
  // (the thumbnails load lazily: below the fold of the card's column they would not come at all, so they are scrolled into view)
  const until = async (test, ms = 5000) => { for (let t = 0; t < ms; t += 100) { await ev(SHOW); const s = await ev(STATE); if (test(s)) return s; await sleep(100) } return ev(STATE) }
  await page.send('Page.navigate', { url: `http://127.0.0.1:${port}/card/${CARD}?mock=1` })
  const first = await until(s => s.imgs > 0 && s.shown === s.imgs, 15000)
  if (!first.imgs || first.shown !== first.imgs) bad.push(`the card's pictures did not come: ${JSON.stringify(first)}`)
  // Every object URL let go of, as a page drawn without them does; then each picture fails as a fresh <img> would.
  await ev(`(async () => { const c = await import('/core.mjs'); c.releaseUnusedFiles(document.createElement('div')); for (const i of document.querySelectorAll('main img')) i.dispatchEvent(new Event('error')) })()`)
  const again = await until(s => s.imgs === first.imgs && s.shown === s.imgs)
  if (again.gone || again.shown !== first.imgs) bad.push(`after their URLs were let go of the pictures did not stand again: ${JSON.stringify(again)}`)
  // A picture that fails once more (asked again already) says so, and only that.
  await ev(`(() => { const i = document.querySelector('main .tc-figure img'); i.dataset.attTried = '1'; i.dispatchEvent(new Event('error')) })()`)
  const gone = await until(s => s.gone > 0, 2000)
  if (gone.gone !== 1) bad.push(`a picture failing again does not say so: ${JSON.stringify(gone)}`)
  if (gone.waitWords) bad.push('"Opening the picture…" still stands under "no longer available"')
} finally {
  await browser.close()
  server.kill()
}
console.log(bad.length ? `FAIL ${bad.join('; ')}` : 'ok   pictures whose URLs were let go of come again; a lost one says so alone')
process.exit(bad.length ? 1 : 0)
