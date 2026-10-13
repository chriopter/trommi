// news.test.mjs: what What's new (app/web/public/news.mjs, from /whats-new.json: app/web/whats-new.mjs) is made of
// besides the releases' notes: public/changelog/CHANGELOG.md, one summary of one to three sentences a day and at most one picture of the
// demo room (webp, at most 150 KB), and history.json, the days of the old CHANGELOG.md with New, Better and Fixed;
// every line with its short form (changelogHead).
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import { buildDays, changelogHead, isoOf, parseSummaries } from '../../../app/web/whats-new.mjs'

const DIR = new URL('../../../app/web/public/changelog/', import.meta.url)
const TEXT = fs.readFileSync(new URL('CHANGELOG.md', DIR), 'utf8')
const HISTORY = JSON.parse(fs.readFileSync(new URL('history.json', DIR), 'utf8'))

test('the day summaries: a date heading, one short line, at most one picture that is there', () => {
  const heads = [...TEXT.matchAll(/^## +(.+)$/gm)].map(m => m[1])
  assert.ok(heads.length >= 1, 'at least one day')
  for (const h of heads) assert.ok(isoOf(h), `a day's heading is a date: ${h}`)
  const isos = heads.map(isoOf)
  assert.deepEqual(isos, [...isos].sort().reverse(), 'newest first')
  for (const [iso, d] of Object.entries(parseSummaries(TEXT))) {
    const sentences = d.summary.split(/(?<=[.!?])\s+/).filter(Boolean)
    assert.ok(d.summary.length > 10 && d.summary.length <= 320 && sentences.length <= 3, `${iso} has a summary of one to three short sentences: ${d.summary}`)
    assert.ok(!/^\s*[-*] /m.test(d.summary) && !/\*\*(New|Better|Fixed)\*\*/.test(d.summary), `${iso}: a summary only, no lists`)
    if (d.picture) {
      const file = new URL(`../../../app/web/public${d.picture.src}`, import.meta.url)
      assert.ok(fs.existsSync(file), `${iso}: its picture ${d.picture.src} is in app/web/public/changelog/`)
      assert.ok(fs.statSync(file).size <= 150 * 1024, `${iso}: its picture is at most 150 KB`)
      assert.equal(fs.readFileSync(file).subarray(8, 12).toString(), 'WEBP', `${iso}: its picture is a webp`)
      assert.ok(d.picture.alt, `${iso}: its picture says what it shows`)
    }
  }
})

test('the history: days with New, Better and Fixed, every line with a short form of 3 to 80 characters', () => {
  assert.ok(HISTORY.length >= 1)
  for (const d of buildDays({ history: HISTORY })) {
    assert.ok(d.groups.length, `${d.day} has parts`)
    for (const g of d.groups) {
      assert.ok(['New', 'Better', 'Fixed'].includes(g.kind), `${d.day}: a part is New, Better or Fixed, not "${g.kind}"`)
      for (const i of g.items) assert.ok(i.text.length > 3 && i.head.length >= 3 && i.head.length <= 80, `${d.day} · ${g.kind}: "${i.head}"`)
    }
  }
})

test('a day\'s summary and its one picture', () => {
  const s = parseSummaries('# x\n\n## 12 October 2026\n\nStar cards. Web and\niPhone agree.\n\n![The Desk](2026-10-12.webp)\n![A second](x.webp)\n\n## 11 October 2026\n\nNo picture.\n\n## Not a date\n\nLeft out.\n')
  assert.deepEqual(s, {
    '2026-10-12': { summary: 'Star cards. Web and iPhone agree.', picture: { alt: 'The Desk', src: '/changelog/2026-10-12.webp' } },
    '2026-10-11': { summary: 'No picture.', picture: null },
  })
})

test('a line\'s short form: a bold start, a tag, the first clause, never too short or too long', () => {
  const h = changelogHead
  assert.deepEqual(h('**Readable addresses**: `/chat/<id>` and `/artifact/<id>`.'), { text: '`/chat/<id>` and `/artifact/<id>`.', tag: '', head: 'Readable addresses' })
  assert.deepEqual(h('**Web: no flash of closed cards**: opening the app no longer shows old cards.'), { text: 'Opening the app no longer shows old cards.', tag: 'Web', head: 'No flash of closed cards' })
  assert.deepEqual(h('iPhone: a permission request whose time ran out says "expired", as on the web.'), { text: 'A permission request whose time ran out says "expired", as on the web.', tag: 'iPhone', head: 'A permission request whose time ran out says "expired"' })
  assert.equal(h('Star a card (the star beside its close button) to find it again.').head, 'Star a card')
  assert.equal(h('Duck it: answer every open decision at once.').head, 'Duck it')
  assert.equal(h('Logging out never hangs, even on a slow connection.').head, 'Logging out never hangs')
  assert.equal(h('After logging out, the browser kept the key.').head, 'After logging out')   // (two words: a bold start reads better)
  assert.equal(h('Public beta: join from [trommi.com/beta](https://trommi.com/beta).').head, 'Public beta')
  assert.equal(h('iPhone: the app no longer crashes.').head, 'The app no longer crashes')
  assert.equal(h('iPad layout in a window.').head, 'iPad layout in a window')
  const long = h('A new Claude Code or Codex session on a computer and account that already has a connected agent gets onto the board.')
  assert.ok(long.head.length <= 78 && long.head.endsWith('…'), long.head)
})
