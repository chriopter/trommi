// The web app's executor (app/web/core/shell.ts, CoreShell) with the REAL app core (core/bindings/wasm/pkg, AppCore)
// as one human device of the mega test (tests/client/mega.rs), driven as the page's core worker drives it
// (app/web/core/core-worker.ts), with memory spaces in place of IndexedDB. Every deed is an action of the core
// (`Event::Action`), everything it reports is read from the view model the core renders: nothing of the room is
// decided here. It takes commands from files in a folder and answers each in a file beside it, so that the
// orchestrator drives every client the same way:
//
//   node tests/client/mega/web-observer.mjs <folder> <tree>
//
// <tree> is the repository whose app/web/core/shell.ts and core/bindings/wasm/pkg are used: this branch, or a
// worktree of origin/main (the baseline). Commands: cmd-<n>.json → res-<n>.json, in order of n:
//   { op: "signin", hub, room_id, code, email, password }   a fresh device signs in to the account as the person
//                                             does (`AccountAction::SignIn`; the core joins the room with the
//                                             recovery code the account holds, spec 8.4); `code` is the iPhones'
//   { op: "sync" }                            catch up (`Event::Foreground`), then wait until the network is quiet
//   { op: "restart" }                         the page goes and comes back: a new core over the same stores
//   { op: "faults", fail, delay_ms, changes_down }   the hub misbehaves for this device (seeded)
//   { op: "answer", card, choices } | { op: "take_back", card } | { op: "verdict", request, allow }
//   { op: "snooze", card, until } | { op: "explain", card } | { op: "hand_back", card }
//   { op: "snapshot", ids? }                  what this device shows (normalized, see snapshot()); `ids`: cards and
//                                             requests to open beside those the Desk and the sessions name
//   { op: "stats", reset } | { op: "unsettled" } | { op: "trace", linger_ms }
//   { op: "quit" }
// For the torture run (tests/client/mega/torture.rs) besides:
//   { op: "signin", …, stream: true }          (the core follows the live stream whenever it is live: no difference)
//   { op: "shown" }                           the session list as the sidebar shows it (`view.sidebar.sessions` on
//                                             All desks, in order): session, name, online, parent; `agent` is null
//                                             (the view names no session's agent device)
//   { op: "presence_log", reset }             every change of a session's online dot seen here: [t, session, online]
//   { op: "invite_agent", label }             an agent invite: { invite_id, link }
//   { op: "confirm_invite", invite_id, die }  waits for the newcomer's Request and taps "They match"; die: the device
//                                             stops right after the Commit went out (a closed tab)
//   { op: "allow_join", allow, folder? }      REFUSED: the core shows no self-join asks (`view.account.asks` is never
//                                             filled), so a person on the web cannot Allow or Deny; the torture run
//                                             lets an iPhone answer
//   { op: "say", session, text }              a Chat message to a session
//   { op: "archive", session }                the person archives a session
import fs from 'node:fs'
import path from 'node:path'
import { pathToFileURL } from 'node:url'

const [dir, tree] = process.argv.slice(2)
const at = p => pathToFileURL(path.join(tree, p)).href
const { CoreShell, memorySpaces } = await import(at('app/web/core/shell.ts'))
const binding = await import(at('core/bindings/wasm/pkg/trommi-core.js'))
await binding.init(fs.readFileSync(path.join(tree, 'core/bindings/wasm/pkg/trommi_core_wasm_bg.wasm')))
const { foldView, emptyView } = await import(at('core/bindings/wasm/pkg/views-fold.js'))

// ---- tracing a first login (tests/client/mega/first-login.mjs): TROMMI_TRACE=1 keeps every request, every event the
// core handled, every store operation and samples of what the person would see, as spans
const trace = { on: false, spans: [], samples: [], t0: 0 }
const span = (lane, name, t, e, extra) => { if (trace.on && trace.spans.length < 600_000) trace.spans.push(extra ? [lane, name, t, e, extra] : [lane, name, t, e]) }
const mark = what => { const t = performance.now(); span('mark', what, t, t) }

// ---- the hub misbehaving, seeded; the requests counted; the network's quiet watched (sync)
let faults = { fail: 0, delay_ms: 0, changes_down: false, seed: 1 }
let rng = Number(process.env.MEGA_SEED ?? 1) || 1
const rand = () => { rng = (rng * 1103515245 + 12345) & 0x7fffffff; return rng / 0x7fffffff }
const realFetch = globalThis.fetch
const RTT_MS = Number(process.env.TRACE_RTT_MS ?? 0)
/** Requests by route (`GET /v1/welcomes`, …), for the load rules. */
const asked = new Map()
const routeOf = (method, url) => `${method} ${new URL(url).pathname.replace(/\/[A-Za-z0-9_-]{20,}/g, '/:id')}`
/** Requests (not the stream) in flight, and when one last began or ended. */
let flying = 0
let lastNet = 0
/** When the hub last answered a catch-up (`GET /v1/changes`). */
let lastCaughtUp = 0
/** Set by `confirm_invite` with `die`: told of each answer, to see the Commit go out. */
let onAnswer = null
/** The last requests and how they ended, for the log of a run that does not settle. */
const recent = []
const noted = (route, how) => { recent.push(`${new Date().toISOString().slice(11, 23)} ${route} ${how}`); if (recent.length > 12) recent.shift() }
const faultyFetch = async (input, init) => {
  const url = typeof input === 'string' ? input : input.url
  const method = init?.method ?? 'GET'
  const route = routeOf(method, url)
  asked.set(route, (asked.get(route) ?? 0) + 1)
  if (url.includes('/v1/stream')) {
    const t = performance.now()
    if (RTT_MS) await new Promise(r => setTimeout(r, RTT_MS))
    const res = await realFetch(input, init).catch(e => { noted(route, `failed ${e?.message ?? e}`); throw e })
    noted(route, `stream ${res.status}`)
    span('net', route, t, performance.now(), { status: res.status, query: new URL(url).search })
    return res
  }
  flying++
  lastNet = Date.now()
  try {
    if (faults.changes_down && url.includes('/v1/changes')) return noted(route, 'changes down'), new Response(JSON.stringify({ error: 'overloaded', message: 'mega: changes down' }), { status: 503, headers: { 'content-type': 'application/json', 'retry-after': '1' } })
    if (faults.delay_ms) await new Promise(r => setTimeout(r, Math.floor(rand() * faults.delay_ms)))
    // (a real network: TRACE_RTT_MS of round trip on every request)
    if (RTT_MS) await new Promise(r => setTimeout(r, RTT_MS))
    if (faults.fail && rand() < faults.fail) return noted(route, 'injected 502'), new Response(JSON.stringify({ error: 'unavailable', message: 'mega: injected' }), { status: 502, headers: { 'content-type': 'application/json' } })
    const t = performance.now()
    const res = await realFetch(input, init)
    // (the whole answer is read here, so that "in flight" ends when the core can have it)
    const body = new Uint8Array(await res.arrayBuffer())
    const sent = init?.body?.byteLength ?? (typeof init?.body === 'string' ? Buffer.byteLength(init.body) : 0)
    asked.set('bytes out', (asked.get('bytes out') ?? 0) + sent)
    asked.set('bytes in', (asked.get('bytes in') ?? 0) + body.byteLength)
    span('net', route, t, performance.now(), { in: body.byteLength, out: sent, status: res.status, query: new URL(url).search })
    noted(route, res.status)
    if (route === 'GET /v1/changes' && res.status === 200) lastCaughtUp = Date.now()
    onAnswer?.(method, url, res.status)
    const empty = [101, 204, 205, 304].includes(res.status)
    return new Response(empty ? null : body, { status: res.status, statusText: res.statusText, headers: res.headers })
  } finally {
    flying--
    lastNet = Date.now()
  }
}

// ---- the device: one AppCore, its shell, the view it renders
const config = { platform: 'web', version: 'mega', hub: '', seed: (Number(process.env.MEGA_SEED ?? 1) || 1) * 7919 + 1, utc_offset_min: 0 }
/** The device's state and the cache: kept across a restart, as IndexedDB keeps them across a reload. */
let spaces = null
let core = null
let shell = null
let view = emptyView()
/** The page's focus between commands: the Desk of every desk (the sidebar then lists every session). */
const DESK = { type: 'focus', focus: { screen: { screen: 'desk' }, visible: true, desk: 'all' } }
/** Every store operation's time, for a trace. */
function tracedSpaces(s) {
  const timed = (name, f) => async (...a) => { const t = performance.now(); try { return await f(...a) } finally { span('cache', name, t, performance.now()) } }
  return {
    device: { load: timed('store device_load', s.device.load), apply: timed('store device_apply', s.device.apply) },
    cache: { get: timed('cache get', s.cache.get), scan: timed('cache scan', s.cache.scan), put: timed('cache put', s.cache.put) },
  }
}
function open() {
  core = new binding.AppCore()
  const app = core
  const handled = {
    handle(nowMs, event) {
      if (!trace.on) return app.handle(nowMs, event)
      const t = performance.now()
      try { return app.handle(nowMs, event) } finally { span('model', `handle ${/"type":"([a-z_]+)"/.exec(event.json)?.[1] ?? '?'}`, t, performance.now()) }
    },
  }
  let phase = null, step = null
  view = emptyView()
  shell = new CoreShell({
    core: handled, ...(trace.on ? tracedSpaces(spaces) : spaces), fetch: faultyFetch,
    host: {
      render: delta => {
        view = foldView(view, delta)
        if (view.phase !== phase) { phase = view.phase; mark(`phase ${phase}`) }
        if (view.account?.step !== step) { step = view.account?.step; mark(`account ${step}`) }
      },
      log: (level, text) => { if (level === 'error' || level === 'warn') console.error(new Date().toISOString(), `core ${level}:`, text) },
    },
  })
  shell.send({ type: 'start', config })
  shell.send(DESK)
}
function close() {
  shell?.close()
  try { core?.close() } catch {}
  shell = null
  core = null
}
const act = action => shell.send({ type: 'action', action })
const sleep = ms => new Promise(r => setTimeout(r, ms))
const until = async (f, what, ms = 30_000) => {
  const end = Date.now() + ms
  while (Date.now() < end) { if (await f()) return; await sleep(25) }
  throw new Error(`timed out: ${what} (phase ${view.phase}, account ${JSON.stringify(view.account)})`)
}

/** Caught up: the page went to the back and came to the front again (`Event::Background`, `Event::Foreground`: the
 *  core closes its stream and, coming back without one, catches up), the hub answered that catch-up, the core is
 *  live again and no request has been in flight for a while. The core says nothing of "settled": this is the sign. */
async function settle({ quiet = 600, max = faults.changes_down ? 8_000 : 30_000 } = {}) {
  const end = Date.now() + max
  for (let round = 0; round < 2; round++) {
    const since = Date.now()
    shell.send({ type: 'background' })
    shell.send({ type: 'foreground' })
    await sleep(50)
    while (Date.now() < end && !(lastCaughtUp > since && view.phase === 'live' && flying === 0 && Date.now() - lastNet >= quiet)) await sleep(25)
  }
  if (Date.now() >= end && !faults.changes_down) console.error(new Date().toISOString(), 'not settled:', JSON.stringify(unsettled()))
}
const unsettled = () => ({ phase: view.phase, status: view.status?.en ?? null, flying, quiet_ms: Date.now() - lastNet, toasts: (view.toasts ?? []).map(t => t.text?.en ?? ''), recent: [...recent] })

/** An action, then caught up; a problem the core said with it (an alert toast) fails the command. */
async function deed(action) {
  const before = new Set((view.toasts ?? []).map(t => t.id))
  act(action)
  await settle()
  const said = (view.toasts ?? []).filter(t => t.alert && !before.has(t.id)).map(t => `${t.text?.en ?? ''}${t.line ? `: ${t.line.en}` : ''}`)
  if (said.length) throw new Error(`the core said: ${said.join(' | ')}`)
  return {}
}

// ---- what this device shows
/** One card's or request's page (`Screen::Card`): the core renders the card in view only. */
function page(id) {
  shell.send({ type: 'focus', focus: { screen: { screen: 'card', card_id: id }, visible: true, desk: 'all' } })
  return view.cards?.[id] ?? null
}
function ofSession(id) {
  shell.send({ type: 'focus', focus: { screen: { screen: 'session', session_id: id }, visible: true, desk: 'all' } })
  return view.sessions?.[id] ?? null
}
/** The answer that stands, as the agents hold it (`answer_action` and the choices), from the card's page. */
function answerOf(c) {
  if (c.state === 'open' || c.status === 'catching') return null
  if (c.status === 'shredded') return { action: 'shred', choices: [] }
  if (c.kind === 'info') return c.answer?.back === 'reopen' ? { action: 'read', choices: [] } : null
  const choices = (c.options ?? []).filter(o => o.chosen).map(o => o.key).sort()
  const duck = String(c.answer?.label?.key ?? '').startsWith('card.said.duck')
  return choices.length || duck ? { action: 'answer', choices } : null
}
/** A request's state as the agents hold it, from its page: pending, allowed, denied, expired, withdrawn. */
function requestState(c) {
  if (c.status === 'catching') return null
  if (c.state === 'open') return 'pending'
  const chosen = (c.options ?? []).find(o => o.chosen)?.key
  if (chosen === 'allow') return 'allowed'
  if (chosen === 'deny') return 'denied'
  if (c.answer?.summary === 'Answered in the terminal') return 'withdrawn'
  // ("Expired", or its time ran out on this device's clock)
  return 'expired'
}

function snapshot(given) {
  const d = view.desk ?? {}
  const rowKind = new Map((d.rows ?? []).map(r => [r.id, r.kind]))
  const ids = new Set([...(d.stack ?? []), ...(d.off_your_mind ?? []), ...(d.later ?? []), ...(d.done ?? []), ...(d.starred?.rows ?? []).map(r => r.id), ...(d.running?.rows ?? []).map(r => r.id), ...given])
  for (const r of view.sidebar?.sessions ?? []) for (const q of ofSession(r.id)?.questions ?? []) ids.add(q.id)
  const cards = {}, perms = {}, snooze_values = {}
  for (const id of [...ids].sort()) {
    const c = page(id)
    if (!c) continue
    const held = c.status === 'catching'
    if (c.kind === 'request' || rowKind.get(id) === 'request') {
      perms[id] = { state: requestState(c), tool: c.title?.startsWith('Approval: ') ? c.title.slice(10) || null : null, held, session: c.session_id ?? null }
      continue
    }
    // (`version`: how many versions stood before this one; `closed_how`: how a closed card was closed, from its page's
    // `status` in the model's words: decided "answered", "shredded", done "closed"; `in_revision`: handed back)
    const how = { decided: 'answered', shredded: 'shredded', done: 'closed' }[c.status] ?? null
    cards[id] = {
      state: c.state, title: c.answer?.mode === 'loading' ? null : c.title, version: c.history ?? 0, closed_how: c.state === 'open' ? null : how,
      in_revision: c.answer?.mode === 'revising' ? true : null, answer: answerOf(c), held, session: c.session_id ?? null,
    }
    if (c.snoozed_until != null) snooze_values[id] = c.snoozed_until
  }
  shell.send(DESK)
  const sessions = {}
  for (const r of view.sidebar?.sessions ?? []) sessions[r.id] = { agent_name: r.name?.en ?? null, parent: r.parent ?? null }
  return {
    desk: [...(view.desk?.stack ?? [])], permissions_open: (view.desk?.rows ?? []).filter(r => r.kind === 'request').map(r => r.id),
    cards, permissions: perms, sessions, snoozed: [...(view.desk?.later ?? [])].sort(), snooze_values,
    debug: { phase: view.phase, status: view.status?.en ?? null, toasts: (view.toasts ?? []).map(t => t.text?.en ?? '') },
  }
}
const shown = () => (view.sidebar?.sessions ?? []).map(r => ({ session: r.id, name: r.name?.en ?? null, online: !!r.online, agent: null, parent: r.parent ?? null }))
const myDevice = () => (view.settings?.devices ?? []).find(d => d.this_device)?.id ?? null

/** What the person would see now, for a trace: the Desk, Off your mind, the sessions, the agents online. */
function seen() {
  if (!shell) return null
  const d = view.desk ?? {}
  const rows = view.sidebar?.sessions ?? []
  let order = 0
  for (const id of d.stack ?? []) for (let i = 0; i < 8; i++) order = (Math.imul(order, 31) + id.charCodeAt(i)) >>> 0
  return {
    desk: (d.rows ?? []).filter(r => !r.loading).length, stack: d.stack?.length ?? 0, order, held: (d.rows ?? []).filter(r => r.loading).length,
    off: (d.off_your_mind?.length ?? 0) + (d.later?.length ?? 0) + (d.off?.closed ?? 0), closed: d.off?.closed ?? 0, working: d.running?.count ?? 0,
    snoozed: d.later?.length ?? 0, artifacts: view.artifacts?.length ?? 0, sessions_live: rows.length,
    sessions_named: rows.filter(r => r.name?.en !== 'Loading …').length, online: rows.filter(r => r.online).length, reading: !!d.reading,
  }
}
let sampler = null
function startTrace() {
  trace.spans = []; trace.samples = []; trace.on = true; trace.t0 = performance.now()
  let last = ''
  sampler = setInterval(() => { const v = seen(); if (!v) return; const k = JSON.stringify(v); if (k !== last) { last = k; trace.samples.push([performance.now(), v]) } }, 20)
}
function stopTrace() {
  clearInterval(sampler)
  trace.on = false
  // each card's state at the end, as far as the Desk names it: on the Desk (open), with its agent, closed
  const finalState = {}
  for (const id of view.desk?.done ?? []) finalState[id] = 'closed'
  for (const id of view.desk?.off_your_mind ?? []) finalState[id] = 'working'
  for (const id of view.desk?.stack ?? []) finalState[id] = 'open'
  return { platform: 'web', t0: trace.t0, spans: trace.spans, samples: trace.samples, events: [], final_state: finalState, groups: {}, final: seen(), requests: Object.fromEntries(asked) }
}

// ---- presence: every change of a session's online dot this device showed, by polling its view
let presenceLog = []
const presenceSeen = new Map()
setInterval(() => {
  if (!shell) return
  for (const r of view.sidebar?.sessions ?? []) {
    const on = !!r.online
    // (a change only: the first sighting of a session is no change)
    if (presenceSeen.has(r.id) && presenceSeen.get(r.id) !== on) presenceLog.push([Date.now(), r.id, on])
    presenceSeen.set(r.id, on)
  }
}, 50)

async function signIn(cmd) {
  if (cmd.trace) startTrace()
  config.hub = cmd.hub
  spaces = memorySpaces()
  open()
  await until(() => view.phase === 'signed_out', 'the sign-in page')
  // (the person tries again when the hub refused or failed: the injected faults hit the sign-in too)
  for (let tries = 1; ; tries++) {
    act({ do: 'account', action: { step: 'sign_in', email: cmd.email, password: cmd.password, hub: cmd.hub } })
    mark(`sign-in ${tries}`)
    await until(() => view.phase === 'live' || view.phase === 'opening' || view.account?.problem, 'the sign-in', 120_000)
    if (view.phase === 'live' || view.phase === 'opening') break
    if (tries >= 6) throw new Error(`the sign-in: ${view.account.problem.en}`)
    await sleep(view.account.problem.key === 'account.throttled' ? 2_100 : 500)
  }
  await until(() => view.phase === 'live', 'live after the sign-in', 180_000)
  await settle()
  mark('settled: fully live')
  return { device: myDevice(), not_joined: 0 }
}

async function run(cmd) {
  switch (cmd.op) {
    case 'signin': return signIn(cmd)
    case 'trace': { await sleep(cmd.linger_ms ?? 0); return stopTrace() }
    case 'sync': await settle(); return {}
    case 'restart': {
      close()
      open()
      await until(() => view.phase && view.phase !== 'starting', 'the reopened core', 60_000)
      return {}
    }
    case 'faults': faults = { ...faults, ...cmd }; rng = cmd.seed ?? rng; return {}
    case 'answer': return deed({ do: 'answer', card_id: cmd.card, choices: cmd.choices })
    case 'take_back': return deed({ do: 'decide_again', card_id: cmd.card })
    case 'verdict': return deed({ do: 'verdict', request_id: cmd.request, allow: !!cmd.allow })
    case 'snooze': return deed(cmd.until == null ? { do: 'unsnooze', card_id: cmd.card } : { do: 'snooze', card_id: cmd.card, until_ms: cmd.until })
    // What?? (explain it) and Revise (make it again): both hand the card back to its agent
    case 'explain': return deed({ do: 'hand_back', card_id: cmd.card, note: 'What??' })
    case 'hand_back': return deed({ do: 'hand_back', card_id: cmd.card, note: 'back to you', rework: true })
    case 'snapshot': return snapshot(cmd.ids ?? [])
    case 'stats': {
      const requests = Object.fromEntries([...asked].sort())
      if (cmd.reset) asked.clear()
      const d = view.desk ?? {}
      return { rss: process.memoryUsage().rss, heap: process.memoryUsage().heapUsed, desk: d.stack?.length ?? 0, cards: (d.stack?.length ?? 0) + (d.off_your_mind?.length ?? 0) + (d.later?.length ?? 0) + (d.off?.closed ?? 0), requests }
    }
    case 'shown': return shown()
    case 'unsettled': return unsettled()
    case 'presence_log': { const out = presenceLog; if (cmd.reset) presenceLog = []; return out }
    case 'invite_agent': {
      act({ do: 'account', action: { step: 'invite_agent', label: cmd.label ?? '' } })
      await until(() => view.account?.step === 'invite' && view.account.link && view.account.invite_id, 'the invite link', 60_000)
      return { invite_id: view.account.invite_id, link: view.account.link }
    }
    case 'confirm_invite': {
      const id = cmd.invite_id
      await until(() => view.account?.step === 'compare_emoji' && view.account.invite_id === id, `the Request to invite ${id}`, cmd.timeout_ms ?? 60_000)
      if (cmd.die) {
        // the tab is closed right after "They match": the Commit goes out, nothing after it
        // (the Commit that adds the newcomer: `POST /v1/groups/<room>/commits`, taken by the hub)
        const wrote = []
        // (closed at once, in the answer's own turn: the core hears of the Commit no more, as a closed tab)
        onAnswer = (method, url, status) => {
          if (method === 'GET' || status >= 300) return
          wrote.push(routeOf(method, url))
          if (wrote.at(-1).endsWith('/commits')) { onAnswer = null; close() }
        }
        act({ do: 'account', action: { step: 'confirm_emoji', invite_id: id, matches: true } })
        await until(() => wrote.some(r => r.endsWith('/commits')), 'the confirmation committed', 30_000)
        onAnswer = null
        close()
        return { agent: null, died: true, wrote }
      }
      act({ do: 'account', action: { step: 'confirm_emoji', invite_id: id, matches: true } })
      await until(() => view.account?.step !== 'compare_emoji', 'the newcomer added', 60_000)
      await settle()
      return { agent: null, state: view.account?.step ?? null }
    }
    case 'allow_join': throw Object.assign(new Error('the app core shows no self-join asks (view.account.asks is never filled): nothing to Allow or Deny on the web'), { code: 'unsupported' })
    case 'say': return deed({ do: 'write', session_id: cmd.session, text: cmd.text })
    case 'archive': return deed({ do: 'archive_session', session_id: cmd.session, on: true })
    case 'quit': close(); return {}
    default: throw new Error(`unknown op ${cmd.op}`)
  }
}

// (a call that hangs with nothing pending would end the process silently; this keeps it alive, so it times out)
setInterval(() => {}, 1 << 30)
process.on('unhandledRejection', e => console.error('unhandled', e))
fs.mkdirSync(dir, { recursive: true })
fs.writeFileSync(path.join(dir, 'ready'), String(process.pid))
for (let n = 0; ; n++) {
  const file = path.join(dir, `cmd-${n}.json`)
  while (!fs.existsSync(file)) await sleep(30)
  const cmd = JSON.parse(fs.readFileSync(file, 'utf8'))
  let res
  try { res = { ok: true, result: await run(cmd) } } catch (e) { res = { ok: false, error: String(e?.code ?? ''), message: String(e?.message ?? e) } }
  fs.writeFileSync(path.join(dir, `res-${n}.json.tmp`), JSON.stringify(res))
  fs.renameSync(path.join(dir, `res-${n}.json.tmp`), path.join(dir, `res-${n}.json`))
  if (cmd.op === 'quit') process.exit(0)
}
