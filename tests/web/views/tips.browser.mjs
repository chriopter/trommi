// tips.browser.mjs: every hover hint of the demo is one to three words (the owner, 13 October 2026: "maximal ein bis
// drei Wörter, zum Beispiel einfach der Name der Karte"). Every state of demo/data/screens.json is opened in headless
// Chromium at a desktop's size, and every `title` and `data-tip` on the page is read (app.mjs "tips" shows both, in
// one look). A name the room holds (a card's title, a session's, a desk's, a file's) counts as one word, whatever its
// length: the hint "Landing Page" or a long card title is fine; "Unfold trommi's 12 subs: Connector, Mac, …" is not.
// An aria-label may say more: it is read, not shown.
//   node tests/web/views/tips.browser.mjs      (needs Chromium; serves the app from its sources itself)
//   TROMMI_TIPS_ONLY=<id,id> checks only these states.
import fs from 'node:fs'
import net from 'node:net'
import path from 'node:path'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { launchChromium } from '../../../app/web/dev/cdp.mjs'

const here = path.dirname(fileURLToPath(import.meta.url))
const repo = path.join(here, '..', '..', '..')
const sleep = ms => new Promise(r => setTimeout(r, ms))
const only = (process.env.TROMMI_TIPS_ONLY ?? '').split(',').filter(Boolean)
const states = JSON.parse(fs.readFileSync(path.join(repo, 'demo', 'data', 'screens.json'), 'utf8')).states
  .filter(s => s.web && (!only.length || only.includes(s.id)))
const urlOf = ({ web }) => `${web.path}${web.path.includes('?') ? '&' : '?'}mock=${encodeURIComponent(web.mock ?? '1')}${web.state ? `&state=${encodeURIComponent(web.state)}` : ''}`
/** At most this many words in a hint, a name counting as one. */
export const MAX_WORDS = 3

/** Every hint on the page with its word count, a name of the room counting as one word (run there). */
const PROBE = `(() => {
  const v = window.trommi?.vm?.() ?? {}
  const names = new Set()
  const add = v => { v = String(v ?? '').trim(); if (v.length > 1) names.add(v) }
  for (const r of v.desk?.rows ?? []) { add(r.title); for (const t of r.tiles?.tiles ?? []) add(t.label); add(r.from?.name) }
  for (const c of Object.values(v.cards ?? {})) { add(c.title); for (const o of c.options ?? []) add(o.label); for (const p of c.pictures ?? []) add(p.name) }
  for (const x of [...(v.desk?.starred?.rows ?? []), ...(v.desk?.running?.rows ?? []), ...(v.off?.works ?? []), ...(v.off?.rest ?? []), ...(v.desk?.off?.rows ?? [])]) add(x.title)
  for (const x of v.sidebar?.sessions ?? []) add(x.name?.en)
  for (const d of v.sidebar?.desks ?? []) add(d.name?.en)
  for (const a of v.artifacts ?? []) { add(a.title); add(a.session) }
  for (const f of Object.values(v.files ?? {})) add(f.name)
  for (const s of Object.values(v.sessions ?? {})) { add(s.name?.en); for (const q of s.questions ?? []) add(q.title); for (const r of s.talk ?? []) add(r.card?.title) }
  for (const el of document.querySelectorAll('[data-name]')) add(el.dataset.name)
  for (const el of document.querySelectorAll('.att-name, .asset-name, .file-name, figcaption')) add(el.textContent)
  const list = [...names].sort((a, b) => b.length - a.length)
  const count = t => {
    let s = ' ' + t + ' '
    for (const n of list) if (s.includes(n)) s = s.split(n).join(' NAME ')
    return s.split(/(?:[\\s·,;()\\[\\]→—–/]|:(?!\\d))+/).filter(w => w && !/^[^\\p{L}\\p{N}]+$/u.test(w)).length
  }
  const out = []
  for (const el of document.querySelectorAll('body [title], body [data-tip]')) {
    if (el.closest('svg') && el.tagName.toLowerCase() !== 'svg') continue
    for (const a of ['title', 'data-tip']) {
      const t = el.getAttribute(a)?.trim()
      if (t && count(t) > ${MAX_WORDS}) out.push(t)
    }
  }
  return [...new Set(out)]
})()`

async function freePort() {
  return new Promise((resolve, reject) => { const s = net.createServer(); s.unref(); s.on('error', reject); s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => resolve(port)) }) })
}

const port = await freePort()
const server = spawn(process.execPath, [path.join(repo, 'app', 'web', 'dev', 'serve.mjs'), String(port)], { stdio: ['ignore', 'pipe', 'inherit'] })
await new Promise((resolve, reject) => { server.stdout.on('data', d => { if (String(d).includes('http://')) resolve() }); server.on('exit', c => reject(new Error(`the app server ended (${c})`))) })
const APP = `http://127.0.0.1:${port}`
const browser = await launchChromium({ width: 1440, height: 900 })
const long = new Map()   // hint -> the first state it was seen in
try {
  const page = await browser.page()
  await page.send('Runtime.enable'); await page.send('Page.enable')
  const ev = async x => { const r = await page.send('Runtime.evaluate', { expression: x, awaitPromise: true, returnByValue: true }); if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text); return r.result.value }
  for (const s of states) {
    await page.send('Page.navigate', { url: 'about:blank' })
    await page.send('Page.navigate', { url: APP + urlOf(s) })
    for (let i = 0; i < 100; i++) { if (await ev("document.readyState === 'complete' && (document.documentElement.hasAttribute('data-ready') || !!document.getElementById('room-screen'))").catch(() => false)) break; await sleep(100) }
    await sleep(s.web.state ? 1200 : 700)
    let got = []
    try { got = await ev(PROBE) } catch (err) { got = [`(probe failed: ${err.message.slice(0, 100)})`] }
    for (const t of got) if (!long.has(t)) long.set(t, s.id)
    console.log(`${got.length ? 'FAIL' : 'ok  '} ${s.id}${got.length ? `: ${got.map(t => JSON.stringify(t)).join(', ')}` : ''}`)
  }
} finally {
  await browser.close()
  server.kill()
}
console.log(long.size ? `${long.size} hints longer than ${MAX_WORDS} words` : `every hint of ${states.length} states is at most ${MAX_WORDS} words`)
process.exit(long.size ? 1 : 0)
