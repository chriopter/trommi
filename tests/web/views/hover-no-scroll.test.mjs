// hover-no-scroll.test.mjs: the pointer on an option tile of a card marks its explained block and never scrolls the
// left column (the picture being looked at must stay in view). card.mjs `say` is run on a stand-in DOM, with the
// block far below the visible part, and the timers run on.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import { mock } from 'node:test'

const src = fs.readFileSync(new URL('../../../app/web/public/card.mjs', import.meta.url), 'utf8')
const start = src.indexOf('  say(key) {'), end = src.indexOf('  sayLink() {')
const say = new Function('CSS', `return function ${src.slice(start, end).trim()}`)({ escape: s => s })

test('pointing at an option does not scroll the left column, even for a block out of sight', () => {
  mock.timers.enable({ apis: ['setTimeout'] })
  const left = { scrollTop: 120, scrollCalls: 0, scrollTo() { this.scrollCalls++ }, scrollBy() { this.scrollCalls++ }, getBoundingClientRect: () => ({ top: 0, bottom: 600 }) }
  const block = { dataset: { key: 'a' }, flags: {}, toggleAttribute(n, on) { this.flags[n] = on }, scrollIntoView() { left.scrollCalls++ }, getBoundingClientRect: () => ({ top: 2000 }) }
  const box = { flags: {}, toggleAttribute(n, on) { this.flags[n] = on }, querySelector: () => block, querySelectorAll: () => [block] }
  const host = { querySelector: s => s === '.tc-say' ? box : s === '.tc-left' ? left : null }
  const self = { element: { querySelector: () => host }, sayLink() {} }
  say.call(self, 'a')
  mock.timers.tick(5000)
  assert.equal(block.flags['data-match'], true, 'the block is marked')
  assert.equal(box.flags['data-hover'], true)
  assert.equal(left.scrollCalls, 0, 'no scroll call')
  assert.equal(left.scrollTop, 120, 'scrollTop unchanged')
  mock.timers.reset()
})
