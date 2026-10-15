import test from 'node:test'
import assert from 'node:assert/strict'
import { CoreShell, memorySpaces } from '../../../app/web/core/shell.ts'

test('closing releases the WASM core once and drops late I/O results', async () => {
  let finish, freed = 0, calls = 0
  const spaces = memorySpaces()
  spaces.cache.get = () => new Promise(resolve => { finish = resolve })
  const core = {
    close() { freed++ },
    handle() { calls++; return { json:JSON.stringify([{ type:'store', id:1, op:{ op:'get', keys:['late'] } }]), blobs:[] } },
  }
  const shell = new CoreShell({ core, ...spaces, host:{ render() {} } })
  shell.send({ type:'start' })
  await new Promise(resolve => setImmediate(resolve))
  shell.close(); shell.close()
  finish([new Uint8Array(1024 * 1024)])
  await new Promise(resolve => setImmediate(resolve))
  assert.equal(freed, 1)
  assert.equal(calls, 1)
  assert.equal(shell.queue.length, 0, 'late cache bytes must not remain in an unpumped closed queue')
  assert.equal(shell.held.length, 0)
})
