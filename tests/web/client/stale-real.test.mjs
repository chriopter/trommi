// The stale session of 5.2.8 against the REAL hub and the REAL core (the WASM binding): a device signs in with the
// code, joins the sessions and never comes (a login that hung); it is removed from the room; the session it was in
// waits for a Remove, and a human device must commit it so that a message goes out. Runs when TROMMI_HUB_BIN names
// the hub's binary and core/bindings/wasm/pkg is built; skipped and said otherwise.
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnHub } from '../hub-process.mjs'
import { roomsOn } from '../../../app/web/core/room.ts'
import { AgentStandIn } from '../stand-in/agent.ts'
import { MemoryStore, memoryCache, storage, TIMING, until } from './helpers.mjs'

const BIN = process.env.TROMMI_HUB_BIN
const PKG = new URL('../../../core/bindings/wasm/pkg/trommi-core.js', import.meta.url)
const skip = !BIN ? 'TROMMI_HUB_BIN is not set' : !fs.existsSync(PKG) ? 'core/bindings/wasm/pkg is not built' : false

async function realRooms() {
  const binding = await import(PKG.href)
  await binding.init(fs.readFileSync(new URL('../../../core/bindings/wasm/pkg/trommi_core_wasm_bg.wasm', import.meta.url)))
  const { init: _i, Device, TrommiError, StoreConflict: _s, ...stateless } = binding
  const core = { ...stateless, createDevice: store => Device.create(store), openDevice: store => Device.open(store), errorCode: e => (e instanceof TrommiError ? e.code : null) }
  return roomsOn({ core: async () => core, store: name => new MemoryStore(name), cache: async name => memoryCache(name), destroy: async () => {}, timing: TIMING })
}

test('real hub: a ghost device removed from the room leaves its session stale; this device repairs it and its message goes out', { skip }, async t => {
  const data = fs.mkdtempSync(path.join(process.env.TMPDIR ?? os.tmpdir(), 'stale-hub-'))
  const hub = await spawnHub(BIN, { HUB_HOST: '127.0.0.1', HUB_DATA: data, HUB_QUIET: '1', HUB_LOGIN_THROTTLE: 'off', PATH: process.env.PATH ?? '' })
  t.after(() => hub.child.kill())
  await until(async () => (await fetch(`${hub.url}/healthz`).catch(() => null))?.ok, 'the hub up')
  const R = await realRooms()
  const { client: a, recovery_code } = await R.foundRoom({ storage: storage('a'), hub_url: hub.url, device_name: 'a' })
  t.after(() => a.stop().catch(() => {}))
  await a.start()
  // an agent in by link: its main session holds a and the agent
  const invite = await a.createInvite({ device_role: 'agent', app_url: 'https://app.example/join' })
  const joining = R.joinRoom({ link: invite.link, storage: storage('agent'), poll_ms: 50 })
  await joining.check_code
  await until(() => a.model.invites.get(invite.invite_id)?.invite_state === 'confirm_code', 'the agent\'s Request')
  await a.confirmInvite(invite.invite_id, true)
  const ac = await joining.client
  t.after(() => ac.stop().catch(() => {}))
  const agent = new AgentStandIn(ac)
  await ac.start(); await a.settle()
  await until(() => [...a.model.sessions.values()].some(s => s.group_id), 'the session on a')
  const session = agent.session_id
  // the ghost: in with the code, in the session, then gone
  const ghost = await R.joinWithCode({ storage: storage('ghost'), hub_url: hub.url, room_id: a.model.room.room_id, code: recovery_code })
  const joins = await ghost.sessions
  assert.deepEqual(joins.not_joined, [], 'the ghost joined the session')
  const g = ghost.client.my_device_id
  await ghost.client.stop()
  await a.settle()
  await a.removeDevices([g])
  await a.settle()
  const alerts = () => a.model.alerts.map(x => x.code)
  await a.sendMessage({ session_id: session, text: 'after the ghost' })
  await a.settle()
  await until(async () => { await ac.catchUp?.().catch(() => {}); return [...(ac.model.timelines.get(`chat:session/${session}`)?.items.values() ?? [])].some(i => i.content?.text === 'after the ghost') }, `the agent reads it (a's alerts: ${alerts().join(', ')})`, 20000)
})

test('real hub: a message sealed before this device heard of the removal is held by the hub as stale; it is repaired and the same words go again', { skip }, async t => {
  const data = fs.mkdtempSync(path.join(process.env.TMPDIR ?? os.tmpdir(), 'stale-hub-'))
  const hub = await spawnHub(BIN, { HUB_HOST: '127.0.0.1', HUB_DATA: data, HUB_QUIET: '1', HUB_LOGIN_THROTTLE: 'off', PATH: process.env.PATH ?? '' })
  t.after(() => hub.child.kill())
  await until(async () => (await fetch(`${hub.url}/healthz`).catch(() => null))?.ok, 'the hub up')
  const R = await realRooms()
  const name = storage('a')
  const { client: a, recovery_code } = await R.foundRoom({ storage: name, hub_url: hub.url, device_name: 'a' })
  await a.start()
  const invite = await a.createInvite({ device_role: 'agent', app_url: 'https://app.example/join' })
  const joining = R.joinRoom({ link: invite.link, storage: storage('agent'), poll_ms: 50 })
  await joining.check_code
  await until(() => a.model.invites.get(invite.invite_id)?.invite_state === 'confirm_code', 'the agent\'s Request')
  await a.confirmInvite(invite.invite_id, true)
  const ac = await joining.client
  t.after(() => ac.stop().catch(() => {}))
  const agent = new AgentStandIn(ac)
  await ac.start(); await a.settle()
  await until(() => [...a.model.sessions.values()].some(s => s.group_id), 'the session on a')
  const session = agent.session_id
  // b: a second human device of the person (the phone); the ghost: in with the code and the session, then gone
  const b = (await R.joinWithCode({ storage: storage('b'), hub_url: hub.url, room_id: a.model.room.room_id, code: recovery_code, device_name: 'b' }))
  await b.sessions
  await b.client.start()
  const ghost = await R.joinWithCode({ storage: storage('ghost'), hub_url: hub.url, room_id: a.model.room.room_id, code: recovery_code })
  assert.deepEqual((await ghost.sessions).not_joined, [])
  const g = ghost.client.my_device_id
  await ghost.client.stop()
  await a.settle()
  await a.stop()
  // b removes the ghost from the room and is gone before it repairs the session
  await b.client.catchUp()
  const room = b.client.core.roomGroupId(b.client.room_id)
  await b.client.engine.land(async d => d.removeHumanDevices([await d.cutOf(room, Buffer.from(g, 'hex'))], Date.now()))
  await b.client.stop()
  // a comes back: its message is sealed before it read the removal
  const again = await R.openRoom({ storage: name, hub_url: hub.url })
  t.after(() => again.stop().catch(() => {}))
  const sent = again.sendMessage({ session_id: session, text: 'sealed before the removal' })
  await again.start()
  await sent
  await until(async () => { await ac.catchUp?.().catch(() => {}); return [...(ac.model.timelines.get(`chat:session/${session}`)?.items.values() ?? [])].some(i => i.content?.text === 'sealed before the removal') }, `the agent reads it (blocked: ${again.engine.blocked?.code ?? '-'}, alerts: ${again.model.alerts.map(x => x.code).join(', ')})`, 20000)
})
