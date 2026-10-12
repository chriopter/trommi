// A new agent let in by "They match" gets its main session at once, with its device as the session's seat (spec
// 5.2.2, 5.2.5), against the REAL hub and the REAL core (the WASM binding), in a room that already has a second
// human device and an agent. Runs when TROMMI_HUB_BIN names the hub's binary and core/wasm/pkg is built.
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnHub } from '../hub-process.mjs'
import { roomsOn } from '../../../app/web/core/room.ts'
import { hex } from '../../../app/web/core/ids.ts'
import { MemoryStore, memoryCache, storage, TIMING, until } from './helpers.mjs'

const BIN = process.env.TROMMI_HUB_BIN
const PKG = new URL('../../../core/wasm/pkg/trommi-core.js', import.meta.url)
const skip = !BIN ? 'TROMMI_HUB_BIN is not set' : !fs.existsSync(PKG) ? 'core/wasm/pkg is not built' : false

async function realRooms() {
  const binding = await import(PKG.href)
  await binding.init(fs.readFileSync(new URL('../../../core/wasm/pkg/trommi_core_wasm_bg.wasm', import.meta.url)))
  const { init: _i, Device, TrommiError, StoreConflict: _s, ...stateless } = binding
  const core = { ...stateless, createDevice: store => Device.create(store), openDevice: store => Device.open(store), errorCode: e => (e instanceof TrommiError ? e.code : null) }
  return roomsOn({ core: async () => core, store: name => new MemoryStore(name), cache: async name => memoryCache(name), destroy: async () => {}, timing: TIMING })
}

/** The main session `human` holds whose seat is `agent` (its one leaf that is no human device). */
const seatOf = (human, agent) => {
  const humans = new Set((human.engine.roles?.humans ?? []).map(hex))
  return human.engine.groups.find(g => g.session && !g.archived && !g.session.parent.some(b => b !== 0)
    && g.leaves.filter(l => !humans.has(hex(l))).map(hex).join() === agent)
}

async function admit(t, R, human, label) {
  const invite = await human.createInvite({ device_role: 'agent', app_url: 'https://app.example/join', label })
  const joining = R.joinRoom({ link: invite.link, storage: storage(label), poll_ms: 50, device_name: label })
  await joining.check_code
  await until(() => human.model.invites.get(invite.invite_id)?.invite_state === 'confirm_code', `${label}'s Request`)
  const confirmed = human.confirmInvite(invite.invite_id, true)
  const ac = await joining.client
  t.after(() => ac.stop().catch(() => {}))
  await ac.start()
  await Promise.race([confirmed, new Promise((_, no) => setTimeout(() => no(new Error(`"Adding the agent" never finished for ${label} (invite ${human.model.invites.get(invite.invite_id)?.invite_state})`)), 15000))])
  assert.equal(human.model.invites.get(invite.invite_id)?.invite_state, 'joined')
  assert.ok(seatOf(human, ac.my_device_id), `${label} is the seat of a main session on the inviter`)
  await until(async () => { await ac.catchUp?.().catch(() => {}); return ac.engine.groups.some(g => g.session && g.leaves.some(l => hex(l) === ac.my_device_id)) }, `${label} holds its main session`, 15000)
  return ac
}

test('real hub: after "They match" the human app founds the new agent\'s main session with the agent as its seat', { skip }, async t => {
  const data = fs.mkdtempSync(path.join(process.env.TMPDIR ?? os.tmpdir(), 'seat-hub-'))
  const hub = await spawnHub(BIN, { HUB_HOST: '127.0.0.1', HUB_DATA: data, HUB_QUIET: '1', HUB_LOGIN_THROTTLE: 'off', PATH: process.env.PATH ?? '' })
  t.after(() => hub.child.kill())
  await until(async () => (await fetch(`${hub.url}/healthz`).catch(() => null))?.ok, 'the hub up')
  const R = await realRooms()
  const { client: a, recovery_code } = await R.foundRoom({ storage: storage('a'), hub_url: hub.url, device_name: 'a' })
  t.after(() => a.stop().catch(() => {}))
  await a.start()
  await admit(t, R, a, 'first-agent')
  // the person's phone: in with the code, online, then gone with its KeyPackages left at the hub
  const b = await R.joinWithCode({ storage: storage('b'), hub_url: hub.url, room_id: a.model.room.room_id, code: recovery_code, device_name: 'b' })
  await b.sessions
  await b.client.start()
  await a.settle(); await b.client.settle?.()
  await admit(t, R, a, 'second-agent')
  await b.client.stop()
  await admit(t, R, a, 'third-agent')
})

// The same with the REAL connector (its binary in TROMMI_CONNECTOR_BIN): `trommi-connector join <link>` in a folder
// of its own, confirmed on the human device; the connector then names its session.
const CONNECTOR = process.env.TROMMI_CONNECTOR_BIN
test('real hub, real connector: "They match" gives the connector its main session', { skip: skip || (!CONNECTOR && 'TROMMI_CONNECTOR_BIN is not set') }, async t => {
  const { spawn } = await import('node:child_process')
  const data = fs.mkdtempSync(path.join(process.env.TMPDIR ?? os.tmpdir(), 'seat-hub-'))
  const hub = await spawnHub(BIN, { HUB_HOST: '127.0.0.1', HUB_DATA: data, HUB_QUIET: '1', HUB_LOGIN_THROTTLE: 'off', PATH: process.env.PATH ?? '' })
  t.after(() => hub.child.kill())
  await until(async () => (await fetch(`${hub.url}/healthz`).catch(() => null))?.ok, 'the hub up')
  const R = await realRooms()
  const { client: a, recovery_code } = await R.foundRoom({ storage: storage('a'), hub_url: hub.url, device_name: 'a' })
  t.after(() => a.stop().catch(() => {}))
  await a.start()
  const b = await R.joinWithCode({ storage: storage('b'), hub_url: hub.url, room_id: a.model.room.room_id, code: recovery_code, device_name: 'b' })
  await b.sessions
  await b.client.start()
  t.after(() => b.client.stop().catch(() => {}))
  for (const label of ['first', 'second']) {
    const invite = await a.createInvite({ device_role: 'agent', app_url: 'https://app.example/join', label })
    const home = fs.mkdtempSync(path.join(process.env.TMPDIR ?? os.tmpdir(), `seat-${label}-`))
    const folder = path.join(home, 'work'); fs.mkdirSync(folder)
    const child = spawn(CONNECTOR, ['join', invite.link], { cwd: folder, env: { PATH: process.env.PATH, HOME: home, TROMMI_LAUNCH: '0', XDG_CONFIG_HOME: path.join(home, '.config'), XDG_DATA_HOME: path.join(home, '.local/share'), XDG_STATE_HOME: path.join(home, '.local/state'), XDG_CACHE_HOME: path.join(home, '.cache') } })
    let out = ''
    child.stdout.on('data', d => { out += d }); child.stderr.on('data', d => { out += d })
    t.after(() => child.kill())
    const exited = new Promise(r => child.on('exit', code => r(code)))
    await until(() => a.model.invites.get(invite.invite_id)?.invite_state === 'confirm_code', `the connector's Request (${out})`, 20000)
    const confirmed = a.confirmInvite(invite.invite_id, true)
    await Promise.race([confirmed, new Promise((_, no) => setTimeout(() => no(new Error(`"Adding the agent" never finished (${a.model.invites.get(invite.invite_id)?.invite_state}); connector: ${out}`)), 30000))])
    const agent = a.model.invites.get(invite.invite_id).newcomer.device_id
    assert.ok(seatOf(a, agent), `the ${label} connector is the seat of a main session`)
    const code = await Promise.race([exited, new Promise(r => setTimeout(() => r('still running'), 30000))])
    assert.equal(code, 0, `the connector joined: ${out}`)
    assert.match(out, /session [0-9a-f]{32}/, `the connector names its session: ${out}`)
  }
})

/** A connector (CONNECTOR) joining by `link` in a folder of its own: its exit code and everything it said. */
async function connectorJoin(t, link, label) {
  const { spawn } = await import('node:child_process')
  const home = fs.mkdtempSync(path.join(process.env.TMPDIR ?? os.tmpdir(), `seat-${label}-`))
  const folder = path.join(home, 'work'); fs.mkdirSync(folder)
  const child = spawn(CONNECTOR, ['join', link], { cwd: folder, env: { PATH: process.env.PATH, HOME: home, TROMMI_LAUNCH: '0', XDG_CONFIG_HOME: path.join(home, '.config'), XDG_DATA_HOME: path.join(home, '.local/share'), XDG_STATE_HOME: path.join(home, '.local/state'), XDG_CACHE_HOME: path.join(home, '.cache') } })
  const said = { out: '' }
  child.stdout.on('data', d => { said.out += d }); child.stderr.on('data', d => { said.out += d })
  t.after(() => child.kill())
  said.exited = new Promise(r => child.on('exit', code => r(code)))
  return said
}

// The inviter confirms and is gone before it founds the session (an iPhone sent to the background right after
// "They match", a tab closed): the connector is in the room with no session and waited for one forever ("not yet
// assigned to a session"). Another human device of the room, connected, founds it with the agent as its seat.
test('real hub, real connector: an inviter gone after "They match" leaves the founding to another human device', { skip: skip || (!CONNECTOR && 'TROMMI_CONNECTOR_BIN is not set') }, async t => {
  const data = fs.mkdtempSync(path.join(process.env.TMPDIR ?? os.tmpdir(), 'seat-hub-'))
  const hub = await spawnHub(BIN, { HUB_HOST: '127.0.0.1', HUB_DATA: data, HUB_QUIET: '1', HUB_LOGIN_THROTTLE: 'off', PATH: process.env.PATH ?? '' })
  t.after(() => hub.child.kill())
  await until(async () => (await fetch(`${hub.url}/healthz`).catch(() => null))?.ok, 'the hub up')
  const R = await realRooms()
  const { client: a, recovery_code } = await R.foundRoom({ storage: storage('a'), hub_url: hub.url, device_name: 'a' })
  t.after(() => a.stop().catch(() => {}))
  await a.start()
  const phone = await R.joinWithCode({ storage: storage('phone'), hub_url: hub.url, room_id: a.model.room.room_id, code: recovery_code, device_name: 'phone' })
  await phone.sessions
  const b = phone.client
  t.after(() => b.stop().catch(() => {}))
  await b.start()
  await a.settle()
  const invite = await b.createInvite({ device_role: 'agent', app_url: 'https://app.example/join' })
  const said = await connectorJoin(t, invite.link, 'gone')
  await until(() => b.model.invites.get(invite.invite_id)?.invite_state === 'confirm_code', `the connector's Request (${said.out})`, 20000)
  // the phone commits the agent into the room and does nothing after: its upkeep never runs again
  b.engine.keepSoon = () => {}
  b.confirmInvite(invite.invite_id, true).catch(() => {})
  await until(() => b.model.invites.get(invite.invite_id)?.invite_state === 'adding', 'the confirmation committed')
  const agent = b.model.invites.get(invite.invite_id).newcomer.device_id
  await until(async () => (a.engine.roles?.agents ?? []).some(x => hex(x) === agent), 'the agent in the room, seen on a', 15000)
  await b.stop()
  await until(async () => !!seatOf(a, agent), `a founds the agent's main session with the agent as its seat (${said.out})`, 30000)
  const code = await Promise.race([said.exited, new Promise(r => setTimeout(() => r('still waiting'), 30000))])
  assert.equal(code, 0, `the connector got its session: ${said.out}`)
  assert.match(said.out, /session [0-9a-f]{32}/)
  assert.equal(a.engine.groups.filter(g => g.session && !g.archived && g.leaves.some(l => hex(l) === agent)).length, 1, 'one main session, not two')
})
