// keys.test.mjs: what Settings → Security shows of the keys that open the account's data: when each device came in
// (Member.added_at, from the Commit that added it) and the room's recovery key (room.recovery_key: its fingerprint,
// and since when it is in force). Hand-made core results (factory.mjs).
//   node --test tests/web/model/
import test from 'node:test'
import assert from 'node:assert/strict'
import { cacheAll, keyFingerprint, modelFromCache } from '../../../app/web/core/model.ts'
import { World, device, hex, NOW } from './factory.mjs'

const key = (...tag) => { const k = new Uint8Array(32); k.set(tag); return k }
const DAY = 86_400_000

test('a device is dated by the room Commit that added it: an Add, a join with the recovery code, an enrolled agent', () => {
  const w = new World(); w.groups()
  assert.equal(w.model.members.get(hex(w.phone)).added_at, null, 'a device the lists already held when this device came: unknown')
  const tablet = device(3), stranger = device(4), agent = device(12)
  w.commit({ epoch: 3, adds: [tablet], time: NOW + DAY })
  assert.equal(w.model.members.get(hex(tablet)).added_at, NOW + DAY); assert.equal(w.model.members.get(hex(tablet)).device_role, 'human'); assert.ok(w.last.members)
  w.groups([], { humans: [w.me, w.phone, tablet], epoch: 4 })
  assert.equal(w.model.members.get(hex(tablet)).added_at, NOW + DAY, 'the lists keep the date')
  w.commit({ epoch: 4, committer: stranger, external: true, time: NOW + 2 * DAY })
  assert.equal(w.model.members.get(hex(stranger)).added_at, NOW + 2 * DAY)
  w.commit({ epoch: 5, agents: [w.agent, agent], time: NOW + 3 * DAY })
  assert.equal(w.model.members.get(hex(agent)).added_at, NOW + 3 * DAY); assert.equal(w.model.members.get(hex(agent)).device_role, 'agent')
  assert.equal(w.model.members.get(hex(w.agent)).added_at, null, 'an agent named again is not dated anew')
})

test('this device\'s own Commit, back after the lists were read anew, still dates the device it added; an older one is never re-dated', () => {
  const w = new World(); w.groups()
  const tablet = device(3)
  w.groups([], { humans: [w.me, w.phone, tablet], epoch: 4 })    // the lists first (the engine read them after posting)
  w.commit({ epoch: 3, adds: [tablet], time: NOW + 5 })              // then the Commit that began epoch 4
  assert.equal(w.model.members.get(hex(tablet)).added_at, NOW + 5)
  w.commit({ epoch: 6, adds: [tablet], time: NOW + 9 })
  assert.equal(w.model.members.get(hex(tablet)).added_at, NOW + 5)
  w.commit({ epoch: 6, adds: [w.phone], time: NOW + 9 })
  assert.equal(w.model.members.get(hex(w.phone)).added_at, null, 'a device older than the Commit is not dated by it')
})

test('a helper device is dated by the session Commit that added it', () => {
  const w = new World(); w.groups()
  const helper = device(20)
  w.commit({ group: w.session_group, epoch: 2, committer: w.agent, adds: [helper], time: NOW + 7 })
  assert.equal(w.model.members.get(hex(helper)).added_at, NOW + 7); assert.equal(w.model.members.get(hex(helper)).device_role, 'agent')
  w.commit({ group: w.session_group, epoch: 3, adds: [w.phone], time: NOW + 8 })
  assert.equal(w.model.members.get(hex(w.phone)).added_at, null, 'a human device added to a session is no new device')
})

test('the recovery key: its fingerprint; dated only when one room Commit replaced it', () => {
  const w = new World()
  w.groups([], { recovery: key(0xc4, 0x1e, 0x8b, 0x07) })
  assert.deepEqual(w.model.room.recovery_key, { fingerprint: 'c41e 8b07 0000 0000', since: null }, 'first known: since when is not known'); assert.ok(w.last.room)
  assert.equal(keyFingerprint(key(0xab, 0xcd)), 'abcd 0000 0000 0000')
  // the same key after a Commit: nothing changes
  w.commit({ epoch: 3, time: NOW + DAY }); w.groups([], { recovery: key(0xc4, 0x1e, 0x8b, 0x07), epoch: 4 })
  assert.equal(w.model.room.recovery_key.since, null)
  // replaced by one room Commit: dated by it
  w.commit({ epoch: 4, time: NOW + 2 * DAY }); w.groups([], { recovery: key(0x11), epoch: 5 })
  assert.deepEqual(w.model.room.recovery_key, { fingerprint: '1100 0000 0000 0000', since: NOW + 2 * DAY })
  // a catch-up of two room Commits: which one replaced it is not known
  w.commit({ epoch: 5, time: NOW + 3 * DAY }); w.commit({ epoch: 6, time: NOW + 4 * DAY }); w.groups([], { recovery: key(0x22), epoch: 7 })
  assert.deepEqual(w.model.room.recovery_key, { fingerprint: '2200 0000 0000 0000', since: null })
  // a session Commit counts for nothing; the lists read again without a Commit do not date a key either
  w.commit({ group: w.session_group, epoch: 2, time: NOW + 5 * DAY }); w.groups([], { recovery: key(0x33), epoch: 7 })
  assert.equal(w.model.room.recovery_key.since, null)
})

test('the warm start keeps both: the dates and the recovery key come back from the cache', () => {
  const w = new World(); w.groups([], { recovery: key(0x44) })
  const tablet = device(3)
  w.commit({ epoch: 3, adds: [tablet], time: NOW + 11 }); w.groups([], { humans: [w.me, w.phone, tablet], recovery: key(0x55), epoch: 4 })
  const warm = modelFromCache(cacheAll(w.model).map(([k, v]) => [k, JSON.parse(JSON.stringify(v ?? null))]))
  assert.equal(warm.members.get(hex(tablet)).added_at, NOW + 11)
  assert.deepEqual(warm.room.recovery_key, { fingerprint: '5500 0000 0000 0000', since: NOW + 11 })
  // a cache written before the field: unknown, never undefined
  const old = modelFromCache([['members', [{ ...w.model.members.get(hex(w.phone)), added_at: undefined }]]])
  assert.equal(old.members.get(hex(w.phone)).added_at, null)
})
