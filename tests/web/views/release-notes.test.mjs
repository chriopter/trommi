// The release notes of one part (.github/scripts/release_notes.mjs): the "What's new" lines of the pull requests that
// touch the part, by kind, each once; a PR without the section by its title, internal ones left out. And What's new
// reads them back (app/web/whats-new.mjs parseNotes).
//   node --test tests/web/views/release-notes.test.mjs
import test from 'node:test'
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { gather, notes, prLines, section, touches, NOTHING } from '../../../.github/scripts/release_notes.mjs'

const INPUTS = new URL('../../../.github/scripts/inputs.sh', import.meta.url).pathname

test('a PR\'s section "What\'s new": one line per item, its kind from its start, else the title\'s', () => {
  const body = 'Intro.\n\n## What\'s new\n\n<!-- a hint\n- New: not this one\n-->\n- New: Star cards\n  you want to keep (web and iPhone)\n- Fixed: logging out no longer hangs\n- Faster sign-in\n\n## Notes\n\n- not a line\n'
  assert.deepEqual(prLines({ number: 7, title: '2026-10-13-1: Better: things', body }), [
    { kind: 'New', text: 'Star cards you want to keep (web and iPhone)', pr: 7 },
    { kind: 'Fixed', text: 'Logging out no longer hangs', pr: 7 },
    { kind: 'Better', text: 'Faster sign-in', pr: 7 },
  ])
  // an empty section (or "none"): internal, even when the title reads as news
  assert.deepEqual(prLines({ number: 8, title: 'New: a thing', body: '## What\'s new\n\n<!-- nothing -->\n\n## Notes\n' }), [])
  assert.deepEqual(prLines({ number: 8, title: 'New: a thing', body: '## What\'s new\n- none\n' }), [])
  assert.equal(section('no section'), null)
})

test('a PR without the section: its title, split by kind; internal titles left out', () => {
  assert.deepEqual(prLines({ number: 9, title: '2026-10-13-36: New: Linux app, first version, Fixed: the Mac build reads its base', body: '' }), [
    { kind: 'New', text: 'Linux app, first version', pr: 9 },
    { kind: 'Fixed', text: 'The Mac build reads its base', pr: 9 },
  ])
  assert.deepEqual(prLines({ number: 10, title: 'Updated: start page offers Try the demo', body: null }), [{ kind: 'Better', text: 'Start page offers Try the demo', pr: 10 }])
  assert.deepEqual(prLines({ number: 11, title: '2026-10-13-31: Fixed: the twelve-devices web test waits', body: '' }), [])
  assert.deepEqual(prLines({ number: 12, title: 'Merge branch x', body: '' }), [])
})

test('the notes: New, Better, Fixed in that order, each line once with its PR, <…> escaped outside code', () => {
  const out = notes([
    { number: 1, title: 'x', body: '## What\'s new\n- Fixed: a <b> & `<code>` fix\n- New: one\n' },
    { number: 2, title: 'Better: faster', body: '' },
    { number: 3, title: 'y', body: '## What\'s new\n- New: One\n' },
  ])
  assert.equal(out, '**New**\n- One (#1)\n\n**Better**\n- Faster (#2)\n\n**Fixed**\n- A &lt;b&gt; &amp; `<code>` fix (#1)\n')
  assert.equal(notes([{ number: 4, title: 'Fixed: CI on main', body: '' }]), `${NOTHING}\n`)
})

test('gather: the PRs between two commits that touch the part, oldest first, each once', async () => {
  const calls = []
  const api = async p => {
    calls.push(p)
    if (p === 'compare/a...d') return { commits: [{ sha: 'b' }, { sha: 'c' }, { sha: 'c2' }, { sha: 'd' }] }
    if (p === 'commits/b') return { files: [{ filename: 'app/web/public/news.mjs' }] }
    if (p === 'commits/c') return { files: [{ filename: 'hub/src/main.rs' }] }
    if (p === 'commits/c2') return { files: [{ filename: 'core/x.rs' }] }
    if (p === 'commits/d') return { files: [{ filename: 'app/web/x.css' }] }
    if (p === 'commits/b/pulls') return [{ number: 5, title: 'New: b', body: '', merged_at: 'x' }]
    if (p === 'commits/c2/pulls') return [{ number: 6, title: 'Fixed: c2', body: '', merged_at: 'x' }]
    if (p === 'commits/d/pulls') return [{ number: 5, title: 'New: b', body: '', merged_at: 'x' }]
    throw new Error(p)
  }
  const prs = await gather({ before: 'a', commit: 'd', paths: ['core/', 'app/web/'], api })
  assert.deepEqual(prs.map(p => p.number), [5, 6])
  assert.ok(!calls.includes('commits/c/pulls'), 'a commit outside the part is not asked for its PR')
  assert.deepEqual((await gather({ before: '', commit: 'b', paths: ['app/web/'], api })).map(p => p.number), [5])
})

test('a part\'s paths are those inputs.sh builds it from, the shared ones included', () => {
  const web = execFileSync('sh', [INPUTS, 'paths', 'web'], { encoding: 'utf8' }).split(/\s+/).filter(Boolean)
  assert.ok(web.includes('app/web/') && web.includes('core/'))
  assert.ok(touches('app/web/public/news.mjs', web) && touches('Cargo.lock', web) && !touches('hub/src/main.rs', web))
  assert.ok(!touches('Cargo.lock.bak', ['Cargo.lock']))
  assert.throws(() => execFileSync('sh', [INPUTS, 'paths', 'nothing'], { stdio: 'ignore' }))
})
