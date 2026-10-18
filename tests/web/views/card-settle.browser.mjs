// card-settle.browser.mjs: a card answered on its own page does not jump (demo room, ?mock=1, a 1440 px window). The
// answered state is shorter than the options; the card's height glides there (card.mjs settleAfter), so no frame moves
// the card's foot by more than a small step, and the card ends at its new height. On the Desk an answered row leaves
// and the rows below glide up (app.mjs flipOut): no row moves by more than a small step in one frame either.
//   node tests/web/views/card-settle.browser.mjs      (needs Chromium; serves the app from its sources itself)
import net from 'node:net'
import path from 'node:path'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { launchChromium } from '../../../app/web/dev/cdp.mjs'

const here = path.dirname(fileURLToPath(import.meta.url))
const repo = path.join(here, '..', '..', '..')
const sleep = ms => new Promise(r => setTimeout(r, ms))
const STEP = 60   // px one frame may move what stands on the page

/** Watches every frame for 2 s after a click on `sel`: the largest move of the card's foot (or of a Desk row) in one frame. */
const WATCH = (sel, what) => `(async () => {
  const el = () => document.querySelector(${JSON.stringify(what)})
  const rows = () => [...document.querySelectorAll('#desk-list .inbox-row[id]')]
  const at = new Map(), worst = { step: 0, by: '' }
  const look = () => {
    const c = el(); if (c) { const b = Math.round(c.getBoundingClientRect().bottom), was = at.get('card'); if (was != null) { const d = Math.abs(b - was); if (d > worst.step) Object.assign(worst, { step: d, by: 'card' }) } at.set('card', b) }
    for (const r of rows()) { const t = Math.round(r.getBoundingClientRect().top); if (t > innerHeight || t < -200) continue; const was = at.get(r.id); if (was != null && Math.abs(t - was) > worst.step) Object.assign(worst, { step: Math.abs(t - was), by: r.id }); at.set(r.id, t) }
  }
  let on = true; const loop = () => { look(); if (on) requestAnimationFrame(loop) }; loop()
  await new Promise(r => setTimeout(r, 200))
  const before = el()?.getBoundingClientRect().height ?? 0
  const b = document.querySelector(${JSON.stringify(sel)}); b?.click()
  await new Promise(r => setTimeout(r, 2000)); on = false
  return { clicked: Boolean(b), worst, before, after: el()?.getBoundingClientRect().height ?? 0 }
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
  await page.send('Emulation.setDeviceMetricsOverride', { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false })
  const ev = async x => { const r = await page.send('Runtime.evaluate', { expression: x, awaitPromise: true, returnByValue: true }); if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text); return r.result.value }
  const open = async (url, ready) => {
    await page.send('Page.navigate', { url: 'about:blank' })
    await page.send('Page.navigate', { url: `${APP}${url}` })
    for (let i = 0; i < 100; i++) { if (await ev(`document.readyState === 'complete' && document.documentElement.hasAttribute('data-ready') && Boolean(document.querySelector(${JSON.stringify(ready)}))`).catch(() => false)) break; await sleep(100) }
    await sleep(800)
  }
  // a card to read, answered on its own page
  await open('/?mock=1', '#desk-list')
  const id = await ev("trommi.vm().desk.rows.find(r => r.kind === 'info')?.id ?? ''")
  await open(`/card/${id}?mock=1`, '.tc-card .tc-tile.is-ack')
  const c = await ev(WATCH('.tc-card .tc-tile.is-ack', '.tc-card'))
  const bad = []
  if (!c.clicked) bad.push('no "Got it" on the card page')
  if (Math.abs(c.after - c.before) < 8) bad.push(`the answered card is as tall as before (${c.before} px): nothing to glide, the test does not see the case`)
  if (c.worst.step > STEP) bad.push(`the card jumps: ${c.worst.by} moved ${c.worst.step} px in one frame`)
  console.log(`${bad.length ? 'FAIL' : 'ok  '} card page: ${Math.round(c.before)} px to ${Math.round(c.after)} px, at most ${c.worst.step} px a frame${bad.length ? `; ${bad.join('; ')}` : ''}`)
  if (bad.length) failed.push('card page')
  // a row answered on the Desk
  await open('/?mock=1', '#desk-list .inbox-row')
  const d = await ev(WATCH('#desk-list .inbox-row .inbox-answer.is-thumb.is-lead:not(.is-ack)', '#no-such-element'))
  const bad2 = []
  if (!d.clicked) bad2.push('no answer tile on the Desk')
  if (d.worst.step > STEP) bad2.push(`a row jumps: ${d.worst.by} moved ${d.worst.step} px in one frame`)
  console.log(`${bad2.length ? 'FAIL' : 'ok  '} Desk: at most ${d.worst.step} px a frame${bad2.length ? `; ${bad2.join('; ')}` : ''}`)
  if (bad2.length) failed.push('Desk')
} finally {
  await browser.close()
  server.kill()
}
process.exit(failed.length ? 1 : 0)
