// A reading cut short goes on where it was: the cache is stamped with where a rebuild began, and a start reads from
// there, never the room's past from its start (9.0.6). STAND-IN core and FAKE hub.
import test from 'node:test'
import assert from 'node:assert/strict'
import { memoryCache, scene, until } from './helpers.mjs'

test('a cache behind the device is filled from its own cursor on, never from the start; a stop in the middle resumes there', async t => {
  const { fake, R, a, b } = await scene(t, { second: true })
  for (let i = 0; i < 20; i++) await a.saveNote({ text: `note ${i}` })
  await a.settle(); await b.settle()
  await b.stop()
  const cache = memoryCache(b.stored_as.name)
  const at = await cache.get('client/at')
  const behind = at.cursor - 10
  await cache.set('client/at', { ...at, cursor: behind })
  for (const round of [1, 2]) {
    const asked = fake.requests.length
    const again = await R.openRoom({ storage: b.stored_as })
    await again.start({ stream: false })
    if (round === 1) {
      // cut short right away: what the cache says now is where the rebuild began
      await again.stop()
      assert.equal((await cache.get('client/at')).cursor >= behind, true, 'the cache keeps where the rebuild began')
    } else {
      await again.settle()
      await until(() => !again.engine.rescanPending, 'the rebuild through')
      await again.stop()
    }
    const from = fake.requests.slice(asked).filter(r => r.method === 'GET' && r.path === '/v1/changes').map(r => Number(r.query.after))
    assert.ok(from.every(n => n >= behind - 1), `round ${round}: read from the cache's cursor on, never from the start (${from.join(', ')})`)
  }
})
