// The web app's client layer (app/web/core: room.ts, engine.ts, client.ts, model.ts) with the REAL core (core/wasm/pkg)
// as one human device of the mega test (tests/client/mega.rs). It takes commands from files in a folder and answers
// each in a file beside it, so that the orchestrator drives every client the same way:
//
//   node tests/client/mega/web-observer.mjs <folder> <tree>
//
// <tree> is the repository whose app/web/core and core/wasm/pkg are used: this branch, or a worktree of origin/main
// (the baseline). Commands: cmd-<n>.json → res-<n>.json, in order of n:
//   { op: "signin", hub, room_id, code }      a fresh device signs in with the recovery code (spec 8.4), with history
//   { op: "sync" }                            catch up, read the hub's Desk, settle
//   { op: "restart" }                         the page goes and comes back: stop, open from the stores, start
//   { op: "faults", fail, delay_ms, changes_down }   the hub misbehaves for this device (seeded)
//   { op: "answer", card, choices } | { op: "take_back", card } | { op: "verdict", request, allow }
//   { op: "snooze", card, until } | { op: "explain", card } | { op: "hand_back", card }
//   { op: "snapshot" }                        what this device shows (normalized, see snapshot())
//   { op: "quit" }
// For the torture run (tests/client/mega/torture.rs) besides:
//   { op: "signin", …, stream: true }          the device follows the live stream (presence, live envelopes)
//   { op: "shown" }                           the session list as the sidebar shows it (app.mjs BoardState.agents,
//                                             not archived, in order): id, name, online, parent
//   { op: "presence_log", reset }             every change of an agent device's online state seen here: [t, device, online]
//   { op: "invite_agent", label }             an agent invite: { invite_id, link }
//   { op: "confirm_invite", invite_id, die }  waits for the newcomer's Request and taps "They match"; die: the device
//                                             stops right after the Commit, before it founds anything (a closed tab)
//   { op: "allow_join", allow, folder? }      answers the first self-join ask of the room (of that folder) (Allow or Deny)
//   { op: "say", session, text }              a Chat message to a session
//   { op: "archive", session }                the person archives a session
import fs from 'node:fs'
import path from 'node:path'
import { pathToFileURL } from 'node:url'

const [dir, tree] = process.argv.slice(2)
const at = p => pathToFileURL(path.join(tree, p)).href
const { roomsOn } = await import(at('app/web/core/room.ts'))
const { MemoryStore, memoryCache, TIMING } = await import(at('tests/web/client/helpers.mjs'))
const binding = await import(at('core/wasm/pkg/trommi-core.js'))
await binding.init(fs.readFileSync(path.join(tree, 'core/wasm/pkg/trommi_core_wasm_bg.wasm')))
// (a tree whose web core takes the binding through codec.ts, e.g. the shared client core: given it here too)
await import(at('app/web/core/codec.ts')).then(m => m.useCodec?.(binding), () => {})
const { init: _i, Device, TrommiError, StoreConflict: _s, ...stateless } = binding
// (the body codec is the core's: it is handed the binding, as core-wasm.ts does in the app)
await import(at('app/web/core/codec.ts')).then(m => m.useCodec?.(binding), () => {})
const core = { ...stateless, createDevice: s => Device.create(s), openDevice: s => Device.open(s), errorCode: e => (e instanceof TrommiError ? e.code : null) }
const R = roomsOn({ core: async () => core, store: name => new MemoryStore(name), cache: async name => memoryCache(name), destroy: async () => {}, timing: TIMING })

// ---- the hub misbehaving, seeded
let faults = { fail: 0, delay_ms: 0, changes_down: false, seed: 1 }
let rng = 1
const rand = () => { rng = (rng * 1103515245 + 12345) & 0x7fffffff; return rng / 0x7fffffff }
const realFetch = globalThis.fetch
/** Requests by route (`GET /v1/welcomes`, …), for the load rules. */
const asked = new Map()
const faultyFetch = async (input, init) => {
  const url = typeof input === 'string' ? input : input.url
  { const route = `${init?.method ?? 'GET'} ${new URL(url).pathname.replace(/\/[A-Za-z0-9_-]{20,}/g, '/:id')}`; asked.set(route, (asked.get(route) ?? 0) + 1) }
  if (faults.changes_down && url.includes('/v1/changes')) return new Response(JSON.stringify({ error: 'overloaded', message: 'mega: changes down' }), { status: 503, headers: { 'content-type': 'application/json', 'retry-after': '1' } })
  if (faults.delay_ms) await new Promise(r => setTimeout(r, Math.floor(rand() * faults.delay_ms)))
  if (faults.fail && rand() < faults.fail && !url.includes('/stream')) return new Response(JSON.stringify({ error: 'unavailable', message: 'mega: injected' }), { status: 502, headers: { 'content-type': 'application/json' } })
  const res = await realFetch(input, init)
  // (payload bytes both ways, as the iOS observer counts them; the live stream is left out)
  const sent = typeof init?.body === 'string' ? Buffer.byteLength(init.body) : init?.body?.byteLength ?? 0
  asked.set('bytes out', (asked.get('bytes out') ?? 0) + sent)
  if (!url.includes('/stream')) res.clone().arrayBuffer().then(b => asked.set('bytes in', (asked.get('bytes in') ?? 0) + b.byteLength), () => {})
  return res
}

const storage = { name: `mega-web-${path.basename(dir)}` }
let client = null
let streaming = false
// (the sidebar's own projection, when this tree has it: app/web/public/app.mjs)
const BoardState = await import(at('app/web/public/app.mjs')).then(m => m.BoardState, () => null)
// every change of an agent's online state this device saw, by polling its model
let presenceLog = []
const presenceSeen = new Map()
setInterval(() => {
  const m = client?.model
  if (!m) return
  for (const d of m.members.values()) {
    if (d.device_role !== 'agent' || !d.is_active) continue
    const on = !!d.is_online
    // (a change only: the first sighting of a device is no change, it may come before its first presence word)
    if (presenceSeen.has(d.device_id) && presenceSeen.get(d.device_id) !== on) presenceLog.push([Date.now(), d.device_id, on])
    presenceSeen.set(d.device_id, on)
  }
}, 50)
function shown() {
  if (!BoardState) return null
  const agents = new BoardState({ model: client.model }).update().agents
  return agents.filter(a => !a.archived).map(a => ({ session: a.session_id ?? a.device_id, name: a.name, online: !!a.online, agent: a.agent_device_id ?? null, parent: a.parent ? (agents.find(b => b.id === a.parent)?.session_id ?? a.parent) : null }))
}
const until = async (f, what, ms = 30_000) => { const end = Date.now() + ms; while (Date.now() < end) { if (await f()) return; await new Promise(r => setTimeout(r, 50)) } throw new Error(`timed out: ${what}`) }
const invites = new Map()
/** What keeps the engine from settling (for the log of a run that waits): its own flags and the outbox. */
const unsettled = async () => {
  const e = client?.engine
  if (!e) return null
  const outbox = await e.outbox().catch(() => [])
  return { owing: e.owing, rescanDue: e.rescanDue, learnDue: e.learnDue, keeping: !!e.keeping, blocked: e.blocked?.code ?? null, outbox: outbox.length, alerts: client.model.alerts.slice(-5).map(a => `${a.code}: ${a.message}`) }
}
const settle = async () => {
  for (let i = 0; i < 3; i++) {
    await client.catchUp().catch(() => {})
    await client.settle({ timeout_ms: 20_000 }).catch(async e => console.error(new Date().toISOString(), 'not settled:', String(e?.message ?? e), JSON.stringify(await unsettled())))
  }
  // the hub's Desk is read after every catch-up (client.ts readDesk): give the timer its turn
  await new Promise(r => setTimeout(r, 400))
  await client.settle({ timeout_ms: 20_000 }).catch(() => {})
}

function snapshot() {
  const m = client.model
  const cards = {}, perms = {}, sessions = {}
  for (const c of m.cards.values()) {
    cards[c.object_id] = {
      state: c.object_state, title: c.content_state === 'ok' ? c.title : null, version: c.object_version, closed_how: c.closed_how ?? null,
      in_revision: c.in_revision?.by ?? null, answer: c.answer ? { action: c.answer.answer_action, choices: [...c.answer.choices].sort() } : null,
      held: !!c.catching_up, session: c.session_id,
    }
  }
  for (const p of m.permissions.values()) {
    perms[p.object_id] = { state: p.permission_state, tool: p.tool_name || null, held: !!p.catching_up, session: p.session_id }
  }
  for (const s of m.sessions.values()) {
    if (!s.group_id) continue
    // (an archived session's content, its name too, loads only when it is opened: no name to compare)
    const archived = !!(s.settings?.archived || s.group_archived)
    sessions[s.session_id] = { agent_name: archived ? null : s.profile?.agent_name ?? null, parent: s.parent_session_id ?? null }
  }
  const snoozed = [...(m.human?.snoozes?.keys?.() ?? [])].sort()
  const snooze_values = Object.fromEntries([...(m.human?.snoozes ?? [])])
  return { desk: [...m.stack], permissions_open: [...m.open_permission_ids], cards, permissions: perms, sessions, snoozed, snooze_values, debug: { alerts: m.alerts.map(a => `${a.code}: ${a.message}`) } }
}

const card = id => client.model.cards.get(id)
async function run(cmd) {
  switch (cmd.op) {
    case 'signin': {
      const code = Uint8Array.from(Buffer.from(cmd.code, 'hex'))
      const { client: c, sessions } = await R.joinWithCode({ storage, hub_url: cmd.hub, room_id: cmd.room_id, code, device_name: `web ${path.basename(dir)}`, fetch: faultyFetch })
      const joins = await sessions
      client = c
      await client.start({ stream: !!cmd.stream })
      streaming = !!cmd.stream
      await settle()
      return { device: client.my_device_id, not_joined: joins.not_joined.length }
    }
    case 'sync': await settle(); return {}
    case 'restart': {
      await client.stop()
      client = await R.openRoom({ storage, fetch: faultyFetch })
      await client.start({ stream: streaming })
      return {}
    }
    case 'faults': faults = { ...faults, ...cmd }; rng = cmd.seed ?? rng; return {}
    case 'answer': await client.answer({ object_id: cmd.card, choices: cmd.choices }); await settle(); return {}
    case 'take_back': await client.decideAgain({ object_id: cmd.card }); await settle(); return {}
    case 'verdict': await client.verdict({ object_id: cmd.request, allow: cmd.allow }); await settle(); return {}
    case 'snooze': await client.setRegisters({ [`snooze/${cmd.card}`]: cmd.until == null ? null : { until: cmd.until } }); await settle(); return {}
    case 'explain': await client.sendMessage({ object_id: cmd.card, session_id: card(cmd.card)?.session_id ?? null, text: 'What??', explain: true }); await settle(); return {}
    case 'hand_back': await client.sendMessage({ object_id: cmd.card, session_id: card(cmd.card)?.session_id ?? null, text: 'back to you', hand_back: true }); await settle(); return {}
    case 'snapshot': return snapshot()
    case 'stats': { const m = client?.model; const requests = Object.fromEntries([...asked].sort()); if (cmd.reset) asked.clear(); return { rss: process.memoryUsage().rss, heap: process.memoryUsage().heapUsed, desk: m?.stack.length ?? 0, cards: m?.cards.size ?? 0, requests } }
    case 'shown': return shown()
    case 'unsettled': return unsettled()
    case 'presence_log': { const out = presenceLog; if (cmd.reset) presenceLog = []; return out }
    case 'invite_agent': {
      const inv = await client.createInvite({ device_role: 'agent', app_url: 'https://app.example/join', label: cmd.label ?? null })
      invites.set(inv.invite_id, inv)
      return { invite_id: inv.invite_id, link: inv.link }
    }
    case 'confirm_invite': {
      const id = cmd.invite_id
      await until(() => client.model.invites.get(id)?.invite_state === 'confirm_code', `the Request to invite ${id}`, cmd.timeout_ms ?? 60_000)
      if (cmd.die) {
        // the tab is closed right after "They match": the Commit goes out, nothing after it
        client.engine.keepSoon = () => {}
        client.confirmInvite(id, true).catch(() => {})
        await until(() => ['adding', 'joined'].includes(client.model.invites.get(id)?.invite_state), 'the confirmation committed')
        const agent = client.model.invites.get(id)?.newcomer?.device_id ?? null
        await until(() => (client.engine.roles?.agents ?? []).some(x => Buffer.from(x).toString('hex') === agent), 'the agent in the room', 20_000).catch(() => {})
        await client.stop().catch(() => {})
        client = null
        return { agent, died: true }
      }
      await client.confirmInvite(id, true)
      await settle()
      return { agent: client.model.invites.get(id)?.newcomer?.device_id ?? null, state: client.model.invites.get(id)?.invite_state }
    }
    case 'allow_join': {
      let nonce = null
      await until(() => {
        // (an ask that has its answer, `join_link/<nonce>`, Allow or Deny, is an earlier one: never answered again)
        // (`folder`: only the ask of the session in a folder of that name; another one's late ask is not answered for it)
        const mine = v => { const f = String(v?.value?.folder ?? ''); return !cmd.folder || f === cmd.folder || f.endsWith(`/${cmd.folder}`) }
        for (const s of client.model.sessions.values()) for (const [k, v] of s.registers ?? []) if (k.startsWith('join_ask/') && v?.value != null && mine(v) && s.registers.get(`join_link/${k.slice(9)}`)?.value == null) { nonce = k.slice(9); return true }
        return false
      }, 'a self-join ask', cmd.timeout_ms ?? 60_000)
      await client.answerVouch(nonce, !!cmd.allow)
      return { nonce }
    }
    case 'say': await client.sendMessage({ session_id: cmd.session, text: cmd.text }); return {}
    case 'archive': {
      const had = client.model.human.session_settings.get(cmd.session) ?? {}
      await client.setRegisters({ [`session/${cmd.session}`]: { ...had, archived: true } })
      await settle()
      return {}
    }
    case 'quit': await client?.stop().catch(() => {}); return {}
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
  while (!fs.existsSync(file)) await new Promise(r => setTimeout(r, 30))
  const cmd = JSON.parse(fs.readFileSync(file, 'utf8'))
  let res
  try { res = { ok: true, result: await run(cmd) } } catch (e) { res = { ok: false, error: String(e?.code ?? ''), message: String(e?.message ?? e) } }
  fs.writeFileSync(path.join(dir, `res-${n}.json.tmp`), JSON.stringify(res))
  fs.renameSync(path.join(dir, `res-${n}.json.tmp`), path.join(dir, `res-${n}.json`))
  if (cmd.op === 'quit') process.exit(0)
}
