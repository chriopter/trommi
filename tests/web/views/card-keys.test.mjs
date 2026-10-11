// card-keys.test.mjs: on an opened question ← → are the pictures' (the gallery), never the questions'; the next and
// previous question are Shift ↓ ↑, as ↑ ↓ walk its answers (ui.mjs LAYOUT, scope "card").
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import { LAYOUT, SHORT } from '../../../app/web/public/ui.mjs'

const card = LAYOUT.find(g => g.scope === 'card').keys
const owners = key => card.filter(k => k.keys.includes(key)).map(k => k.id)

test('← → on a card: the pictures, never another question', () => {
  assert.deepEqual(owners('ArrowRight'), ['ans.right', 'card.pic.next'])
  assert.deepEqual(owners('ArrowLeft'), ['ans.left', 'card.pic.prev'])
})

test('another question: Shift ↓ ↑; plain ↑ ↓ stay the answers', () => {
  assert.deepEqual(owners('Shift+ArrowDown'), ['card.next'])
  assert.deepEqual(owners('Shift+ArrowUp'), ['card.prev'])
  assert.deepEqual(owners('ArrowDown'), ['ans.down'])
  assert.deepEqual(owners('ArrowUp'), ['ans.up'])
  assert.match(SHORT.find(k => k.id === 'move').does, /Shift ↑ ↓ there: the questions, ← →: the pictures/)
})
