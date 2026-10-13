// /whats-new.json (app/web/whats-new.mjs through worker.js): What's new from the public GitHub releases' notes, the day
// summaries (public/changelog/CHANGELOG.md) and the frozen history (public/changelog/history.json). GitHub is a
// stand-in here; the worker's ASSETS reads public/ as in association.test.mjs.
//   node --test tests/web/worker/
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import worker from '../../../app/web/worker.js'
import { buildDays, changelogHead, forget, parseNotes, parseSummaries, readReleases, whatsNew } from '../../../app/web/whats-new.mjs'
import { notes } from '../../../.github/scripts/release_notes.mjs'

const PUBLIC = new URL('../../../app/web/public/', import.meta.url)
const ASSETS = {
  async fetch(request) {
    const bytes = await readFile(new URL(`.${new URL(request.url).pathname}`, PUBLIC)).catch(() => null)
    return bytes ? new Response(bytes) : new Response('not found', { status: 404 })
  },
}
const rel = (tag, at, prs) => ({ tag_name: tag, published_at: at, draft: false, body: `${notes(prs)}\n---\n\nThe part of commit x.` })

test('the notes the release script writes read back line by line, escaping undone', () => {
  const body = notes([{ number: 3, title: 'x', body: '## What\'s new\n- Fixed: a <b> & c\n- New: Web: a row\n' }, { number: 4, title: 'Better: faster', body: '' }])
  assert.deepEqual(parseNotes(`${body}\n---\n\n**New**\n- not this\n`), [
    { kind: 'New', text: 'Web: a row', pr: 3 },
    { kind: 'Better', text: 'Faster', pr: 4 },
    { kind: 'Fixed', text: 'A <b> & c', pr: 3 },
  ])
  assert.deepEqual(parseNotes('Nothing you can see changed: internal work only.\n'), [])
})

test('days: a line on the day of its first release, tagged with every part that lists it; summary, picture, history', () => {
  const pr = n => ({ number: n, title: `New: thing ${n}`, body: '' })
  const releases = [
    rel('hub-v3', '2026-10-15T08:00:00Z', [pr(2)]),
    rel('web-v2', '2026-10-14T23:30:00Z', [pr(2), pr(3)]),        // 01:30 in Berlin: the 15th
    rel('web-v1', '2026-10-14T10:00:00Z', [pr(1)]),
    rel('mac-v1', '2026-10-15T09:00:00Z', [pr(1)]),
    rel('v375', '2026-10-13T09:00:00Z', [pr(9)]),                  // the old series: left out
    { ...rel('web-v9', '2026-10-15T10:00:00Z', [pr(8)]), draft: true },
  ]
  const summaries = parseSummaries('# x\n\nIntro.\n\n## 15 October 2026\n\nTwo things.\nAnd more.\n\n![The Desk](2026-10-15.webp)\n![Second](x.webp)\n\n## 12 October 2026\n\nOnly a summary.\n')
  const history = [{ iso: '2026-10-13', releases: 'v300 – v375', groups: [{ kind: 'Fixed', items: ['**Old fix**: words.'] }] }]
  const days = buildDays({ releases, summaries, history })
  assert.deepEqual(days.map(d => d.iso), ['2026-10-15', '2026-10-14', '2026-10-13', '2026-10-12'])
  const [d15, d14, d13, d12] = days
  assert.equal(d15.day, '15 October 2026')
  assert.equal(d15.summary, 'Two things. And more.')
  assert.deepEqual(d15.picture, { alt: 'The Desk', src: '/changelog/2026-10-15.webp' })
  assert.equal(d15.releases, 'web-v2 · hub-v3 · mac-v1')
  assert.deepEqual(d15.groups, [{ kind: 'New', items: [{ text: 'Thing 2', tag: 'Web · Hub', head: 'Thing 2', pr: 2 }, { text: 'Thing 3', tag: 'Web', head: 'Thing 3', pr: 3 }] }])
  assert.deepEqual(d14.groups[0].items.map(i => [i.head, i.tag]), [['Thing 1', 'Web · Mac']])
  assert.equal(d13.releases, 'v300 – v375')
  assert.deepEqual(d13.groups, [{ kind: 'Fixed', items: [{ text: 'Words.', tag: '', head: 'Old fix', pr: null }] }])
  assert.deepEqual([d12.summary, d12.groups], ['Only a summary.', []])
  assert.equal(changelogHead('iPhone: the app no longer crashes.').tag, 'iPhone')
})

/** Cloudflare's cache as a stand-in: what is put is matched again (by its address). */
const fakeCache = () => { const m = new Map(); return { m, async match(k) { const r = m.get(String(k.url ?? k)); return r ? r.clone() : undefined }, async put(k, r) { m.set(String(k.url ?? k), r.clone()) } } }

test('the releases: read page by page without a token, up to the old series; a token only when one is set', async () => {
  forget()
  const asked = []
  const page = n => Array.from({ length: 100 }, (_, i) => ({ tag_name: `web-v${1000 - (n - 1) * 100 - i}`, published_at: '2026-10-15T10:00:00Z', body: '' }))
  const get = async (url, init) => {
    asked.push([url, init.headers.Authorization])
    const n = Number(new URL(url).searchParams.get('page'))
    return Response.json(n === 1 ? page(1) : [{ tag_name: 'web-v900' }, { tag_name: 'v375' }, { tag_name: 'v374' }])
  }
  assert.equal((await readReleases({}, { fetch: get, now: 0, cache: null })).length, 103)
  assert.deepEqual(asked.map(a => a[1]), [undefined, undefined], 'unauthenticated')
  assert.match(asked[0][0], /^https:\/\/api\.github\.com\/repos\/trommi\/trommi\/releases\?per_page=100&page=1$/)
  forget()
  await readReleases({ GITHUB_RELEASES_TOKEN: 'secret' }, { fetch: get, now: 0, cache: null })
  assert.equal(asked.at(-1)[1], 'Bearer secret')
  forget()
})

test('the releases: fresh ten minutes in the isolate and in Cloudflare\'s cache; on any GitHub error the last good list, asked again in two minutes', async () => {
  forget()
  const cache = fakeCache()
  let calls = 0, status = 200
  const get = async () => { calls++; return status === 200 ? Response.json([{ tag_name: 'web-v1', published_at: '2026-10-15T10:00:00Z', body: '' }]) : new Response('rate limited', { status }) }
  assert.equal((await readReleases({}, { fetch: get, now: 0, cache })).length, 1)
  forget()   // another isolate of the same data centre: from the cache, GitHub not asked
  await readReleases({}, { fetch: get, now: 599_000, cache })
  assert.equal(calls, 1)
  forget()
  status = 403   // limited: the last good list, and GitHub not asked again for two minutes
  assert.equal((await readReleases({}, { fetch: get, now: 700_000, cache })).length, 1)
  assert.equal(calls, 2)
  forget()
  assert.equal((await readReleases({}, { fetch: get, now: 760_000, cache })).length, 1)
  assert.equal(calls, 2, 'within two minutes of the failure: not asked')
  await readReleases({}, { fetch: get, now: 830_000, cache })
  assert.equal(calls, 3, 'after two minutes: asked again')
  forget()
  await assert.rejects(readReleases({}, { fetch: async () => { throw new Error('offline') }, now: 0, cache: fakeCache() }), 'nothing kept: no releases')
  forget()
})

test('/whats-new.json: JSON from the releases, the summaries and the history in public/changelog/; GitHub down: still the history', async () => {
  forget()
  const get = async () => Response.json([rel('web-v1', new Date().toISOString(), [{ number: 7, title: 'New: today\'s thing', body: '' }])])
  const r = await whatsNew(new Request('https://app.trommi.com/whats-new.json'), { ASSETS }, { fetch: get, cache: null })
  assert.equal(r.status, 200)
  assert.match(r.headers.get('Content-Type'), /^application\/json/)
  assert.equal(r.headers.get('Cache-Control'), 'public, max-age=600')
  const { days } = await r.json()
  assert.equal(days[0].groups[0].items[0].head, 'Today\'s thing')
  const history = JSON.parse(await readFile(new URL('changelog/history.json', PUBLIC), 'utf8'))
  for (const h of history) assert.ok(days.some(d => d.iso === h.iso && d.groups.length), `history day ${h.iso}`)
  assert.ok(days.some(d => d.picture?.src === '/changelog/2026-10-12.webp'), 'a summary\'s picture')
  forget()
  // through the worker, GitHub not answering (its fetch a stand-in that fails): JSON with the history, kept shortly
  const real = globalThis.fetch
  globalThis.fetch = async () => { throw new Error('offline') }
  let down
  try { down = await worker.fetch(new Request('https://app.trommi.com/whats-new.json'), { ASSETS }) } finally { globalThis.fetch = real }
  assert.equal(down.status, 200)
  assert.equal(down.headers.get('Cache-Control'), 'public, max-age=60')
  assert.ok((await down.json()).days.length >= history.length)
  forget()
})
