// app/web/core/shell.ts: a platform op of the core goes to the page (host.platform), its answer back as
// Event::Platform with the bytes by index; without the page's service, or when it fails, the core hears so.
import test from 'node:test'
import assert from 'node:assert/strict'
import { CoreShell, memorySpaces } from '../../../app/web/core/shell.ts'

/** A core that asks for one platform op at start and keeps every event it is handed. */
function fakeCore(op, blobs) {
  const seen = []
  return {
    seen,
    handle(_now, packet) {
      const event = JSON.parse(packet.json)
      seen.push({ event, blobs: packet.blobs })
      if (event.type === 'start') return { json: JSON.stringify([{ type: 'platform', id: 7, op }]), blobs }
      return { json: '[]', blobs: [] }
    },
  }
}
const settle = () => new Promise(r => setTimeout(r, 10))

test('a passkey op goes to the page and its answer back with its bytes', async () => {
  const core = fakeCore({ op: 'passkey_get', rp_id: 'app.trommi.com', challenge: 0, credential_ids: [], prf_input: 1 }, [new Uint8Array([1, 2]), new Uint8Array([3])])
  let asked = null
  const shell = new CoreShell({
    core, ...memorySpaces(),
    host: {
      render() {},
      async platform(op, blobs) {
        asked = { op, challenge: [...blobs[op.challenge]], prf: [...blobs[op.prf_input]] }
        return { answer: { answer: 'passkey_asserted', credential_id: 0, authenticator_data: 1, client_data_json: 2, signature: 3, prf: 4 }, blobs: [1, 2, 3, 4, 5].map(n => new Uint8Array([n])) }
      },
    },
  })
  shell.send({ type: 'start', config: {} })
  await settle()
  assert.deepEqual(asked, { op: { op: 'passkey_get', rp_id: 'app.trommi.com', challenge: 0, credential_ids: [], prf_input: 1 }, challenge: [1, 2], prf: [3] })
  const back = core.seen.find(s => s.event.type === 'platform')
  assert.equal(back.event.id, 7)
  assert.equal(back.event.result.ok.answer, 'passkey_asserted')
  assert.deepEqual([...back.blobs[back.event.result.ok.prf]], [5])
})

test('without the page\'s service the op is refused; a failing service is a failure', async () => {
  const none = fakeCore({ op: 'push_unregister' }, [])
  new CoreShell({ core: none, ...memorySpaces(), host: { render() {} } }).send({ type: 'start', config: {} })
  await settle()
  assert.deepEqual(none.seen.find(s => s.event.type === 'platform').event.result, { ok: { answer: 'refused' } })

  const broken = fakeCore({ op: 'push_register' }, [])
  new CoreShell({ core: broken, ...memorySpaces(), host: { render() {}, platform: async () => { throw new Error('no service worker') } } }).send({ type: 'start', config: {} })
  await settle()
  assert.equal(broken.seen.find(s => s.event.type === 'platform').event.result.err.kind, 'failed')
})
