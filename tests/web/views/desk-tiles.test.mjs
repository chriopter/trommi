// desk-tiles.test.mjs: the Desk's tiles (desk.mjs deskTiles, DeskView.tiles) draw whatever the core leaves out: a tile
// without items (a team where nothing runs and nothing is published: the core sent none before 17 October 2026, and
// the Desk did not load), without a count, and no tiles at all.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import { deskTiles } from '../../../app/web/public/desk.mjs'

const say = en => ({ key: 'x', en })
const tile = (key, extra = {}) => ({ key, title: say(key), line: say('Nothing yet'), opens: { screen: 'desk' }, ...extra })

test('tiles without items, count or team draw, each with its link', () => {
  const d = { tiles: [tile('work'), tile('artifacts', { count: 0 }), tile('board'), tile('knowledge', { count: 0, team_id: 'main' })] }
  const out = String(deskTiles(d, ''))
  for (const key of ['work', 'artifacts', 'board', 'knowledge']) assert.match(out, new RegExp(`id="tile-${key}"`))
  assert.doesNotMatch(out, /dt-pages|<canvas/, 'nothing small where there is nothing')
})

test('empty items, and items the Desk does not hold, draw nothing small', () => {
  const d = { off: { running: [] }, tiles: [tile('work', { items: ['gone'] }), tile('artifacts', { items: [] })] }
  assert.doesNotMatch(String(deskTiles(d, '')), /dt-run"|dt-page/)
})

test('the bento: the size and the quiet state the core gives; an unknown size is narrow', () => {
  const d = { tiles: [tile('work', { size: 'wide', empty: true }), tile('artifacts', { size: 'narrow' }), tile('board', { size: 'full', team_id: 'main' }), tile('knowledge', { size: 'huge', names: ['A', 'B'] })] }
  const out = String(deskTiles(d, ''))
  assert.match(out, /class="desk-tile is-wide is-empty" id="tile-work"/)
  assert.match(out, /class="desk-tile is-full" id="tile-board"/)
  assert.match(out, /class="desk-tile is-narrow" id="tile-knowledge"/)
  assert.equal(out.match(/<li>/g)?.length, 2, "Knowledge shows the core's titles")
})

test('a tile without names, or with lists that are null, draws; an unknown tile is left out', () => {
  const d = { tiles: [tile('knowledge', { names: null, items: null }), tile('work', { items: null }), tile('mystery'), null] }
  const out = String(deskTiles(d, ''))
  assert.match(out, /id="tile-knowledge"/)
  assert.doesNotMatch(out, /tile-mystery|<li>/)
})

test('no tiles: an empty, hidden foot', () => {
  assert.match(String(deskTiles({}, '')), /<nav class="desk-tiles" id="desk-end" hidden><\/nav>/)
  assert.match(String(deskTiles({ tiles: [] }, '')), /hidden/)
})
