// app/web/core/shell.ts: a request's `timeout_ms` bounds the wait for the answer, of a stream only the wait for its
// head (core/client/src/app/io.rs, as trommi-io's executor): an open stream is never cut by the clock, and an answer
// read in time is not aborted afterwards.
import test from 'node:test'
import assert from 'node:assert/strict'
import { CoreShell, memorySpaces } from '../../../app/web/core/shell.ts'

/** A core that opens one stream and sends one request at start, and keeps every event it is handed. */
function fakeCore(timeout_ms) {
  const seen = []
  return {
    seen,
    handle(_now, packet) {
      const event = JSON.parse(packet.json)
      seen.push(event)
      if (event.type !== 'start') return { json: '[]', blobs: [] }
      const request = url => ({ method: 'GET', url, headers: [], timeout_ms, max_answer: 1000, class: 'background' })
      return { json: JSON.stringify([{ type: 'open_stream', id: 1, request: request('http://hub.test/v1/stream') }, { type: 'http', id: 2, request: request('http://hub.test/v1/changes') }]), blobs: [] }
    },
  }
}
const wait = ms => new Promise(r => setTimeout(r, ms))

test('an open stream outlives its timeout; an answered request is not aborted after it', async () => {
  const signals = new Map()
  let push = null
  const fetcher = async (url, init) => {
    signals.set(new URL(url).pathname, init.signal)
    if (url.endsWith('/v1/stream')) {
      const body = new ReadableStream({ start(c) { push = c } })
      return new Response(body, { status: 200, headers: { 'content-type': 'text/event-stream' } })
    }
    return new Response('{}', { status: 200 })
  }
  const core = fakeCore(30)
  const shell = new CoreShell({ core, ...memorySpaces(), host: { render() {} }, fetch: fetcher })
  shell.send({ type: 'start', config: {} })
  await wait(120)
  push.enqueue(new TextEncoder().encode('event: here\ndata: {}\n\n'))
  await wait(20)
  const items = core.seen.filter(e => e.type === 'stream').map(e => e.item.item)
  assert.deepEqual(items, ['opened', 'chunk'], 'the stream is open four timeouts later and still brings its events')
  assert.equal(signals.get('/v1/stream').aborted, false)
  assert.equal(signals.get('/v1/changes').aborted, false, 'an answer read in time is left alone')
  assert.equal(core.seen.find(e => e.type === 'http').result.ok.status, 200)
  shell.close()
})

test('a stream whose head does not come in time fails', async () => {
  const fetcher = (url, init) => new Promise((_, reject) => init.signal.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError'))))
  const core = fakeCore(20)
  const shell = new CoreShell({ core, ...memorySpaces(), host: { render() {} }, fetch: fetcher })
  shell.send({ type: 'start', config: {} })
  await wait(80)
  assert.deepEqual(core.seen.filter(e => e.type === 'stream').map(e => e.item.item), ['failed'])
  assert.equal(core.seen.find(e => e.type === 'http').result.err.kind, 'timeout')
  shell.close()
})
