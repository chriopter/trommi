// touch-callout.test.mjs: on an iPhone or iPad Safari, a long press on a chat row would bring up its link callout
// and take the touch the order's drag needs (sidebar.mjs, controller "order": a finger lifts a row after a short
// hold). sidebar.css turns the callout off on the chat row itself; a chat row has no rank part (data-order) since
// chats stand newest first, so the rule must hold for the rows the sidebar actually draws: data-unit, no data-order.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { sidebarRows } from '../../../app/web/public/sidebar.mjs'

const css = fs.readFileSync(path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..', '..', 'app', 'web', 'public', 'sidebar.css'), 'utf8')

/** The row selectors (`#agents .agent-row[attr]…`) whose rule turns the callout off: each as the attributes it asks for. */
const calloutOff = [...css.replace(/\/\*[^]*?\*\//g, '').matchAll(/([^{}]+)\{([^{}]*)\}/g)]
  .filter(([, , body]) => /-webkit-touch-callout:\s*none/.test(body))
  .flatMap(([, sel]) => sel.split(','))
  .map(s => s.trim().match(/^#agents \.agent-row((?:\[[\w-]+\])*)$/))
  .filter(Boolean)
  .map(m => [...m[1].matchAll(/\[([\w-]+)\]/g)].map(a => a[1]))

test('a chat row without a rank part keeps the touch callout off', () => {
  const chat = (id, team) => ({ id, slug: id, name: { en: id }, mark: 'draw:bell', team_id: team, cards: 0, online: true })
  const vm = { sidebar: { root: [chat('root-chat', '')], places: [{ name: { en: 'No team' } }], sessions: [], stacks: [{ id: 'work', name: { en: 'Work' }, open: true, sessions: [chat('team-chat', 'work')], says: { en: '1 chat' }, faces: [], edges: [] }] } }
  const html = String(sidebarRows(vm, ''))
  for (const id of ['root-chat', 'team-chat']) {
    const tag = html.match(new RegExp(`<div class="agent-row[^"]*" id="agent-${id}"[^>]*>`))?.[0]
    assert.ok(tag, `the chat ${id} is drawn`)
    assert.doesNotMatch(tag, /data-order/, 'a chat row has no rank part')
    const attrs = new Set([...tag.matchAll(/\s([\w-]+)=/g)].map(a => a[1]))
    assert.ok(calloutOff.some(need => need.every(a => attrs.has(a))), `no sidebar.css rule turns the callout off on ${tag}`)
  }
})
