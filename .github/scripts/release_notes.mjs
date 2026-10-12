#!/usr/bin/env node
// A release's notes (release.yml, job publish): what changed since the release before, short.
//   node .github/scripts/release_notes.mjs <CHANGELOG.md> [<this tag> <published releases>
//     [<CHANGELOG.md of the release before> <its tag> [<commit subjects since it>]]]
// With the release before: the lines CHANGELOG.md added since then (a line counts as there before when its text, its
// text without a bold start or tag, that text's first 60 characters, or its short form was), under the heading of the
// newest day with new lines and "(since v<N>)"; the day's summary and picture only when they are new too. When
// CHANGELOG.md added nothing: the commit subjects (PR titles) since, by their New:/Fixed:/Updated: start, internal ones
// (tests, CI, refactors) left out; none left: a line saying so.
// Without it: the whole newest day, its heading with the day's releases in brackets, counted from <published
// releases> (lines "<tag> <created, ISO 8601 UTC>"): every v<N> made that day (Europe/Berlin), this tag included.
// Each line is the short form What's new shows (changelogHead: the bold start, else the start up to the first colon,
// comma or full stop; cut at 120 characters here, not 78; one of fewer than four words reads on to the first colon
// or full stop), a "Web:"/"iPhone:" tag after it in brackets, under New, Better, Fixed in that order. Repeated
// lines and text outside a part's list are left out; the summary is its first sentence.
// "<", ">" and "&" outside `code` are escaped, or GitHub drops "<…>" as HTML.
// NOTES_BASE (e.g. https://raw.githubusercontent.com/<repo>/<commit>/): put in front of the picture's path, which
// CHANGELOG.md gives from the repository's root.
// Prints nothing when there is no such day; never fails, so the notes can never stop a release.
import fs from 'node:fs'
import { fileURLToPath } from 'node:url'

/** A line's short form; the same as app/web/dev/build.mjs changelogHead (tests/web/views/news.test.mjs checks it). */
export function changelogHead(line, max = 78) {
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
    if (head.length > max) head = `${head.slice(0, max - 3).replace(/\s+\S*$/, '')}…`
  }
  if (!/^i[A-Z]/.test(head)) head = head.charAt(0).toUpperCase() + head.slice(1)
  return { text, tag, head }
}

/** A line's short form for the notes: changelogHead, cut at 120 characters; a start of fewer than four words that is
 *  not bold ("Settings", "After logging out") reads on to the first colon or full stop, since the notes have no more. */
export function noteHead(line) {
  const short = changelogHead(line, 120)
  if (/^\*\*/.test(String(line).trim()) || short.head.split(' ').length >= 4) return short
  const plain = short.text.replace(/\[([^\]]+)\]\([^)]+\)/g, '$1').replace(/`/g, '')
  let head = plain.split(/[:;]\s|\.\s|\.$/)[0]
  if (head.length > 120) head = `${head.slice(0, 117).replace(/\s+\S*$/, '')}…`
  return { ...short, head: head.charAt(0).toUpperCase() + head.slice(1) }
}

/** &, < and > as entities outside `code`. */
function esc(s) {
  return s.split('`').map((p, i, all) => (i % 2 === 0 || i === all.length - 1 ? p.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;') : p)).join('`')
}

/** v<N> released on <iso day> (Europe/Berlin), this tag counted as released now. */
function range(iso, tag, list) {
  const berlin = new Intl.DateTimeFormat('en-CA', { timeZone: 'Europe/Berlin', year: 'numeric', month: '2-digit', day: '2-digit' })
  const rows = []
  try { if (list) rows.push(...fs.readFileSync(list, 'utf8').split('\n')) } catch {}
  if (tag) rows.push(`${tag} ${new Date().toISOString()}`)
  const numbers = new Set()
  for (const row of rows) {
    const [t, at] = row.trim().split(/\s+/)
    const n = /^v([1-9][0-9]*)$/.exec(t || '')
    const when = new Date(at || '')
    if (n && !Number.isNaN(when.getTime()) && berlin.format(when) === iso) numbers.add(Number(n[1]))
  }
  const sorted = [...numbers].sort((a, b) => a - b)
  if (!sorted.length) return ''
  return sorted.length === 1 ? ` (v${sorted[0]})` : ` (v${sorted[0]} – v${sorted.at(-1)})`
}

/** CHANGELOG.md as days: { day, summary, picture: { alt, path }, parts: [[name, [line text]]] }, newest first. */
export function readDays(changelog) {
  const days = []
  let at = null, part = null, last = null
  const add = text => {
    if (!at) return
    const name = /^\*\*([^*]+)\*\* *$/.exec(text)
    if (name) { part = name[1].trim(); if (!at.parts.has(part)) at.parts.set(part, []); last = null; return }
    const pic = /^!\[([^\]]*)\]\(([^:)]+)\) *$/.exec(text)
    if (pic) { if (!at.picture) at.picture = { alt: pic[1], path: pic[2] }; last = null; return }
    const li = /^[-*+] +(.*)$/.exec(text)
    if (li && part) { at.parts.get(part).push(li[1]); last = { list: at.parts.get(part) }; return }
    last = null
    if (!part && !at.summary) at.summary = text
  }
  // a line that goes on (hard-wrapped) is joined to the one before
  let cur = null
  const flush = () => { if (cur !== null) add(cur); cur = null }
  for (const raw of String(changelog).split(/\r?\n/)) {
    if (/^## /.test(raw)) {
      flush()
      at = { day: raw.replace(/^## */, '').replace(/ *\([^)]*\) *$/, '').trim(), summary: '', picture: null, parts: new Map() }
      part = null
      days.push(at)
    } else if (/^\s*$/.test(raw)) flush()
    else if (/^([-*+] |[0-9]+\. |#|\*\*[^*]+\*\* *$|!\[|\||>)/.test(raw) || cur === null) { flush(); cur = raw.trim() }
    else cur += ` ${raw.trim()}`
  }
  flush()
  return days.map(d => ({ ...d, parts: [...d.parts] }))
}

const norm = s => String(s).replace(/\s+/g, ' ').trim().toLowerCase()
const ORDER = ['New', 'Better', 'Fixed']
const byOrder = (a, b) => (ORDER.indexOf(a) + 1 || 99) - (ORDER.indexOf(b) + 1 || 99)

/** A commit subject (a PR's title) as { part, line }, or null when it is internal (tests, CI, refactors). */
export function subjectLine(subject) {
  let s = String(subject).trim().replace(/^\d{4}-\d{2}-\d{2}-\d+: */, '').replace(/ *\(#\d+\)$/, '')
  if (!s || /^Merge /.test(s)) return null
  if (/\b(tests?|CI|workflows?|build\.yml|lint|refactors?|fuzz)\b/i.test(s)) return null
  const kind = /^(New|Better|Updated|Fixed|Removed): +/.exec(s)
  let part = 'Other'
  if (kind) { part = { New: 'New', Better: 'Better', Updated: 'Better', Fixed: 'Fixed', Removed: 'Better' }[kind[1]]; s = s.slice(kind[0].length) }
  s = s.split(/;? +(?:New|Better|Updated|Fixed|Removed): /)[0]
  const { tag, head } = noteHead(s)
  return { part, line: tag ? `${head} (${tag})` : head }
}

/** The notes for CHANGELOG.md's text; '' when it has no day.
 *  previous: CHANGELOG.md of the release before (since: its tag): only what was added since then is listed; without
 *  it, the whole newest day. subjects: the commit subjects since that release, listed when CHANGELOG.md added nothing. */
export function releaseNotes(changelog, { tag = '', list = '', base = '', previous = null, since = '', subjects = [] } = {}) {
  const days = readDays(changelog)
  if (!days.length) return ''
  const before = previous == null ? null : readDays(previous)
  const oldText = new Set(), oldHead = new Set(), oldPics = new Set(), oldSummary = new Set()
  for (const d of before || []) {
    if (d.picture) oldPics.add(d.picture.path)
    if (d.summary) oldSummary.add(norm(d.summary))
    for (const [, lines] of d.parts) for (const l of lines) { const h = noteHead(l); oldText.add(norm(l)); oldText.add(norm(h.text)); oldText.add(norm(h.text).slice(0, 60)); oldHead.add(norm(h.head)) }
  }
  const parts = new Map(), seen = new Set()
  let day = null, summary = '', picture = null
  for (const d of before ? days : days.slice(0, 1)) {
    let any = false
    for (const [name, lines] of d.parts) {
      for (const l of lines) {
        const { tag: t, head, text } = noteHead(l)
        if (before && (oldText.has(norm(l)) || oldText.has(norm(text)) || oldText.has(norm(text).slice(0, 60)) || oldHead.has(norm(head)))) continue
        const line = t ? `${head} (${t})` : head
        if (seen.has(norm(line))) continue
        seen.add(norm(line))
        if (!parts.has(name)) parts.set(name, [])
        parts.get(name).push(line)
        any = true
      }
    }
    if (any && !day) day = d
  }
  day ||= days[0]
  const top = days[0]
  if (top.summary && !(before && oldSummary.has(norm(top.summary)))) summary = top.summary.split(/(?<=[.!?])\s+/)[0]
  if (top.picture && !(before && oldPics.has(top.picture.path))) picture = top.picture
  if (before && !parts.size) {
    for (const sub of subjects) {
      const s = subjectLine(sub)
      if (!s || seen.has(norm(s.line))) continue
      seen.add(norm(s.line))
      if (!parts.has(s.part)) parts.set(s.part, [])
      parts.get(s.part).push(s.line)
    }
  }
  const when = new Date(`${day.day} 12:00 UTC`)
  const iso = Number.isNaN(when.getTime()) ? '' : when.toISOString().slice(0, 10)
  const out = [`## ${day.day}${since ? ` (since ${since})` : iso ? range(iso, tag, list) : ''}`, '']
  if (summary) out.push(esc(summary), '')
  if (picture) out.push(`![${picture.alt}](${base}${picture.path})`, '')
  for (const [name, lines] of [...parts].sort(([a], [b]) => byOrder(a, b))) out.push(`**${name}**`, ...lines.map(l => `- ${esc(l)}`), '')
  if (before && !parts.size) out.push('Nothing you can see changed: internal work only.', '')
  return out.join('\n').replace(/\n+$/, '\n')
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    // <CHANGELOG.md> [<tag> <releases> [<CHANGELOG.md of the release before> <its tag> [<commit subjects since>]]]
    const [file, tag = '', list = '', prev = '', since = '', subs = ''] = process.argv.slice(2)
    const read = f => { try { return f ? fs.readFileSync(f, 'utf8') : null } catch { return null } }
    const previous = read(prev)
    if (file && fs.existsSync(file)) {
      process.stdout.write(releaseNotes(fs.readFileSync(file, 'utf8'), {
        tag, list, base: process.env.NOTES_BASE || '',
        previous, since: previous == null ? '' : since,
        subjects: (read(subs) || '').split('\n').filter(Boolean),
      }))
    }
  } catch {}
}
