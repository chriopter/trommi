// option-texts.test.mjs: the options' own paragraphs stand at the end of the card's left column, after the pictures
// (card.mjs optionsSaid): one open block per option in the order of the tiles (the recommended one last, as its tile),
// keyed for the pointer's highlight and arrow; nothing when no option says more than its label.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import { optionsSaid } from '../../../app/web/public/card.mjs'

const card = sections => ({ id: 'c1', options: [{ key: 'monday', label: 'Monday' }, { key: 'tuesday', label: 'Tuesday' }], recommended: ['monday'], sections })

test('one open block per option, in the tiles\' order, the recommended one marked', () => {
  const html = String(optionsSaid(card([{ text: 'Intro.' }, { key: 'monday', label: 'Monday', recommended: true, text: 'Web first.' }, { key: 'tuesday', label: 'Tuesday', text: 'Wait a day.' }]), new Map()))
  assert.match(html, /<section class="tc-say" id="card-say-c1"/)
  assert.doesNotMatch(html, /<details/, 'all open, nothing to fold')
  const keys = [...html.matchAll(/class="tc-say-one" data-key="(\w+)"/g)].map(m => m[1])
  assert.deepEqual(keys, ['tuesday', 'monday'])
  assert.match(html, /Monday<\/span> <span class="tc-advised-word">recommended<\/span>/)
  assert.match(html, /Wait a day\./)
})

test('no paragraph beyond the labels: an empty, hidden place for the live stream', () => {
  const html = String(optionsSaid(card([{ key: 'monday', label: 'Monday', text: 'Monday' }]), new Map()))
  assert.equal(html, '<div id="card-say-c1" hidden></div>')
})
