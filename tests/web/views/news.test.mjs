// news.test.mjs: What's new (app/web/public/news.mjs) shows CHANGELOG.md as the build reads it (dev/build.mjs
// parseChangelog): days newest first, each with its releases and its parts New, Better, Fixed.
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
    { day: '12 October 2026', releases: 'v116', groups: [{ kind: 'New', items: ['One thing that goes on.', '`code` too.'] }, { kind: 'Fixed', items: ['A fix.'] }] },
    { day: '11 October 2026', releases: '', groups: [{ kind: '', items: ['Without a part.'] }] },
  ])
})
