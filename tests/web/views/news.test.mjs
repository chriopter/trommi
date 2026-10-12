// news.test.mjs: What's new (app/web/public/news.mjs) shows CHANGELOG.md as the build reads it (dev/build.mjs
// parseChangelog): days newest first, each with its one-line summary, at most one picture of the demo room
// (app/web/public/changelog/, webp, at most 150 KB) and its parts New, Better, Fixed, every line with its short form
// (changelogHead). The page (news.mjs) shows the picture on the newest three days only. And a GitHub release's notes
// (.github/scripts/release_notes.sh): the newest day, its releases counted from the published ones, as GitHub shows it.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { changelogHead, parseChangelog } from '../../../app/web/dev/build.mjs'

const FILE = fs.readFileSync(new URL('../../../CHANGELOG.md', import.meta.url), 'utf8')

test('the repository\'s CHANGELOG.md reads as days with New, Better and Fixed', () => {
  const days = parseChangelog(FILE)
  assert.ok(days.length >= 1, 'at least one day')
  for (const d of days) {
    assert.match(d.day, /^\d{1,2} [A-Z][a-z]+ \d{4}$/, `a day's heading is a date: ${d.day}`)
    assert.equal(d.releases, '', `${d.day}: no release numbers by hand (the release notes count them)`)
    assert.ok(d.groups.length, `${d.day} has parts`)
    assert.ok(d.summary.length > 10 && d.summary.length <= 110, `${d.day} has a summary of one short line: ${d.summary}`)
    if (d.picture) {
      const file = new URL(`../../../app/web/public${d.picture.src}`, import.meta.url)
      assert.ok(fs.existsSync(file), `${d.day}: its picture ${d.picture.src} is in app/web/public/changelog/`)
      assert.ok(fs.statSync(file).size <= 150 * 1024, `${d.day}: its picture is at most 150 KB`)
      assert.equal(fs.readFileSync(file).subarray(8, 12).toString(), 'WEBP', `${d.day}: its picture is a webp`)
      assert.ok(d.picture.alt, `${d.day}: its picture says what it shows`)
    }
    for (const g of d.groups) {
      assert.ok(['New', 'Better', 'Fixed'].includes(g.kind), `${d.day}: a part is New, Better or Fixed, not "${g.kind}"`)
      assert.ok(g.items.length && g.items.every(i => i.text.length > 3), `${d.day} · ${g.kind}: lines with words`)
      for (const i of g.items) assert.ok(i.head.length >= 3 && i.head.length <= 80, `${d.day} · ${g.kind}: a short form of 3 to 80 characters: "${i.head}"`)
    }
  }
  const dates = days.map(d => Date.parse(d.day))
  assert.deepEqual(dates, [...dates].sort((a, b) => b - a), 'newest first')
})

test('a heading\'s releases, a line that goes on indented, and the words before the first day', () => {
  const days = parseChangelog('# Changelog\n\nIntro words.\n\n## 12 October 2026 (v116)\n\n**New**\n- One thing\n  that goes on.\n- `code` too.\n\n**Fixed**\n- A fix.\n\n## 11 October 2026\n\n- Without a part.\n')
  assert.deepEqual(days, [
    { day: '12 October 2026', releases: 'v116', summary: '', picture: null, groups: [{ kind: 'New', items: [{ text: 'One thing that goes on.', tag: '', head: 'One thing that goes on' }, { text: '`code` too.', tag: '', head: 'Code too' }] }, { kind: 'Fixed', items: [{ text: 'A fix.', tag: '', head: 'A fix' }] }] },
    { day: '11 October 2026', releases: '', summary: '', picture: null, groups: [{ kind: '', items: [{ text: 'Without a part.', tag: '', head: 'Without a part' }] }] },
  ])
})

test('a day\'s summary and its one picture: the lines before its first part', () => {
  const days = parseChangelog('# Changelog\n\n## 12 October 2026\n\nStar cards. Web and\niPhone agree.\n\n![The Desk](app/web/public/changelog/2026-10-12.webp)\n![A second](app/web/public/changelog/x.webp)\n\n**New**\n- One thing.\n\nNot a summary any more.\n\n## 11 October 2026\n\nNo picture.\n\n**New**\n- A thing.\n')
  assert.equal(days[0].summary, 'Star cards. Web and iPhone agree.')
  assert.deepEqual(days[0].picture, { alt: 'The Desk', src: '/changelog/2026-10-12.webp' })
  assert.equal(days[0].groups[0].items.length, 1)
  assert.equal(days[1].picture, null)
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

const NOTES = new URL('../../../.github/scripts/release_notes.sh', import.meta.url).pathname
const notes = (changelog, ...args) => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'notes-'))
  fs.writeFileSync(path.join(dir, 'CHANGELOG.md'), changelog)
  const files = args.map((a, i) => (Array.isArray(a) ? (fs.writeFileSync(path.join(dir, `r${i}`), a.join('\n') + '\n'), path.join(dir, `r${i}`)) : a))
  try {
    return execFileSync('sh', [NOTES, path.join(dir, 'CHANGELOG.md'), ...files], { encoding: 'utf8', env: { ...process.env, NOTES_BASE: 'https://raw.example/c/' } })
  } finally { fs.rmSync(dir, { recursive: true }) }
}

test('release notes: the newest day, its lines unwrapped, <…> escaped outside code, the picture as a whole address', () => {
  const out = notes('# Changelog\n\nIntro.\n\n## 11 October 2026 (v1 – v2)\n\nA summary that\ngoes on.\n\n![The demo](app/web/public/changelog/2026-10-11.webp)\n\n**New**\n- "<session> is working on it", what it keeps on\n  the hub.\n- `<tag>` & more\n\n**Fixed**\n- A fix.\n\n## 10 October 2026\n\n- Older.\n')
  assert.equal(out, '## 11 October 2026\n\nA summary that goes on.\n\n![The demo](https://raw.example/c/app/web/public/changelog/2026-10-11.webp)\n\n**New**\n- "&lt;session&gt; is working on it", what it keeps on the hub.\n- `<tag>` &amp; more\n\n**Fixed**\n- A fix.\n')
})

test('release notes: the heading counts the day\'s releases (Europe/Berlin), this one included', () => {
  const log = '## 11 October 2020\n\n**New**\n- A thing.\n'
  const published = ['v108 2020-10-10T21:00:54Z', 'v109 2020-10-10T22:06:13Z', 'v150 2020-10-11T12:00:00Z', 'v208 2020-10-11T18:42:57Z', 'v300 2020-10-11T22:30:00Z', 'hub-v5 2020-10-11T12:00:00Z']
  // a day in the past: only the releases published on it (v109 at 00:06 in Berlin is the 11th, v300 the 12th)
  assert.match(notes(log, 'v400', published), /^## 11 October 2020 \(v109 – v208\)\n/)
  assert.match(notes(log, '', ['v150 2020-10-11T12:00:00Z']), /^## 11 October 2020 \(v150\)\n/)
  assert.match(notes(log), /^## 11 October 2020\n/)
  // today: this release counts although it is not published yet
  const today = new Intl.DateTimeFormat('en-GB', { timeZone: 'Europe/Berlin', day: 'numeric', month: 'long', year: 'numeric' }).format(new Date())
  assert.match(notes(`## ${today}\n\n- Now.\n`, 'v999', []), new RegExp(`^## ${today} \\(v999\\)\\n`))
})

test('release notes: nothing for a missing or empty CHANGELOG.md, and never a failure', () => {
  assert.equal(execFileSync('sh', [NOTES, '/nonexistent/CHANGELOG.md'], { encoding: 'utf8' }), '')
  assert.equal(notes('# Changelog\n\nNo day yet.\n'), '')
})
