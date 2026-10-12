#!/usr/bin/env node
// A release's notes (release.yml, job publish): the newest day of CHANGELOG.md, short. Releases of the same day
// repeat it as it grows.
//   node .github/scripts/release_notes.mjs <CHANGELOG.md of this release> [<this tag> <published releases>]
// <published releases>: a file of lines "<tag> <created, ISO 8601 UTC>" (gh release list). The heading gets the day's
// releases in brackets, counted from them: every release v<N> made on that day (Europe/Berlin), this tag included
// when the day is today. CHANGELOG.md itself names no release numbers.
// What is kept of the day: the first sentence of its summary, its picture, and under New, Better and Fixed one line
// per change: its short form as What's new shows it (changelogHead: the bold start, else the start up to the first
// colon, comma or full stop; cut at 120 characters here, not 78; one of fewer than four words reads on to the first
// colon or full stop), a "Web:"/"iPhone:" tag after it in brackets. A part named twice is one part; a line whose
// short form is there already, and text outside a part's list, are left out.
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

/** The notes for CHANGELOG.md's text; '' when it has no day. */
export function releaseNotes(changelog, { tag = '', list = '', base = '' } = {}) {
  const lines = String(changelog).split(/\r?\n/)
  const start = lines.findIndex(l => /^## /.test(l))
  if (start < 0) return ''
  let end = lines.findIndex((l, i) => i > start && /^## /.test(l))
  if (end < 0) end = lines.length
  const day = lines[start].replace(/^## */, '').replace(/ *\([^)]*\) *$/, '').trim()
  const when = new Date(`${day} 12:00 UTC`)
  const iso = Number.isNaN(when.getTime()) ? '' : when.toISOString().slice(0, 10)

  // the block's lines joined where they go on (hard-wrapped)
  const items = []
  for (const raw of lines.slice(start + 1, end)) {
    if (/^\s*$/.test(raw)) { items.push(''); continue }
    if (/^([-*+] |[0-9]+\. |#|\*\*[^*]+\*\* *$|!\[|\||>)/.test(raw) || !items.length || items.at(-1) === '') items.push(raw.trim())
    else items[items.length - 1] += ` ${raw.trim()}`
  }

  let summary = '', picture = '', part = null
  const parts = new Map(), seen = new Set()
  for (const item of items) {
    if (!item) continue
    const name = /^\*\*([^*]+)\*\* *$/.exec(item)
    if (name) { part = name[1].trim(); if (!parts.has(part)) parts.set(part, []); continue }
    const pic = /^!\[([^\]]*)\]\(([^:)]+)\) *$/.exec(item)
    if (pic) { if (!picture) picture = `![${pic[1]}](${base}${pic[2]})`; continue }
    const li = /^[-*+] +(.*)$/.exec(item)
    if (li && part) {
      const { tag: t, head } = noteHead(li[1])
      const line = t ? `${head} (${t})` : head
      if (seen.has(line.toLowerCase())) continue
      seen.add(line.toLowerCase())
      parts.get(part).push(line)
      continue
    }
    if (!part && !summary) summary = item.split(/(?<=[.!?])\s+/)[0]
  }

  const out = [`## ${day}${iso ? range(iso, tag, list) : ''}`, '']
  if (summary) out.push(esc(summary), '')
  if (picture) out.push(picture, '')
  for (const [name, list] of parts) {
    if (!list.length) continue
    out.push(`**${name}**`, ...list.map(l => `- ${esc(l)}`), '')
  }
  return out.join('\n').replace(/\n+$/, '\n')
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    const [file, tag = '', list = ''] = process.argv.slice(2)
    if (file && fs.existsSync(file)) process.stdout.write(releaseNotes(fs.readFileSync(file, 'utf8'), { tag, list, base: process.env.NOTES_BASE || '' }))
  } catch {}
}
