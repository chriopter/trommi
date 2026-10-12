// catchup-real.browser.mjs: a device that comes back after a long time, and a fresh login, against the REAL hub with
// the REAL core everywhere. What it measures is what a person sees: how long the app shows cards as open that were
// answered and closed elsewhere meanwhile, and how long until it is live at the hub's head; and where the time goes
// (the core worker's sums, app/web/core/spent.ts: hub pages, Commits and other log entries, Welcomes, key handover,
// the walk of a joined group's past, the store's writes, the model, the app's cache, the newest state read first).
//
//   TROMMI_HUB_BIN=/path/to/trommi-hub node tests/web/e2e/catchup-real.browser.mjs
//   TROMMI_CR=cards,chat,agents    what happens while the device is away (default 120,1500,12): cards asked by the
//                                  agent, all but a fifth answered on the writing device and closed by the agent; chat
//                                  items; more agents let in by invite (each a room Commit and a main session: a
//                                  Welcome and new epochs for every human device), with 20 items each
//   TROMMI_CR_OUT=<file.json>      where the numbers go (default: the system's temp folder)
// REAL: the built app with its core worker on the real binding, IndexedDB on disk, the hub's binary, an agent device
// on the real core in Node (tests/web/stand-in/agent.ts, let in by the app's own agent invite). Nothing is replaced.
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { buildApp, main, openProfile, run, serveApp, skip, sleep, watch } from './harness.mjs'
import * as ui from './ui.mjs'
import { startHub } from './real.mjs'
import { roomsOn } from '../../../app/web/core/room.ts'
import { AgentHub, AgentStandIn } from '../stand-in/agent.ts'
import { MemoryStore, memoryCache, storage, TIMING } from '../client/helpers.mjs'

const [CARDS, CHAT, AGENTS] = (process.env.TROMMI_CR || '120,1500,12').split(',').map(Number)
const OUT = process.env.TROMMI_CR_OUT || path.join(os.tmpdir(), 'trommi-catchup-real.json')
const DIR = path.join(os.homedir(), '.cache', 'trommi-catchup-real')

export async function setUp() {
  fs.rmSync(DIR, { recursive: true, force: true })
  fs.mkdirSync(DIR, { recursive: true })
  const dir = await buildApp('catchup-real-dist')
  let hubUrl = null
  const app = await serveApp(dir, () => hubUrl)
  const hub = await startHub(app.origin)
  hubUrl = hub.url
  return { app, hub, seen: watch(), run: run('catch-up after a long time, real hub'), open: [], email: `cr+${Date.now().toString(36)}@example.org`, results: [], closed: new Set(), open_cards: new Set(), more: [] }
}
export async function tearDown(ctx) {
  await ctx.agent?.client.stop().catch(() => {})
  for (const g of ctx.more) await g.client.stop().catch(() => {})
  for (const p of ctx.open.splice(0)) await p.close().catch(() => {})
  await ctx.app.close().catch(() => {})
  await ctx.hub.close().catch(() => {})
  fs.rmSync(DIR, { recursive: true, force: true })
}
const profile = async (ctx, name) => { const p = await openProfile(name, ctx.seen, { keep: path.join(DIR, name) }); ctx.open.push(p); return p }
const close = async (ctx, p) => { ctx.open.splice(ctx.open.indexOf(p), 1); await p.close() }
/** The hub's head as the writing device A has taken it (A stays open and live the whole run). */
const head = ctx => ctx.writer.page.js('return trommi.client.model.room.last_envelope_number')
const q = s => JSON.stringify(s)

/** The agent: a device on the real core in Node, let in by the agent invite of page A. */
export async function agentIn(ctx, A, name = 'cr-agent') {
  await ui.openSettings(A)
  // (Settings has two of them, one for each layout: the one shown is pressed)
  const shown = "[...document.querySelectorAll('#settings-invite-agent')].find(b => b.getClientRects().length)"
  await A.until(shown, 'Invite an Agent on Settings', 30000)
  await A.js(`${shown}.click()`)
  await A.until("location.pathname.startsWith('/pair/') && document.querySelector('[data-state=open] .clip-copy[data-line=connect] code')", 'the agent invite page')
  const link = /'(http\S+\/join#v1\.[^']+)'/.exec(await A.js("return document.querySelector('[data-state=open] .clip-copy[data-line=connect] code').textContent"))?.[1]
  const binding = await import('../../../core/wasm/pkg/trommi-core.js')
  await binding.init(fs.readFileSync(new URL('../../../core/wasm/pkg/trommi_core_wasm_bg.wasm', import.meta.url)))
  const { init: _i, Device, TrommiError, StoreConflict: _s, ...stateless } = binding
  const core = { ...stateless, createDevice: store => Device.create(store), openDevice: store => Device.open(store), errorCode: e => (e instanceof TrommiError ? e.code : null) }
  // (AgentHub: the room group's Commits at their place for the agent, which follows the room by its log)
  const R = roomsOn({ core: async () => core, store: name => new MemoryStore(name), cache: async name => memoryCache(name), destroy: async () => {}, timing: TIMING, hub: opts => new AgentHub(opts) })
  const joining = R.joinRoom({ link, storage: storage(name), poll_ms: 100, device_name: name, fetch: leased() })
  await joining.check_code
  await A.until("document.querySelector('[data-state=confirm_code] .clip-ask .check-yes')", 'the agent\'s six emoji')
  await A.click('[data-state=confirm_code] .clip-ask .check-yes')
  const client = await joining.client
  const agent = new AgentStandIn(client)
  await client.start()
  await agent.setRegister('profile', { model: 'stand-in', task: 'Catch-up', agent_name: name })
  await A.until("document.querySelector('[data-state=joined]')", 'the agent is in', 30000)
  await ui.openDesk(A)
  return { agent, core }
}

/** A fetch for the agent's hub client that holds the device's lease (13.7) as the connector does: the real hub takes
 *  an agent device's writes only with its generation in `Trommi-Lease`. Acquired at the first write, renewed every 20 s. */
function leased() {
  const process = Buffer.from(crypto.getRandomValues(new Uint8Array(16))).toString('base64url')
  let lease = null
  return async (url, init = {}) => {
    const headers = new Headers(init.headers), auth = headers.get('authorization'), method = (init.method ?? 'GET').toUpperCase()
    if (auth && method !== 'GET' && !String(url).endsWith('/v1/link')) {
      if (!lease || Date.now() - lease.at > 20_000) {
        const res = await fetch(new URL('/v1/link', url), { method: 'POST', headers: { authorization: auth, 'content-type': 'application/json' }, body: JSON.stringify({ process, ...(lease ? { generation: lease.generation } : {}) }) })
        if (res.ok) lease = { generation: (await res.json()).generation, at: Date.now() }
      }
      if (lease) headers.set('trommi-lease', String(lease.generation))
    }
    return fetch(url, { ...init, headers })
  }
}

/** What page P shows, in one read: whether it is drawn, its cursor and connection, and how many cards it holds or
 *  shows as open that the hub has closed. */
const look = (P, closed, wanted) => P.js(`const m = window.trommi?.client?.model, closed = new Set(${q([...closed])}), wanted = ${q([...wanted])}
  if (!m) return null
  const open = [...m.cards.values()].filter(c => c.object_state === 'open' && !c.catching_up)
  const shown = [...document.querySelectorAll('.inbox-groups .inbox-row[data-id]')].map(r => r.dataset.id)
  return { ready: document.documentElement.hasAttribute('data-ready'), cursor: m.room.last_envelope_number, connection: m.room.connection,
    stale_open: open.filter(c => closed.has(c.object_id)).length, stale_shown: shown.filter(id => closed.has(id)).length, shown: shown.length,
    open: open.length, readable: wanted.filter(id => m.cards.get(id)?.object_state === 'open' && m.cards.get(id)?.content_state === 'ok').length, catching: [...m.cards.values()].filter(c => c.catching_up).length }`).catch(() => null)

/** Opens profile `name` at the app (logged in already, or `login`: through the login screen) and follows it until it
 *  is live at the hub's head. Returns the times since the navigation. */
async function follow(ctx, name, { login = false } = {}) {
  const p = await profile(ctx, name), P = p.page, top = await head(ctx)
  const t = Date.now()
  if (login) await ui.logIn(P, ctx.app.start('/'), ctx.email, ctx.password)
  else await P.session.send('Page.navigate', { url: ctx.app.start('/') })
  const out = { name, head: top, drawn: null, model_stale_gone: null, marked: null, first_seen_stale: null, stale_gone: null, readable: null, live: null, max_stale_shown: 0, samples: 0 }
  for (;;) {
    const s = await look(P, ctx.closed, ctx.open_cards)
    const at = Date.now() - t
    if (s) {
      out.samples++
      if (s.ready && out.drawn === null) out.drawn = at
      const stale = Math.max(s.stale_open, s.stale_shown)
      if (s.stale_open === 0 && out.model_stale_gone === null && s.cursor > 0) out.model_stale_gone = at
      if (s.catching > 0 && out.marked === null) out.marked = at
      if (s.ready) out.max_stale_shown = Math.max(out.max_stale_shown, s.stale_shown)
      if (s.ready && stale > 0 && out.first_seen_stale === null) out.first_seen_stale = at
      if (s.ready && stale === 0 && out.stale_gone === null && s.cursor > 0) out.stale_gone = at
      if (stale > 0) out.stale_gone = null
      if (s.ready && s.readable === ctx.open_cards.size && out.readable === null) out.readable = at
      if (s.ready && s.readable < ctx.open_cards.size) out.readable = null
      if (s.ready && s.connection === 'live' && s.cursor >= top && out.live === null) out.live = at
      if (out.live !== null && out.readable !== null && out.stale_gone !== null) { out.last = s; break }
    }
    if (at > 30 * 60_000) throw new Error(`${name} not at the head after 30 minutes: ${JSON.stringify(s)}`)
    await sleep(100)
  }
  await sleep(2000)   // (what the start does after it is live: the marks of it)
  const all = (await P.workers('globalThis.__trommiSpent ? JSON.parse(JSON.stringify(globalThis.__trommiSpent)) : undefined')).filter(Boolean)
  // (the core worker is the one that made calls into a device; a page may have more workers)
  out.spent = all.sort((x, y) => y.calls - x.calls)[0]
  await close(ctx, p)
  return out
}

const s1 = v => (v === null || v === undefined ? '–' : `${(v / 1000).toFixed(1)} s`)
const line = r => {
  const s = r.spent ?? {}
  return `${r.name}: drawn ${s1(r.drawn)}, closed cards shown as open until ${s1(r.stale_gone)} (model ${s1(r.model_stale_gone)}, marked ${s1(r.marked)}; at most ${r.max_stale_shown} rows), every open card readable ${s1(r.readable)}, live at the head ${s1(r.live)}; ` +
    `hub ${s1(s.hub)}, core ${s1(s.core)} (store ${s1(s.store)}: wrap ${s1(s.wrap)}, idb ${s1(s.idb)}), log ${s1(s.log)}, welcome ${s1(s.welcome)} (${s.welcomes}), keys ${s1(s.keys)}, learn ${s1(s.learn)}, desk ${s1(s.desk)}, model ${s1(s.model)}, cache ${s1(s.cache)}; ` +
    `items ${s.items}, log entries ${s.commits}, core calls ${s.calls}, store writes ${s.writes}, rescan ${s1(s.rescan)} (read again ${s.again}); ` +
    `marks: ${(s.marks ?? []).map(([w, t]) => `${w} @${(t / 1000).toFixed(1)}`).join(' | ')}`
}

export const steps = [
  ['the account (writer A), an agent on the real core let in by the app\'s invite, and the away device B logged in once', async ctx => {
    const w = await profile(ctx, 'writer'), A = w.page
    ctx.writer = w
    ctx.password = await ui.signUp(A, ctx.app.start('/'), ctx.email)
    await ui.takeKit(A)
    await ui.live(A, 'A live', 60000)
    Object.assign(ctx, await agentIn(ctx, A))
    // cards that B will see open before it goes away
    for (let i = 0; i < 30; i++) ctx.open_cards.add(await ctx.agent.askCard({ title: `Before ${i}`, body: 'Seen open by B.', options: [{ key: 'a', label: 'A' }, { key: 'b', label: 'B' }] }, { push: false }))
    await ctx.agent.client.settle({ timeout_ms: 60000 })
    const b = await profile(ctx, 'away')
    await ui.logIn(b.page, ctx.app.start('/'), ctx.email, ctx.password)
    await ui.live(b.page, 'B live', 120000)
    await b.page.until(`[...trommi.client.model.cards.values()].filter(c => c.object_state === 'open').length >= 30`, 'B holds the 30 open cards', 120000)
    await sleep(3000)   // (its cache written, its KeyPackages at the hub)
    await close(ctx, b)
  }],
  [`while B is away: ${CARDS} more cards, all but a fifth answered on A and closed by the agent; ${CHAT} chat items; ${AGENTS} more agents`, async ctx => {
    const A = ctx.writer.page, agent = ctx.agent, t = Date.now()
    const cards = [...ctx.open_cards]
    for (let i = 0; i < CARDS; i++) cards.push(await agent.askCard({ title: `Away ${i}`, body: 'Asked while B was away.', options: [{ key: 'a', label: 'A' }, { key: 'b', label: 'B' }] }, { push: false }))
    await agent.client.settle({ timeout_ms: 120000 })
    const keep = new Set(cards.slice(cards.length - Math.floor(CARDS / 5)))
    await A.until(`[...trommi.client.model.cards.values()].filter(c => c.object_state === 'open').length >= ${cards.length}`, 'A holds every card', 120000)
    for (const id of cards) {
      if (keep.has(id)) continue
      await A.js(`await trommi.client.answer({ object_id: ${q(id)}, choices: ['a'] }); return true`)
      await agent.closeCard(id, { closed_note: 'done' })
      ctx.closed.add(id)
    }
    ctx.open_cards = keep
    for (let i = 0; i < CHAT; i++) {
      await agent.say({ text: `said ${i} ${'x'.repeat(120)}` })
      if (i % 200 === 199) await agent.client.settle({ timeout_ms: 120000 })
    }
    // more agents, each let in by A's invite: a room Commit, a main session founded by A (a Welcome and new epochs
    // for B), and what each says there
    for (let i = 0; i < AGENTS; i++) {
      const more = await agentIn(ctx, A, `cr-agent-${i}`)
      ctx.more.push(more.agent)
      for (let k = 0; k < 20; k++) await more.agent.say({ text: `agent ${i} says ${k}` })
      await more.agent.client.settle({ timeout_ms: 60000 })
    }
    await agent.client.settle({ timeout_ms: 120000 })
    await A.until("trommi.client.model.outbox.length === 0", 'A sent everything', 120000)
    ctx.written = true
    ctx.run.note(`history written in ${Math.round((Date.now() - t) / 1000)} s; hub head ${await head(ctx)}; ${ctx.closed.size} cards closed, ${ctx.open_cards.size} open`)
  }],
  ['B comes back: how long it shows closed cards as open, how long until live at the head', async ctx => {
    if (!ctx.written) throw skip('no history was written')
    const r = await follow(ctx, 'away')
    ctx.results.push(r)
    ctx.run.note(line(r))
  }],
  ['a fresh login on a new browser with all that history', async ctx => {
    if (!ctx.written) throw skip('no history was written')
    const r = await follow(ctx, 'fresh', { login: true })
    ctx.results.push(r)
    ctx.run.note(line(r))
  }],
  ['nothing the pages complained about', async ctx => {
    ctx.run.check(ctx.seen.exceptions.length === 0, 'no uncaught exception', ctx.seen.exceptions.slice(0, 5))
    fs.writeFileSync(OUT, JSON.stringify({ CARDS, CHAT, AGENTS, results: ctx.results }, null, 2))
    ctx.run.note(`numbers in ${OUT}`)
  }],
]

if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) await main('catchup-real', { setUp, steps, tearDown }, { hub: true })
