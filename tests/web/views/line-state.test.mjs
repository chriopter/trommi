// line-state.test.mjs: a status line shows what its card and session say now (app.mjs lineState, client/README.md
// "Status lines"), so it never says more than the Desk's "Off your mind".
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'

const { lineState, LINE_STALE_MS, ACTING_MS } = await import('../../../app/web/public/app.mjs')
const now = 100 * 3600000
const agent = { id: 'a', online: true, archived: false }
const line = (state, extra = {}) => ({ agent: 'a', id: 't', state, updated: now - 60000, ...extra })
const card = extra => ({ id: 'c', agent: 'a', kind: 'decision', status: 'open', ...extra })
const at = (t, c = null, a = agent) => lineState(t, c, a, { now })

test('a line of an archived or vanished session is gone', () => {
  assert.equal(at(line('working'), null, { ...agent, archived: true }), 'gone')
  assert.equal(at(line('working'), null, null), 'gone')
})

test('a line whose card is put off is later, not waiting on him', () => {
  assert.equal(at(line('decision'), card({ snoozed_until: now + 3600000 })), 'later')
  assert.equal(at(line('working'), card({ snoozed_until: now + 3600000 })), 'later')
})

test('an open card holds its line as set, however old', () => {
  assert.equal(at(line('decision', { updated: now - 50 * 3600000 }), card()), 'decision')
  assert.equal(at(line('working', { updated: now - 50 * 3600000 }), card({ kind: 'info', with_agent: now - 1000 })), 'working')
})

test('answered: working while its session is at it, done once that time is over', () => {
  const answered = card({ status: 'decided', choice: 'a', decided: now - 60000 })
  assert.equal(at(line('decision'), answered), 'working')
  assert.equal(at(line('working'), answered), 'working')
  assert.equal(at(line('working'), { ...answered, decided: now - ACTING_MS - 1 }), 'done')
  assert.equal(at(line('working'), card({ status: 'shredded' })), 'done')
})

test('an unlinked line nobody updated for LINE_STALE_MS is stale; done stays done', () => {
  assert.equal(at(line('working', { updated: now - LINE_STALE_MS + 1000 })), 'working')
  assert.equal(at(line('working', { updated: now - LINE_STALE_MS - 1000 })), 'stale')
  assert.equal(at(line('decision', { updated: now - LINE_STALE_MS - 1000 })), 'stale')
  assert.equal(at(line('done', { updated: now - LINE_STALE_MS - 1000 })), 'done')
})
