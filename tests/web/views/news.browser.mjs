// news.browser.mjs: What's new as the page draws it (app/web/public/news.mjs), in headless Chromium from the sources:
// one card per day of /whats-new.json (the dev server's: the summaries and the history, no releases), its lines folded
// behind Details, each day's picture on its card, small and loaded, a day without a picture drawn all the same.
//   node tests/web/views/news.browser.mjs      (needs Chromium; serves the app from its sources itself)
import net from 'node:net'
import path from 'node:path'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { launchChromium } from '../../../app/web/dev/cdp.mjs'

const repo = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..', '..')
const sleep = ms => new Promise(r => setTimeout(r, ms))

const port = await new Promise((resolve, reject) => { const s = net.createServer(); s.unref(); s.on('error', reject); s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => resolve(port)) }) })
const server = spawn(process.execPath, [path.join(repo, 'app', 'web', 'dev', 'serve.mjs'), String(port)], { stdio: ['ignore', 'pipe', 'inherit'] })
await new Promise((resolve, reject) => { server.stdout.on('data', d => { if (String(d).includes('http://')) resolve() }); server.on('exit', c => reject(new Error(`the app server ended (${c})`))) })
const { days: DAYS } = await (await fetch(`http://127.0.0.1:${port}/whats-new.json`)).json()
const browser = await launchChromium({ width: 1200, height: 900 })
const problems = []
try {
  const page = await browser.page()
  await page.send('Runtime.enable'); await page.send('Page.enable')
  const ev = async x => { const r = await page.send('Runtime.evaluate', { expression: x, awaitPromise: true, returnByValue: true }); if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text); return r.result.value }
  await page.send('Page.navigate', { url: `http://127.0.0.1:${port}/whats-new?mock=1` })
  for (let i = 0; i < 150; i++) { if (await ev("document.querySelectorAll('.news-day').length > 0").catch(() => false)) break; await sleep(100) }
  await sleep(800)
  const got = await ev(`(async () => {
    for (const i of document.querySelectorAll('.news-pic')) i.loading = 'eager'
    await Promise.all([...document.querySelectorAll('.news-pic')].map(i => i.decode().catch(() => {})))
    return [...document.querySelectorAll('.news-day')].map(d => ({
      open: Boolean(d.querySelector('.news-more')?.open),
      pics: [...d.querySelectorAll('img.news-pic')].map(i => ({ src: new URL(i.src).pathname, loaded: i.complete && i.naturalWidth > 0, width: i.getBoundingClientRect().width })),
      lines: d.querySelectorAll('.news-line').length,
    }))
  })()`)
  if (got.length !== DAYS.length) problems.push(`${got.length} days drawn, /whats-new.json has ${DAYS.length}`)
  DAYS.forEach((d, i) => {
    const g = got[i]
    if (!g) return
    const lines = d.groups.reduce((n, p) => n + p.items.length, 0)
    if (g.open) problems.push(`${d.day}: its lines are open, should be behind Details`)
    if (g.lines !== lines) problems.push(`${d.day}: ${g.lines} lines drawn of ${lines}`)
    if (d.picture) {
      if (g.pics.length !== 1 || g.pics[0].src !== d.picture.src) problems.push(`${d.day}: its picture ${d.picture.src} is not shown`)
      else if (!g.pics[0].loaded) problems.push(`${d.day}: its picture ${d.picture.src} did not load`)
      else if (g.pics[0].width > 400) problems.push(`${d.day}: its picture is ${Math.round(g.pics[0].width)}px wide, not small`)
    } else if (g.pics.length) problems.push(`${d.day}: a picture on a day without one`)
  })
} finally {
  await browser.close()
  server.kill()
}
for (const p of problems) console.log(`FAIL ${p}`)
console.log(problems.length ? `${problems.length} problems` : `ok: ${DAYS.length} day cards, ${DAYS.filter(d => d.picture).length} pictures shown`)
process.exit(problems.length ? 1 : 0)
