// warm-desk.browser.mjs: the app opened again on a device that was away, while cards it last saw open were closed
// on another device. Its first paint comes from its own cache; the person's rule is that the Desk is the hub's open
// list, so no closed card may ever stand on the Desk, not for one frame (app.mjs boot: client.deskFirst).
//
//   TROMMI_HUB_BIN=/path/to/trommi-hub node tests/web/e2e/warm-desk.browser.mjs
//
// REAL: the built app with its core worker, IndexedDB on disk, the hub's binary, an agent on the real core in Node.
// The page is watched from before its first script: every animation frame and every change of the DOM is checked
// for a row of a closed card.
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { buildApp, main, openProfile, run, serveApp, skip, sleep, watch } from './harness.mjs'
import * as ui from './ui.mjs'
import { startHub } from './real.mjs'
import { agentIn } from './catchup-real.browser.mjs'

const DIR = path.join(os.homedir(), '.cache', 'trommi-warm-desk')
const KEPT = 2, CLOSED = 4
const q = s => JSON.stringify(s)

export async function setUp() {
  fs.rmSync(DIR, { recursive: true, force: true })
  fs.mkdirSync(DIR, { recursive: true })
  const dir = await buildApp('warm-desk-dist')
  let hubUrl = null
  const app = await serveApp(dir, () => hubUrl)
  const hub = await startHub(app.origin)
  hubUrl = hub.url
  return { app, hub, seen: watch(), run: run('a warm open never shows a card closed elsewhere'), open: [], more: [], email: `wd+${Date.now().toString(36)}@example.org`, kept: [], closed: [] }
}
export async function tearDown(ctx) {
  await ctx.agent?.client.stop().catch(() => {})
  for (const p of ctx.open.splice(0)) await p.close().catch(() => {})
  await ctx.app.close().catch(() => {})
  await ctx.hub.close().catch(() => {})
  fs.rmSync(DIR, { recursive: true, force: true })
}
const profile = async (ctx, name) => { const p = await openProfile(name, ctx.seen, { keep: path.join(DIR, name) }); ctx.open.push(p); return p }
const close = async (ctx, p) => { ctx.open.splice(ctx.open.indexOf(p), 1); await p.close() }

/** Watches the page from before its first script: any row of a closed card on the Desk, at any frame or DOM change. */
const watcher = closed => `(() => {
  const closed = new Set(${q(closed)}), seen = (window.__stale = [])
  const t0 = performance.now()
  const check = why => {
    for (const row of document.querySelectorAll('.inbox-groups .inbox-row[data-id]')) if (closed.has(row.dataset.id)) seen.push({ id: row.dataset.id, at: Math.round(performance.now() - t0), why })
  }
  const frame = () => { check('frame'); requestAnimationFrame(frame) }
  requestAnimationFrame(frame)
  new MutationObserver(() => check('dom')).observe(document, { childList: true, subtree: true })
})()`

export const steps = [
  ['the account (writer A), an agent, and device B that sees every card open', async ctx => {
    const w = await profile(ctx, 'writer'), A = w.page
    ctx.writer = w
    ctx.password = await ui.signUp(A, ctx.app.start('/'), ctx.email)
    await ui.takeKit(A)
    await ui.live(A, 'A live', 60000)
    Object.assign(ctx, await agentIn(ctx, A, 'wd-agent'))
    for (let i = 0; i < KEPT + CLOSED; i++) {
      const id = await ctx.agent.askCard({ title: `Warm ${i}`, body: 'Seen open by B.', options: [{ key: 'a', label: 'A' }] }, { push: false })
      ;(i < KEPT ? ctx.kept : ctx.closed).push(id)
    }
    // (an old status info, read on A later: the card the owner saw come back for a moment)
    ctx.info = await ctx.agent.askCard({ card_type: 'info', title: 'Status 11:05', body: 'All rooms still working.' }, { push: false })
    await ctx.agent.client.settle({ timeout_ms: 60000 })
    ctx.closed.push(ctx.info)
    const b = await profile(ctx, 'away')
    await ui.logIn(b.page, ctx.app.start('/'), ctx.email, ctx.password)
    await ui.live(b.page, 'B live', 120000)
    await b.page.until(`[...document.querySelectorAll('.inbox-groups .inbox-row[data-id]')].filter(r => ${q([...ctx.kept, ...ctx.closed])}.includes(r.dataset.id)).length === ${KEPT + CLOSED + 1}`, 'B shows every card', 120000)
    await sleep(3000)   // (its cache and the hub's Desk as read are written)
    await close(ctx, b)
  }],
  ['while B is away: A reads the status info, answers four cards and the agent closes them', async ctx => {
    const A = ctx.writer.page
    await A.until(`[...trommi.client.model.cards.values()].filter(c => c.object_state === 'open').length >= ${KEPT + CLOSED}`, 'A holds every card', 60000)
    await A.js(`await trommi.client.markRead({ object_id: ${q(ctx.info)} }); return true`)
    for (const id of ctx.closed.filter(id => id !== ctx.info)) {
      await A.js(`await trommi.client.answer({ object_id: ${q(id)}, choices: ['a'] }); return true`)
      await ctx.agent.closeCard(id, { closed_note: 'done' })
    }
    await ctx.agent.client.settle({ timeout_ms: 60000 })
    await A.until('trommi.client.model.outbox.length === 0', 'A sent everything', 60000)
    ctx.written = true
  }],
  ['B opens again: no closed card in the Desk at any frame, the open ones there', async ctx => {
    if (!ctx.written) throw skip('the cards were not closed')
    const b = await profile(ctx, 'away'), P = b.page
    await P.session.send('Page.addScriptToEvaluateOnNewDocument', { source: watcher(ctx.closed) })
    await P.session.send('Page.navigate', { url: ctx.app.start('/') })
    await P.until('document.documentElement.hasAttribute("data-ready")', 'B drawn', 60000)
    await ui.live(P, 'B live again', 120000)
    await P.until(`${q(ctx.closed)}.every(id => trommi.client.model.cards.get(id)?.object_state !== 'open')`, 'B read the closings', 120000)
    await P.until(`${q(ctx.kept)}.every(id => document.querySelector('.inbox-groups .inbox-row[data-id="' + id + '"]'))`, 'the open cards on the Desk', 30000)
    await sleep(1000)
    const stale = await P.js('return window.__stale')
    const waited = await P.js('return Math.round(window.trommi.deskFirstMs ?? -1)')
    ctx.run.note(`first paint waited for the Desk until ${waited} ms after the start`)
    ctx.run.check(Array.isArray(stale) && stale.length === 0, 'no row of a closed card was ever in the Desk', stale?.slice(0, 5))
    await close(ctx, b)
  }],
  ['nothing the pages complained about', async ctx => {
    ctx.run.check(ctx.seen.exceptions.length === 0, 'no uncaught exception', ctx.seen.exceptions.slice(0, 5))
  }],
]

if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) await main('warm-desk', { setUp, steps, tearDown }, { hub: true })
