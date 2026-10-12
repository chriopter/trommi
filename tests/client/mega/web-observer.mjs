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
import fs from 'node:fs'
import path from 'node:path'
import { pathToFileURL } from 'node:url'

const [dir, tree] = process.argv.slice(2)
const at = p => pathToFileURL(path.join(tree, p)).href
const { roomsOn } = await import(at('app/web/core/room.ts'))
const { MemoryStore, memoryCache, TIMING } = await import(at('tests/web/client/helpers.mjs'))
const binding = await import(at('core/wasm/pkg/trommi-core.js'))
await binding.init(fs.readFileSync(path.join(tree, 'core/wasm/pkg/trommi_core_wasm_bg.wasm')))
const { init: _i, Device, TrommiError, StoreConflict: _s, ...stateless } = binding
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
const settle = async () => {
  for (let i = 0; i < 3; i++) {
    await client.catchUp().catch(() => {})
    await client.settle({ timeout_ms: 20_000 }).catch(() => {})
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
    sessions[s.session_id] = { agent_name: s.profile?.agent_name ?? null, parent: s.parent_session_id ?? null }
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
      await client.start({ stream: false })
      await settle()
      return { device: client.my_device_id, not_joined: joins.not_joined.length }
    }
    case 'sync': await settle(); return {}
    case 'restart': {
      await client.stop()
      client = await R.openRoom({ storage, fetch: faultyFetch })
      await client.start({ stream: false })
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
