// session-archive.test.mjs: a session's More menu (session.mjs sessionMore) has a plain Archive beside Delete…:
// the session goes to the archive and its device stays (agents.mjs /edit, archived=1, with an Undo on the toast);
// only while its agent is away (the route refuses a connected one); an archived one comes back the same way.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import { archiveEntry } from '../../../app/web/public/session.mjs'

const row = a => String(archiveEntry({ id: 'abc123', name: 'trommi', ...a }, ''))

test('an away session can be archived without deleting its device', () => {
  const html = row({ online: false, archived: false })
  assert.match(html, /action="\/sessions\/abc123\/edit"/)
  assert.match(html, /name="archived" value="1"/)
  assert.match(html, /<span>Archive<\/span>/)
  assert.doesNotMatch(html, /delete/)
})

test('a connected session has no Archive; an archived one comes back', () => {
  assert.equal(row({ online: true, archived: false }), '')
  assert.match(row({ online: false, archived: true }), /name="archived" value="0"[^]*Back from the Archive/)
})
