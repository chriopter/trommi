// news.test.mjs: What's new (app/web/public/news.mjs) shows CHANGELOG.md as the build reads it (dev/build.mjs
// parseChangelog): days newest first, each with its releases, its summary, at most one picture of the demo room
// (app/web/public/changelog/, webp, at most 150 KB) and its parts New, Better, Fixed.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import { parseChangelog } from '../../../app/web/dev/build.mjs'

const FILE = fs.readFileSync(new URL('../../../CHANGELOG.md', import.meta.url), 'utf8')

test('the repository\'s CHANGELOG.md reads as days with New, Better and Fixed', () => {
  const days = parseChangelog(FILE)
  assert.ok(days.length >= 1, 'at least one day')
  for (const d of days) {
    assert.match(d.day, /^\d{1,2} [A-Z][a-z]+ \d{4}$/, `a day's heading is a date: ${d.day}`)
    assert.ok(d.groups.length, `${d.day} has parts`)
    assert.ok(d.summary.length > 10, `${d.day} has a summary`)
    if (d.picture) {
      const file = new URL(`../../../app/web/public${d.picture.src}`, import.meta.url)
      assert.ok(fs.existsSync(file), `${d.day}: its picture ${d.picture.src} is in app/web/public/changelog/`)
      assert.ok(fs.statSync(file).size <= 150 * 1024, `${d.day}: its picture is at most 150 KB`)
      assert.equal(fs.readFileSync(file).subarray(8, 12).toString(), 'WEBP', `${d.day}: its picture is a webp`)
      assert.ok(d.picture.alt, `${d.day}: its picture says what it shows`)
    }
    for (const g of d.groups) {
      assert.ok(['New', 'Better', 'Fixed'].includes(g.kind), `${d.day}: a part is New, Better or Fixed, not "${g.kind}"`)
      assert.ok(g.items.length && g.items.every(i => i.length > 3), `${d.day} · ${g.kind}: lines with words`)
    }
  }
  const dates = days.map(d => Date.parse(d.day))
  assert.deepEqual(dates, [...dates].sort((a, b) => b - a), 'newest first')
})

test('a heading\'s releases, a line that goes on indented, and the words before the first day', () => {
  const days = parseChangelog('# Changelog\n\nIntro words.\n\n## 12 October 2026 (v116)\n\n**New**\n- One thing\n  that goes on.\n- `code` too.\n\n**Fixed**\n- A fix.\n\n## 11 October 2026\n\n- Without a part.\n')
  assert.deepEqual(days, [
    { day: '12 October 2026', releases: 'v116', summary: '', picture: null, groups: [{ kind: 'New', items: ['One thing that goes on.', '`code` too.'] }, { kind: 'Fixed', items: ['A fix.'] }] },
    { day: '11 October 2026', releases: '', summary: '', picture: null, groups: [{ kind: '', items: ['Without a part.'] }] },
  ])
})

test('a day\'s summary and its one picture, before its first part', () => {
  const days = parseChangelog('# Changelog\n\n## 12 October 2026 (v116)\n\nTwo lines\nof summary.\n\n![The Desk](app/web/public/changelog/2026-10-12.webp)\n![A second](app/web/public/changelog/x.webp)\n\n**New**\n- One thing.\n\nNot a summary any more.\n')
  assert.deepEqual(days, [
    { day: '12 October 2026', releases: 'v116', summary: 'Two lines of summary.', picture: { alt: 'The Desk', src: '/changelog/2026-10-12.webp' }, groups: [{ kind: 'New', items: ['One thing.'] }] },
  ])
})
