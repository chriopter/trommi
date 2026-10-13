// card-next.test.mjs: a decision made on a card's own page keeps the card open, decided, and offers the next open
// question and Close (card.mjs nextStep); in Blitz (?walk=1) the answer goes straight on to the next card.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'

const src = fs.readFileSync(new URL('../../../app/web/public/card.mjs', import.meta.url), 'utf8')

test('decided on its own page, not in Blitz: back to the card, with the question that followed it', () => {
  assert.match(src, /if \(\['decide', 'trust'\]\.includes\(what\) && !form\.has\('walk'\) && form\.has\('page'\)\) \{[\s\S]*?return redirect\(res, to\(card, fromSession \? home : BASE, said, next \? `next=\$\{next\}` : ''\)\)/)
  assert.match(src, /<input type="hidden" name="page" value="1">/, 'the card page\'s form says where it was sent from')
})

test('in Blitz the answer still goes on to the next card', () => {
  const stay = src.indexOf("form.has('page')"), walk = src.indexOf("if (form.has('walk') || (form.has('next') && what === 'revise'))")
  assert.ok(stay > 0 && walk > stay, 'the walk keeps its own way on')
})

test('the decided card offers the next question, or says there is none, and Close', () => {
  assert.match(src, /<nav class="tc-next" aria-label="What next">/)
  assert.match(src, /<span>Next question<\/span><small>\$\{next\.title\}<\/small>/)
  assert.match(src, /That was the last open question\./)
  assert.match(src, /<a class="tc-next-close" data-nav href="\$\{home\}">Close<\/a>/)
})

test('which question comes next is the client core\'s, not the page\'s', () => {
  // (client/src/model/walk.rs next_open_after, through the worker: the Desk's order, the desk in view)
  assert.match(src, /const next = await Promise\.resolve\(hub\.nextOpenAfter\?\.\(id, deskScope\(model\(\)\)\)\)/)
  // (while it still waits; else the first that waits on this desk in the core's order)
  assert.match(src, /model\.state\.desk_order \?\? \[\]\)\.find\(x => waits\.has\(x\)\)/)
  assert.doesNotMatch(src, /const nextOpen = [^\n]*walk\[0\]/)
})
