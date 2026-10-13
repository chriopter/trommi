// What's new as data, served by the worker at /whats-new.json (worker.js) for the web app's page (public/news.mjs)
// and the iPhone's (WhatsNew.swift). Three sources, newest day first:
//   - the GitHub releases of every part (web-v, hub-v, connector-v, mac-v): their notes list the merged pull requests'
//     "What's new" lines (.github/scripts/release_notes.mjs writes them, parseNotes reads them back). A line counts on
//     the day (Europe/Berlin) of the first release that lists it; later releases of other parts only add their tag.
//   - public/changelog/CHANGELOG.md: one short summary per day and at most one picture (written by hand, once a day).
//   - public/changelog/history.json: the days before the releases had notes (the old CHANGELOG.md, frozen).
// The releases come from GitHub's API with a read-only token (the worker's secret GITHUB_RELEASES_TOKEN; without it,
// unauthenticated), kept for ten minutes in the isolate and in Cloudflare's cache. When GitHub does not answer, the
// last list kept, else none: the page still has the summaries and the history.
//
// The answer: { days: [{ day: '13 October 2026', iso: '2026-10-13', releases: 'web-v40 – 44 · hub-v12', summary,
// picture: { alt, src: '/changelog/<name>.webp' } | null, groups: [{ kind: 'New' | 'Better' | 'Fixed', items:
// [{ text, tag, head, pr }] }] }] }. `head` is a line's short form (changelogHead), `tag` where it shows (a leading
// "Web:" of the line, else the parts whose releases list it: "Web · iPhone").

export const WHATS_NEW = '/whats-new.json'
export const SUMMARIES = '/changelog/CHANGELOG.md'
export const HISTORY = '/changelog/history.json'
export const REPO = 'trommi/trommi'
const TTL = 600_000        // how long the releases are kept (ms)
const PAGES = 10           // at most 1000 releases are read
const KINDS = ['New', 'Better', 'Fixed']
/** A part's release series and the name its lines are tagged with. */
export const PARTS = { web: 'Web', hub: 'Hub', connector: 'Connector', mac: 'Mac' }
const SERIES = /^(web|hub|connector|mac)-v([1-9][0-9]*)$/

/** A line and its short form: { text, tag, head }. A bold start ("**Readable addresses**: …") is the head; else a
 *  leading "Web:", "iPhone:" (iPad, Mac, Desk, Codex, Connector) becomes the tag, and the head is the line up to its
 *  first colon, semicolon, bracket, comma or full stop (when that leaves fewer than three words, up to the first colon,
 *  semicolon or full stop), at most 78 characters. */
export function changelogHead(line) {
  let text = String(line).trim(), tag = '', head
  const bold = /^\*\*([^*]+)\*\*/.exec(text)
  if (bold) { head = bold[1].trim(); text = text.slice(bold[0].length).replace(/^:\s*/, ''); text = text.charAt(0).toUpperCase() + text.slice(1) }
  const TAG = /^(Web|iPhone|iPad|Mac|Desk|Codex|Connector): +/
  let rest = text
  if (bold) { const t = TAG.exec(head); if (t) { tag = t[1]; head = head.slice(t[0].length) } }
  else { const t = TAG.exec(text); if (t) { tag = t[1]; rest = text = text.slice(t[0].length).replace(/^./, c => c.toUpperCase()) } }
  if (!head) {
    const plain = rest.replace(/\[([^\]]+)\]\([^)]+\)/g, '$1').replace(/`/g, '')
    head = plain.split(/[:;]\s|\s\(|,\s|\.\s|\.$/)[0]
    if (head.split(' ').length < 3) head = plain.split(/[:;]\s|\.\s|\.$/)[0]
    if (head.length > 78) head = `${head.slice(0, 75).replace(/\s+\S*$/, '')}…`
  }
  if (!/^i[A-Z]/.test(head)) head = head.charAt(0).toUpperCase() + head.slice(1)
  return { text, tag, head }
}

const unesc = s => s.replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&')

/** A release's notes as their lines: [{ kind, text, pr }] (the part under "---" is not read). */
export function parseNotes(body) {
  const out = []
  let kind = null
  for (const line of String(body ?? '').split(/\r?\n/)) {
    if (/^---+\s*$/.test(line)) break
    const k = /^\*\*(New|Better|Fixed)\*\*\s*$/.exec(line)
    if (k) { kind = k[1]; continue }
    const item = kind && /^- +(.*?)(?: \(#([0-9]+)\))?\s*$/.exec(line)
    if (item && item[1]) out.push({ kind, text: unesc(item[1]), pr: item[2] ? Number(item[2]) : null })
  }
  return out
}

const MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December']
/** "2026-10-13" from "13 October 2026" (null when it is no such date). */
export function isoOf(day) {
  const m = /^(\d{1,2}) ([A-Z][a-z]+) (\d{4})$/.exec(String(day).trim())
  const month = m ? MONTHS.indexOf(m[2]) : -1
  return month < 0 ? null : `${m[3]}-${String(month + 1).padStart(2, '0')}-${m[1].padStart(2, '0')}`
}
/** "13 October 2026" from "2026-10-13". */
export const dayOf = iso => { const [y, m, d] = iso.split('-').map(Number); return `${d} ${MONTHS[m - 1]} ${y}` }
const berlin = new Intl.DateTimeFormat('en-CA', { timeZone: 'Europe/Berlin', year: 'numeric', month: '2-digit', day: '2-digit' })
/** The day (Europe/Berlin) of a moment, as "2026-10-13". */
export const berlinDay = at => berlin.format(new Date(at))

/** public/changelog/CHANGELOG.md as { iso: { summary, picture } }: a day is a "## 13 October 2026" heading, its plain
 *  lines the summary, a line "![words](<name>.webp)" its one picture (served at /changelog/<name>.webp). */
export function parseSummaries(text) {
  const out = {}
  let at = null
  for (const line of String(text ?? '').split(/\r?\n/)) {
    const head = /^## +(.+?)\s*$/.exec(line)
    if (head) { const iso = isoOf(head[1]); at = iso ? (out[iso] = { summary: '', picture: null }) : null; continue }
    if (!at || !line.trim()) continue
    const pic = /^!\[([^\]]*)\]\((?:\.\/)?([\w.-]+\.webp)\)\s*$/.exec(line.trim())
    if (pic) { at.picture ??= { alt: pic[1], src: `/changelog/${pic[2]}` }; continue }
    at.summary = `${at.summary} ${line.trim()}`.trim()
  }
  return out
}

/** The tags of a day's releases, short: "web-v40 – 44 · hub-v12". */
function releasesLine(tags) {
  const by = new Map()
  for (const t of tags) { const m = SERIES.exec(t); if (m) by.set(m[1], [...(by.get(m[1]) ?? []), Number(m[2])]) }
  return [...by].map(([part, ns]) => { const lo = Math.min(...ns), hi = Math.max(...ns); return lo === hi ? `${part}-v${lo}` : `${part}-v${lo} – ${hi}` }).join(' · ')
}

/** The days of What's new from the releases ([{ tag_name, published_at, body }], any order), the summaries
 *  (parseSummaries) and the history ([{ iso, releases, groups: [{ kind, items: [text] }] }]), newest first. */
export function buildDays({ releases = [], summaries = {}, history = [] } = {}) {
  const days = new Map()
  const dayAt = iso => { if (!days.has(iso)) days.set(iso, { iso, tags: [], releases: '', items: [] }); return days.get(iso) }
  for (const h of history) {
    const d = dayAt(h.iso)
    d.releases = h.releases ?? ''
    for (const g of h.groups ?? []) for (const text of g.items ?? []) d.items.push({ kind: g.kind, text, pr: null, parts: [] })
  }
  const seen = new Map()
  const ours = releases.filter(r => SERIES.test(r.tag_name ?? '') && !r.draft && (r.published_at || r.created_at))
  ours.sort((a, b) => Date.parse(a.published_at || a.created_at) - Date.parse(b.published_at || b.created_at))
  for (const r of ours) {
    const part = SERIES.exec(r.tag_name)[1]
    const d = dayAt(berlinDay(r.published_at || r.created_at))
    d.tags.push(r.tag_name)
    for (const l of parseNotes(r.body)) {
      const key = `${l.pr ?? ''}|${l.text.replace(/\s+/g, ' ').toLowerCase()}`
      const had = seen.get(key)
      if (had) { if (!had.parts.includes(part)) had.parts.push(part); continue }
      const item = { kind: l.kind, text: l.text, pr: l.pr, parts: [part] }
      seen.set(key, item)
      d.items.push(item)
    }
  }
  for (const iso of Object.keys(summaries)) dayAt(iso)
  return [...days.values()].sort((a, b) => (a.iso < b.iso ? 1 : -1)).map(d => {
    const groups = []
    for (const kind of KINDS) {
      const items = d.items.filter(i => i.kind === kind).map(i => {
        const h = changelogHead(i.text)
        return { text: h.text, tag: h.tag || i.parts.map(p => PARTS[p]).join(' · '), head: h.head, pr: i.pr }
      })
      if (items.length) groups.push({ kind, items })
    }
    const s = summaries[d.iso] ?? {}
    return { day: dayOf(d.iso), iso: d.iso, releases: [d.releases, releasesLine(d.tags)].filter(Boolean).join(' · '), summary: s.summary ?? '', picture: s.picture ?? null, groups }
  }).filter(d => d.groups.length || d.summary)
}

let kept = null   // { at, releases }: the releases last read from GitHub, in this isolate

/** The releases of the four series, newest first, read from GitHub (kept for ten minutes); the old v<N> series and
 *  everything after it is not read. Throws when GitHub does not answer and nothing is kept. */
export async function readReleases(env, { fetch: get = fetch, now = Date.now() } = {}) {
  if (kept && now - kept.at < TTL) return kept.releases
  try {
    const headers = { Accept: 'application/vnd.github+json', 'User-Agent': 'trommi-app-whats-new', 'X-GitHub-Api-Version': '2022-11-28' }
    if (env?.GITHUB_RELEASES_TOKEN) headers.Authorization = `Bearer ${env.GITHUB_RELEASES_TOKEN}`
    const all = []
    for (let page = 1; page <= PAGES; page++) {
      const r = await get(`https://api.github.com/repos/${REPO}/releases?per_page=100&page=${page}`, { headers })
      if (!r.ok) throw new Error(`GitHub answered ${r.status}`)
      const list = await r.json()
      if (!Array.isArray(list)) throw new Error('GitHub answered no list')
      all.push(...list.map(({ tag_name, published_at, created_at, draft, body }) => ({ tag_name, published_at, created_at, draft, body })))
      // the releases before the per-part series (v<N>): their days are the history
      if (list.length < 100 || list.some(x => /^v[0-9]+$/.test(x.tag_name ?? ''))) break
    }
    kept = { at: now, releases: all }
    return all
  } catch (err) {
    if (kept) { kept.at = now - TTL + 60_000; return kept.releases }   // asked again in a minute
    throw err
  }
}
/** Forget the releases kept (the tests). */
export const forget = () => { kept = null }

/** The answer at /whats-new.json: the days as JSON. ASSETS gives the summaries and the history. */
export async function whatsNew(request, env, { fetch: get = fetch, now = Date.now() } = {}) {
  const cache = globalThis.caches?.default
  const key = new Request(new URL(WHATS_NEW, request.url).toString())
  const hit = cache && await cache.match(key).catch(() => null)
  if (hit) return hit
  const asset = async (p, as) => { const r = await env.ASSETS.fetch(new Request(new URL(p, request.url))); return r.ok ? r[as]() : null }
  const [text, history] = await Promise.all([asset(SUMMARIES, 'text').catch(() => null), asset(HISTORY, 'json').catch(() => null)])
  let releases = [], fresh = true
  try { releases = await readReleases(env, { fetch: get, now }) } catch { fresh = false }
  const body = JSON.stringify({ days: buildDays({ releases, summaries: parseSummaries(text ?? ''), history: Array.isArray(history) ? history : [] }) })
  const res = new Response(request.method === 'HEAD' ? null : body, {
    headers: { 'Content-Type': 'application/json; charset=utf-8', 'Cache-Control': `public, max-age=${fresh ? 600 : 60}`, 'Access-Control-Allow-Origin': '*', 'X-Content-Type-Options': 'nosniff' },
  })
  if (cache && request.method !== 'HEAD') await cache.put(key, new Response(body, res)).catch(() => {})
  return res
}
