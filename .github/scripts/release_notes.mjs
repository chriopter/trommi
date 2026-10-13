#!/usr/bin/env node
// A release's notes (sign-release.yml for web-v, hub-v and connector-v; mac_release.sh for mac-v; ship-apple.yml for ios-v): the pull requests
// merged since the series' release before that touch the part, their "What's new" lines under New, Better and Fixed.
//   node .github/scripts/release_notes.mjs <part> <commit of the release before, or empty> <commit>
// <part>: web, hub, connector or mac (the Mac app: the iOS paths); its paths are those .github/scripts/inputs.sh
// builds it from (`inputs.sh paths <part>`). GH_TOKEN and GH_REPO (owner/name) for the GitHub API.
//
// Where a line comes from: a pull request's body, its section "## What's new" (.github/pull_request_template.md),
// one list item per line, each starting with "New:", "Better:" or "Fixed:" (else the kind of the PR's title). An empty
// section (or "- none") means internal: nothing listed. A PR without the section: its title (without "<date>-<n>: "),
// internal ones (tests, CI, refactors) left out.
//
// The notes are What's new's source (app/web/whats-new.mjs parseNotes reads them back):
//   **New**
//   - <the line> (#<PR>)
//   **Better** …  **Fixed** …
// "<", ">" and "&" outside `code` are escaped, or GitHub drops "<…>" as HTML. Nothing to list: one line saying so.
// Never fails: on an error it prints nothing (and says why on stderr), so the notes can never stop a release.
import { execFileSync } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

export const KINDS = ['New', 'Better', 'Fixed']
export const NOTHING = 'Nothing you can see changed: internal work only.'
const KIND = { New: 'New', Better: 'Better', Updated: 'Better', Removed: 'Better', Fixed: 'Fixed' }
const KIND_RE = /^(New|Better|Updated|Removed|Fixed): +/
const INTERNAL = /\b(tests?|CI|workflows?|build\.yml|lint|refactors?|fuzz)\b/i

/** &, < and > as entities outside `code`. */
export function esc(s) {
  return String(s).split('`').map((p, i, all) => (i % 2 === 0 || i === all.length - 1 ? p.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;') : p)).join('`')
}

/** A PR title's kind: New, Better or Fixed ('' when it starts with none). */
const titleKind = title => KIND[KIND_RE.exec(String(title).replace(/^\d{4}-\d{2}-\d{2}-\d+: */, ''))?.[1]] ?? ''

/** The body's section "## What's new" as its list items (a line indented under one goes on with it); null without it. */
export function section(body) {
  const lines = String(body ?? '').replace(/<!--[\s\S]*?-->/g, '').split(/\r?\n/)
  const at = lines.findIndex(l => /^#{2,3} +What['’]s new *$/i.test(l.trim()))
  if (at < 0) return null
  const items = []
  for (const line of lines.slice(at + 1)) {
    if (/^#{1,3} /.test(line)) break
    const item = /^ {0,3}[-*] +(.*)$/.exec(line)
    if (item) items.push(item[1].trim())
    else if (/^\s+\S/.test(line) && items.length) items[items.length - 1] += ` ${line.trim()}`
  }
  return items.filter(t => t && !/^(none|nothing|n\/a)\.?$/i.test(t))
}

/** A pull request ({ number, title, body }) as its lines: [{ kind, text, pr }]. */
export function prLines(pr) {
  const fallback = titleKind(pr.title) || 'Better'
  const items = section(pr.body)
  if (items) {
    return items.map(text => {
      const k = KIND_RE.exec(text)
      return { kind: k ? KIND[k[1]] : fallback, text: k ? text.slice(k[0].length).replace(/^./, c => c.toUpperCase()) : text, pr: pr.number }
    })
  }
  let s = String(pr.title ?? '').trim().replace(/^\d{4}-\d{2}-\d{2}-\d+: */, '').replace(/ *\(#\d+\)$/, '')
  if (!s || /^Merge /.test(s) || INTERNAL.test(s)) return []
  const k = KIND_RE.exec(s)
  if (k) s = s.slice(k[0].length)
  // "New: a, Fixed: b" in one title: each its own line
  const out = []
  let kind = k ? KIND[k[1]] : fallback
  for (const [i, piece] of s.split(/[,;]? +(New|Better|Updated|Removed|Fixed): +/).entries()) {
    if (i % 2 === 1) { kind = KIND[piece]; continue }
    const text = piece.trim()
    if (text) out.push({ kind, text: text.replace(/^./, c => c.toUpperCase()), pr: pr.number })
  }
  return out
}

/** The notes for these pull requests, oldest first: their lines by kind, each once. */
export function notes(prs) {
  const groups = new Map(KINDS.map(k => [k, []])), seen = new Set()
  for (const pr of prs) {
    for (const l of prLines(pr)) {
      const text = l.text.replace(/\s+/g, ' ')
      if (seen.has(text.toLowerCase())) continue
      seen.add(text.toLowerCase())
      groups.get(l.kind).push(`- ${esc(text)}${l.pr ? ` (#${l.pr})` : ''}`)
    }
  }
  const out = []
  for (const [kind, lines] of groups) if (lines.length) out.push(`**${kind}**`, ...lines, '')
  return out.length ? out.join('\n') : `${NOTHING}\n`
}

/** Whether a file is one of these paths ("dir/" a folder, else one file). */
export const touches = (file, paths) => paths.some(p => (p.endsWith('/') ? file.startsWith(p) : file === p))

/** The pull requests merged between `before` and `commit` (only `commit`'s own when there is no release before) that
 *  touch these paths, oldest first. `api(path)` answers GitHub's REST API for the repository (JSON). */
export async function gather({ before, commit, paths, api }) {
  const shas = before ? ((await api(`compare/${before}...${commit}`)).commits ?? []).map(c => c.sha) : [commit]
  const prs = [], numbers = new Set()
  for (const sha of shas) {
    const files = ((await api(`commits/${sha}`)).files ?? []).map(f => f.filename)
    if (!files.some(f => touches(f, paths))) continue
    const pr = ((await api(`commits/${sha}/pulls`)) ?? []).find(p => p.merged_at)
    if (!pr || numbers.has(pr.number)) continue
    numbers.add(pr.number)
    prs.push({ number: pr.number, title: pr.title, body: pr.body ?? '' })
  }
  return prs
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [part = '', before = '', commit = ''] = process.argv.slice(2)
  try {
    const here = path.dirname(fileURLToPath(import.meta.url))
    const paths = execFileSync('sh', [path.join(here, 'inputs.sh'), 'paths', part === 'mac' ? 'ios' : part], { encoding: 'utf8' }).split(/\s+/).filter(Boolean)
    if (!paths.length || !/^[0-9a-f]{40}$/.test(commit) || (before && !/^[0-9a-f]{40}$/.test(before))) throw new Error('usage: release_notes.mjs <part> <before|""> <commit>')
    const api = async p => {
      const r = await fetch(`https://api.github.com/repos/${process.env.GH_REPO}/${p}`, { headers: { Accept: 'application/vnd.github+json', Authorization: `Bearer ${process.env.GH_TOKEN}`, 'User-Agent': 'trommi-release-notes' } })
      if (!r.ok) throw new Error(`${p}: HTTP ${r.status}`)
      return r.json()
    }
    process.stdout.write(notes(await gather({ before, commit, paths, api })))
  } catch (e) {
    process.stderr.write(`release_notes: ${e.message}\n`)
  }
}
