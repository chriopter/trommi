// card-live.test.mjs: the live pieces of a card's page (card.mjs register, t.live('card')) are streamed as markup: the
// star button and the options explained arrive as HTML, never as escaped text (a star once showed "<form …" for a
// moment).
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import { register } from '../../../app/web/public/card.mjs'
import { stream } from '../../../app/web/public/app.mjs'

function liveCard() {
  let live = null
  const t = { BASE: '', hub: {}, model: {}, stream, says: () => {}, redirect: () => {}, differs: (a, b) => String(a) !== String(b),
    get: () => {}, post: () => {}, live: (name, cfg) => { if (name === 'card') live = cfg } }
  register(t)
  return live
}

test('a star turned on streams the new button as markup', () => {
  const live = liveCard()
  assert.ok(live, 'the card registers its live pieces')
  const was = new Map([['c1|', { star: '<form id="card-star-c1"><button>off</button></form>', face: 'f', say: 's', answerSig: 'a', thread: 't' }]])
  const now = new Map([['c1|', { star: '<form id="card-star-c1"><button aria-pressed="true">on</button></form>', face: 'f', say: 's', answerSig: 'a', thread: 't' }]])
  const out = live.diff(was, now, { params: new URLSearchParams('card=c1') })
  assert.match(out, /<template><form id="card-star-c1"><button aria-pressed="true">on<\/button><\/form><\/template>/)
  assert.doesNotMatch(out, /&lt;form/)
})

test('the options explained, changed, stream as markup too', () => {
  const live = liveCard()
  const was = new Map([['c1|', { star: 'x', face: 'f', say: '<div id="card-say-c1" hidden></div>', answerSig: 'a', thread: 't' }]])
  const now = new Map([['c1|', { star: 'x', face: 'f', say: '<section class="tc-say" id="card-say-c1"></section>', answerSig: 'a', thread: 't' }]])
  const out = live.diff(was, now, { params: new URLSearchParams('card=c1') })
  assert.match(out, /<template><section class="tc-say" id="card-say-c1"><\/section><\/template>/)
  assert.doesNotMatch(out, /&lt;section/)
})
