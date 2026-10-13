// Draws traced first logins (first-login.mjs) as one self-contained page: per client a waterfall with the lanes
// network, crypto, model, cache and the client's own steps; above it what the person sees (sign-in, empty, Desk, live)
// and in which order each kind of data arrives (Artifacts, archived sessions, Off your mind, Desk cards, agents
// online), with hover details; below it the top time sinks. Also prints the numbers as JSON.
//
//   node tests/client/mega/trace-view.mjs <out.html> <trace.json> [<trace.json> …]
import fs from 'node:fs'

const [outFile, ...files] = process.argv.slice(2)
const traces = files.map(f => JSON.parse(fs.readFileSync(f, 'utf8')))

const LANES = [
  ['net', 'Netz'], ['crypto', 'Krypto'], ['model', 'Modell'], ['cache', 'Cache/Speicher'], ['step', 'Schritte des Clients'],
]
/** Which lane a step of spent.ts (web) or Spent.swift (iOS) belongs to. */
function laneOf(s) {
  const [lane, name] = s
  if (lane !== 'step') return lane
  if (/^(core|log|commits)$/.test(name)) return null // (web: every call into the device, drawn from the binding's calls)
  if (/^core /.test(name)) return 'crypto'
  if (/^(model|board)/.test(name)) return 'model'
  if (/^(store|wrap|idb|cache|restore)/.test(name)) return 'cache'
  return 'step'
}
const fmt = ms => ms >= 1000 ? `${(ms / 1000).toFixed(1)} s` : `${Math.round(ms)} ms`
const kb = b => b >= 1048576 ? `${(b / 1048576).toFixed(1)} MB` : `${Math.round(b / 1024)} KB`
const esc = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c])

/** Union length of intervals. */
function union(iv) {
  iv.sort((a, b) => a[0] - b[0])
  if (!iv.length) return 0
  let total = 0, s = iv[0][0], e = iv[0][1]
  for (const [a, b] of iv) { if (a > e) { total += e - s; s = a; e = b } else e = Math.max(e, b) }
  return total + (e - s)
}

/** The kinds of data in the per-second picture (envelopes as the engine took them; MLS commits; presence events). */
const ITEM_KINDS = [
  ['artifacts', 'Artifacts'], ['off', 'Off-your-mind-Karten (geschlossen)'], ['desk', 'Desk-Karten (offen)'], ['chat', 'Chat'],
  ['names', 'Session-Namen (profile)'], ['registers', 'Register/Status'], ['archived', 'Inhalt archivierter Sessions'], ['commits', 'MLS-Commits (Epochen)'], ['presence', 'Presence (online/offline)'], ['other', 'Anderes'],
]
const ROUTE_KINDS = [['changes', 'Änderungs-Log /v1/changes'], ['joins', 'Session-Beitritte (Gruppen, Logs, Infos)'], ['desk', 'Desk-Liste /v1/desk'], ['welcomes', 'Welcomes, KeyPackages'], ['files', 'Dateien'], ['other', 'Anmeldung und Rest']]
function routeKind(name) {
  if (/\/v1\/changes/.test(name)) return 'changes'
  if (/\/v1\/groups|\/v1\/rooms\/:id\/groups|sealed-keys|recovery/.test(name)) return 'joins'
  if (/\/v1\/desk/.test(name)) return 'desk'
  if (/welcomes|key-packages/.test(name)) return 'welcomes'
  if (/\/v1\/files/.test(name)) return 'files'
  return 'other'
}
const KINDS = [
  ['artifacts', 'Artifacts'], ['sessions_archived', 'Archivierte Sessions'], ['off', 'Off your mind'],
  ['desk', 'Desk-Karten'], ['sessions_named', 'Live-Sessions mit Namen'], ['online', 'Agents online'], ['items', 'Chat-Einträge'],
]

function analyse(tr) {
  const t0 = tr.t0
  const spans = tr.spans.map(s => [laneOf(s), s[1], s[2] - t0, s[3] - t0, s[4] ?? null]).filter(s => s[0] && s[3] >= -1)
  const marks = spans.filter(s => s[0] === 'mark').map(s => [s[1], s[2]])
  const work = spans.filter(s => s[0] !== 'mark')
  const samples = tr.samples.map(([t, v]) => [t - t0, v])
  const settled = marks.find(m => m[0].startsWith('settled'))?.[1] ?? null
  const end = Math.max(...work.map(s => s[3]), ...samples.map(s => s[0]), 1)
  const fin = tr.final ?? samples.at(-1)?.[1] ?? {}
  const first = (key, pred = v => v[key] > 0) => samples.find(([, v]) => pred(v))?.[0] ?? null
  const full = key => first(key, v => v[key] >= (fin[key] ?? 0) && (fin[key] ?? 0) > 0)
  const kinds = KINDS.map(([key, label]) => ({ key, label, final: fin[key] ?? 0, first: first(key), full: full(key), series: samples.map(([t, v]) => [t, v[key] ?? 0]) }))
  const opened = marks.find(m => m[0].startsWith('room joined'))?.[1] ?? null
  // (everything there: the first moment every kind stands at its final count and the app no longer says it reads;
  //  what the engine does after that, until it settles, is drawn as its own phase)
  const keys = KINDS.map(k => k[0])
  const live = samples.find(([, v]) => !v.reading && keys.every(k => (v[k] ?? 0) >= (fin[k] ?? 0)))?.[0] ?? settled ?? end
  const net = work.filter(s => s[0] === 'net')
  const routes = new Map()
  for (const s of net) {
    const r = routes.get(s[1]) ?? { n: 0, in: 0, out: 0, busy: [] }
    r.n++; r.in += s[4]?.in ?? 0; r.out += s[4]?.out ?? 0; r.busy.push([s[2], s[3]]); routes.set(s[1], r)
  }
  const crypto = work.filter(s => s[0] === 'crypto')
  const calls = new Map()
  for (const s of crypto) { const c = calls.get(s[1]) ?? { n: 0, ms: 0 }; c.n++; c.ms += s[3] - s[2]; calls.set(s[1], c) }
  // the time sinks: per lane and name, the union of their spans (what the person waits through while it runs)
  const sinks = new Map()
  for (const s of work) { if (s[0] === 'step' && /^(welcomes|read changes|past|upkeep)$/.test(s[1]) && tr.platform === 'web') continue; const k = `${s[0]}|${s[1]}`; (sinks.get(k) ?? sinks.set(k, []).get(k)).push([s[2], s[3]]) }
  const top = [...sinks].map(([k, iv]) => { const [lane, name] = k.split('|'); return { lane, name, ms: union(iv), n: iv.length } })
    .sort((a, b) => b.ms - a.ms)
  const decrypts = crypto.filter(s => /feed|receive|open|decrypt|process|message|envelope/i.test(s[1])).length
  const commits = tr.platform === 'ios'
    ? work.filter(s => s[1] === 'core commit').length
    : null
  // the kinds of data, per envelope as the engine reported it (web), and the wire bytes per route class, per second
  const fs_ = tr.final_state ?? {}, groups = tr.groups ?? {}
  const kindOf = e => {
    const [, what, type, how, , obj, , group] = e
    if (what === 'log') return 'commits'
    if (what === 'stream') return type === 'presence' ? 'presence' : null
    if (groups[group]?.archived) return 'archived'
    if (type === 'artifact') return 'artifacts'
    if (type === 'card') return fs_[obj] === 'open' ? 'desk' : 'off'
    if (type === 'sessionChat' || type === 'cardChat' || type === 'item') return 'chat'
    if (type === 'register' && /^profile/.test(e[10] ?? '')) return 'names'
    if (type === 'request' || type === 'note') return 'other'
    return 'registers'
  }
  const sec = Math.ceil(end / 1000) + 1
  const perSec = Object.fromEntries(ITEM_KINDS.map(([k]) => [k, { n: new Array(sec).fill(0), bytes: new Array(sec).fill(0), again: 0, total: 0, totalBytes: 0, first: null, last: null }]))
  for (const e of tr.events ?? []) {
    const k = kindOf(e); if (!k) continue
    const t = e[0] - t0, i = Math.max(0, Math.min(sec - 1, Math.floor(t / 1000))), b = perSec[k]
    b.n[i]++; b.bytes[i] += e[6] ?? 0; b.total++; b.totalBytes += e[6] ?? 0
    if (e[3] === 'again') b.again++
    if (b.first == null) b.first = t
    b.last = t
  }
  const wire = Object.fromEntries(ROUTE_KINDS.map(([k]) => [k, new Array(sec).fill(0)]))
  for (const s of net) {
    const k = routeKind(s[1]), bytes = (s[4]?.in ?? 0) + (s[4]?.out ?? 0)
    // (a request's bytes spread over the seconds it ran)
    const a = Math.max(0, s[2]), b = Math.max(a + 1, s[3])
    for (let i = Math.floor(a / 1000); i <= Math.min(sec - 1, Math.floor(b / 1000)); i++) {
      const lo = Math.max(a, i * 1000), hi = Math.min(b, (i + 1) * 1000)
      if (hi > lo) wire[k][i] += bytes * (hi - lo) / (b - a)
    }
  }
  const rescans = marks.filter(m => /^rescan from|^rescan end|^catch-up from|^catch-up end|^sessions joined|^client started|^desk read/.test(m[0]))
  const reads = (tr.events ?? []).filter(e => e[1] === 'env' && e[8] != null).map(e => [e[0] - t0, e[8], kindOf(e), e[3]])
  const head = Math.max(1, ...reads.map(r => r[1]))
  return {
    reads, head,
    perSec, wire, sec, rescans,
    platform: tr.platform, wall: tr.wall_ms, end, opened, settled, live, marks, work, samples, kinds, fin,
    requests: net.length, bytesIn: net.reduce((a, s) => a + (s[4]?.in ?? 0), 0), bytesOut: net.reduce((a, s) => a + (s[4]?.out ?? 0), 0),
    routes: [...routes].map(([name, r]) => ({ name, n: r.n, in: r.in, out: r.out, busy: union(r.busy) })).sort((a, b) => b.busy - a.busy),
    calls: [...calls].map(([name, c]) => ({ name, ...c })).sort((a, b) => b.ms - a.ms),
    top, decrypts, commits, counts: tr.fixture?.counts ?? {}, error: tr.error,
  }
}

const runs = traces.map(analyse)
const W = 1100, LEFT = 170, RIGHT = 20
function chart(a, idx) {
  const span = Math.max(a.end, a.live) * 1.02
  const x = t => LEFT + (Math.max(0, t) / span) * (W - LEFT - RIGHT)
  let y = 10
  const parts = []
  const tips = []
  const tip = (text) => { tips.push(text); return `data-tip="${tips.length - 1}"` }
  // axis
  const ticks = []
  const step = [0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300].map(s => s * 1000).find(s => span / s <= 12) ?? 600000
  // what the person sees
  const seeRow = y
  const desk = a.kinds.find(k => k.key === 'desk'), online = a.kinds.find(k => k.key === 'online')
  const phases = [
    [0, a.opened ?? 0, 'Anmeldung läuft (Code wird geprüft)', 'p0'],
    [a.opened ?? 0, desk.first ?? a.live, 'App offen, Desk leer', 'p1'],
    [desk.first ?? a.live, a.live, 'Desk sichtbar, lädt noch', 'p2'],
    [a.live, a.settled && a.settled > a.live + 1000 ? a.settled : span, a.settled && a.settled > a.live + 1000 ? 'alles da, Engine kommt nicht zur Ruhe' : 'alles da', 'p3'],
    ...(a.settled && a.settled > a.live + 1000 ? [[a.settled, span, 'ruhig', 'p3']] : []),
  ].filter(p => p[1] > p[0])
  parts.push(`<text x="${LEFT - 8}" y="${y + 15}" class="lab" text-anchor="end">Was er sieht</text>`)
  for (const [s, e, label, cls] of phases) parts.push(`<rect x="${x(s)}" y="${y}" width="${Math.max(1, x(e) - x(s) - 2)}" height="22" rx="4" class="${cls}" ${tip(`${label}: ${fmt(s)} – ${fmt(e)}`)}/>` + (x(e) - x(s) > 120 ? `<text x="${x(s) + 6}" y="${y + 15}" class="in">${esc(label)}</text>` : ''))
  y += 34
  // the kinds of data: when each arrives (rising from first item to its final count)
  parts.push(`<text x="${LEFT - 8}" y="${y + 2}" class="sec" text-anchor="end">Welche Daten wann</text>`)
  y += 8
  for (const k of a.kinds) {
    const h = 20
    parts.push(`<text x="${LEFT - 8}" y="${y + 14}" class="lab" text-anchor="end">${esc(k.label)} (${k.final})</text>`)
    parts.push(`<line x1="${LEFT}" x2="${W - RIGHT}" y1="${y + h}" y2="${y + h}" class="base"/>`)
    if (k.final > 0) {
      let d = `M${x(0)},${y + h}`
      let prev = 0
      for (const [t, v] of k.series) { const yy = y + h - (Math.min(v, k.final) / k.final) * (h - 2); d += ` L${x(t)},${y + h - (Math.min(prev, k.final) / k.final) * (h - 2)} L${x(t)},${yy}`; prev = v }
      d += ` L${x(span)},${y + h - (Math.min(prev, k.final) / k.final) * (h - 2)} L${x(span)},${y + h} Z`
      parts.push(`<path d="${d}" class="kind k-${k.key}" ${tip(`${k.label}: erstes um ${k.first == null ? '–' : fmt(k.first)}, vollständig (${k.final}) um ${k.full == null ? '–' : fmt(k.full)}`)}/>`)
      if (k.first != null) parts.push(`<circle cx="${x(k.first)}" cy="${y + h - 2}" r="4" class="dot k-${k.key}"/><text x="${x(k.first) + 7}" y="${y + 9}" class="small">${fmt(k.first)}</text>`)
    }
    y += h + 8
  }
  y += 10
  // where in the history each envelope the core took lies (its change number, oldest at the bottom), over time
  {
    const h = 120
    parts.push(`<text x="${LEFT - 8}" y="${y + 2}" class="sec" text-anchor="end">Gelesen: wo im Verlauf</text>`)
    parts.push(`<text x="${LEFT - 8}" y="${y + 18}" class="small" text-anchor="end">neu ↑</text><text x="${LEFT - 8}" y="${y + h}" class="small" text-anchor="end">alt ↓</text>`)
    parts.push(`<rect x="${LEFT}" y="${y}" width="${W - LEFT - RIGHT}" height="${h}" class="plot"/>`)
    const step = Math.max(1, Math.floor(a.reads.length / 15000))
    // (the few that matter most drawn last and larger: Desk cards, Artifacts)
    const big = k => k === 'desk' || k === 'artifacts'
    for (const pass of [false, true]) for (let i = 0; i < a.reads.length; i += pass ? 1 : step) {
      const [t, change, k] = a.reads[i]
      if (big(k) !== pass) continue
      const r = pass ? 3.2 : 1.6
      parts.push(`<rect x="${x(t) - r / 2}" y="${y + h - 2 - (change / a.head) * (h - 4) - r / 2}" width="${r}" height="${r}" class="ik-${k}"/>`)
    }
    parts.push(`<rect x="${LEFT}" y="${y}" width="${W - LEFT - RIGHT}" height="${h}" fill="transparent" ${tip(`${a.reads.length} Envelopes vom Core genommen; senkrecht ihre Stelle im Verlauf (Änderungsnummer 0 bis ${a.head}). Jede schräge Linie von unten nach oben ist ein Durchgang alt → neu.`)}/>`)
    y += h + 16
  }
  // per second: stacked bars, items by kind, then wire bytes by route class (each its own scale)
  const bars = (title, series, colorOf, unit, labelOf) => {
    const h = 110
    const totals = new Array(a.sec).fill(0)
    for (const [, arr] of series) arr.forEach((v, i) => { totals[i] += v })
    const max = Math.max(1, ...totals)
    parts.push(`<text x="${LEFT - 8}" y="${y + 2}" class="sec" text-anchor="end">${esc(title)}</text>`)
    parts.push(`<text x="${LEFT - 8}" y="${y + 16}" class="small" text-anchor="end">max ${unit(max)}/s</text>`)
    parts.push(`<line x1="${LEFT}" x2="${W - RIGHT}" y1="${y + h}" y2="${y + h}" class="base"/>`)
    for (let i = 0; i < a.sec; i++) {
      let yy = y + h
      const x0 = x(i * 1000), w = Math.max(1, x((i + 1) * 1000) - x0 - 1)
      const lines = []
      for (const [k, arr] of series) {
        const v = arr[i]; if (!v) continue
        const hh = (v / max) * (h - 4)
        yy -= hh
        parts.push(`<rect x="${x0}" y="${yy}" width="${w}" height="${Math.max(0.5, hh - 0.5)}" class="${colorOf(k)}" ${tip(`Sekunde ${i}–${i + 1}: ${labelOf(k)} ${unit(v)}`)}/>`)
      }
    }
    y += h + 16
  }
  bars('Daten pro Sekunde (Anzahl)', ITEM_KINDS.map(([k]) => [k, a.perSec[k].n]), k => `ik-${k}`, v => `${Math.round(v)}`, k => ITEM_KINDS.find(i => i[0] === k)[1])
  bars('Netz pro Sekunde (Bytes)', ROUTE_KINDS.map(([k]) => [k, a.wire[k]]), k => `rk-${k}`, kb, k => ROUTE_KINDS.find(i => i[0] === k)[1])
  // lanes
  for (const [lane, label] of LANES) {
    const items = a.work.filter(s => s[0] === lane).sort((p, q) => p[2] - q[2])
    if (!items.length) continue
    // rows: overlapping spans stacked (up to 6 rows; past that they share the last); spans under half a pixel are
    // drawn merged into a busy block per row
    const rowsEnd = []
    const rows = []
    for (const s of items) {
      let r = rowsEnd.findIndex(e => e <= s[2])
      if (r < 0) r = rowsEnd.length < 6 ? rowsEnd.length : 5
      rowsEnd[r] = Math.max(rowsEnd[r] ?? 0, s[3]); (rows[r] ??= []).push(s)
    }
    const rh = lane === 'step' ? 16 : 9
    parts.push(`<text x="${LEFT - 8}" y="${y + 11}" class="lab" text-anchor="end">${label}</text>`)
    rows.forEach((row, r) => {
      let block = null
      const flush = () => { if (!block) return; const w = Math.max(1, x(block.e) - x(block.s)); parts.push(`<rect x="${x(block.s)}" y="${y + r * (rh + 2)}" width="${w}" height="${rh}" rx="2" class="l-${lane}" ${tip(block.n === 1 ? block.text : `${block.n} kurze Aufrufe ${fmt(block.s)} – ${fmt(block.e)}: ${[...block.names].slice(0, 4).join(', ')}${block.names.size > 4 ? ' …' : ''}, zusammen ${fmt(block.busy)}`)}/>`); if (lane === 'step' && w > 70) parts.push(`<text x="${x(block.s) + 4}" y="${y + r * (rh + 2) + 12}" class="in small">${esc(block.label)}</text>`); block = null }
      for (const s of row) {
        const extra = s[4]
        const text = `${s[1]} · ${fmt(s[2])} – ${fmt(s[3])} (${fmt(s[3] - s[2])})${extra?.in != null ? ` · ${kb(extra.in)} herein, ${kb(extra.out ?? 0)} hinaus${extra.query ? ` · ${extra.query.slice(0, 60)}` : ''}` : ''}`
        if (block && x(s[2]) - x(block.e) < 1.5) { block.e = Math.max(block.e, s[3]); block.n++; block.busy += s[3] - s[2]; block.names.add(s[1]); continue }
        flush()
        block = { s: s[2], e: s[3], n: 1, busy: s[3] - s[2], names: new Set([s[1]]), text, label: s[1] }
      }
      flush()
    })
    y += rows.length * (rh + 2) + 10
  }
  // axis and milestones
  for (let t = 0; t <= span; t += step) ticks.push(`<line x1="${x(t)}" x2="${x(t)}" y1="${seeRow}" y2="${y}" class="grid"/><text x="${x(t)}" y="${y + 14}" class="small" text-anchor="middle">${t / 1000} s</text>`)
  const ms = [['App offen', a.opened], ['erste Desk-Karte', desk.first], ['Desk vollständig', desk.full], ['Agents online', online.first], ['alles da', a.live]].filter(m => m[1] != null)
  const milestones = ms.map(([label, t], i) => `<line x1="${x(t)}" x2="${x(t)}" y1="${seeRow}" y2="${y}" class="ms"/><text x="${x(t) + 3}" y="${y + 30 + (i % 3) * 13}" class="ms-l">${esc(label)} ${fmt(t)}</text>`)
  const height = y + 70
  return { svg: `<svg viewBox="0 0 ${W} ${height}" width="100%" class="wf" data-run="${idx}" role="img" aria-label="Wasserfall des ersten Logins, ${a.platform}">${ticks.join('')}${parts.join('')}${milestones.join('')}</svg>`, tips }
}

const titleOf = { web: 'Web (App-Client-Schicht mit echtem Core, Node)', ios: 'iPhone (iOS-Engine mit echtem Core, Linux)' }
const sections = runs.map((a, i) => {
  const { svg, tips } = chart(a, i)
  const desk = a.kinds.find(k => k.key === 'desk'), off = a.kinds.find(k => k.key === 'off'), online = a.kinds.find(k => k.key === 'online'), art = a.kinds.find(k => k.key === 'artifacts')
  const total = a.live
  const top5 = a.top.filter(t => t.lane !== 'step' || true).slice(0, 5)
  return `<section>
<h2>${esc(titleOf[a.platform] ?? a.platform)}${a.error ? ` <span class="err">Fehler: ${esc(a.error)}</span>` : ''}</h2>
<div class="tiles">
<div class="tile"><b>${fmt(a.opened ?? 0)}</b><span>Anmeldung bis App offen</span></div>
<div class="tile"><b>${art.first == null ? '–' : fmt(art.first)}</b><span>erste Artifacts</span></div>
<div class="tile"><b>${art.full == null ? '–' : fmt(art.full)}</b><span>alle Artifacts (${art.final})</span></div>
<div class="tile"><b>${off.first == null ? '–' : fmt(off.first)}</b><span>erstes Off your mind</span></div>
<div class="tile"><b>${desk.first == null ? '–' : fmt(desk.first)}</b><span>erste Desk-Karte</span></div>
<div class="tile"><b>${desk.full == null ? '–' : fmt(desk.full)}</b><span>Desk vollständig (${desk.final})</span></div>
<div class="tile"><b>${off.full == null ? '–' : fmt(off.full)}</b><span>Off your mind vollständig (${off.final})</span></div>
<div class="tile"><b>${online.first == null ? '–' : fmt(online.first)}</b><span>Agents online</span></div>
<div class="tile strong"><b>${fmt(total)}</b><span>alles da</span></div>${a.settled && a.settled > a.live + 1000 ? `<div class="tile"><b>${fmt(a.settled)}</b><span>Engine ruhig</span></div>` : ''}
</div>
<p class="muted">${a.requests} Anfragen · ${kb(a.bytesIn)} herein, ${kb(a.bytesOut)} hinaus · ${a.calls.reduce((s, c) => s + c.n, 0)} Aufrufe in den Core${a.commits != null ? ` · ${a.commits} MLS-Commits verarbeitet` : ''}</p>
${svg}
<script type="application/json" class="tips">${JSON.stringify(tips).replace(/</g, '\\u003c')}</script>
<div class="legend">${ITEM_KINDS.map(([k, l]) => `<span><i class="ik-${k}"></i>${esc(l)}</span>`).join('')}</div>
<div class="legend">${ROUTE_KINDS.map(([k, l]) => `<span><i class="rk-${k}"></i>${esc(l)}</span>`).join('')}</div>
<h3>Was in welcher Reihenfolge kommt</h3>
<table><thead><tr><th>Art</th><th class="num">Anzahl</th><th class="num">davon zweimal gelesen</th><th class="num">Inhalt</th><th class="num">erstes</th><th class="num">letztes</th></tr></thead><tbody>
${ITEM_KINDS.map(([k, l]) => [k, l, a.perSec[k]]).filter(([, , b]) => b.total).sort((p, q) => p[2].first - q[2].first).map(([k, l, b]) => `<tr><td><i class="sw ik-${k}"></i>${esc(l)}</td><td class="num">${b.total}</td><td class="num">${b.again}</td><td class="num">${kb(b.totalBytes)}</td><td class="num">${fmt(b.first)}</td><td class="num">${fmt(b.last)}</td></tr>`).join('')}
</tbody></table>
<p class="muted small">Inhalt: entschlüsselte Bytes. „Zweimal gelesen“: im zweiten Durchlauf (Rescan) nach den Session-Beitritten noch einmal vom Core genommen.</p>
<div class="cols">
<div><h3>Die 5 größten Zeitfresser</h3><table><thead><tr><th>Was</th><th>Spur</th><th class="num">Zeit</th><th class="num">Anteil</th><th class="num">Anzahl</th></tr></thead><tbody>
${top5.map(t => `<tr><td>${esc(t.name)}</td><td>${esc(LANES.find(l => l[0] === t.lane)?.[1] ?? t.lane)}</td><td class="num">${fmt(t.ms)}</td><td class="num">${Math.round(100 * t.ms / total)} %</td><td class="num">${t.n}</td></tr>`).join('')}
</tbody></table><p class="muted small">Zeit: wie lange es lief (überlappende Aufrufe einmal gezählt). Anteil an der Zeit bis „alles da“; Spuren laufen parallel, die Anteile summieren sich nicht zu 100 %.</p></div>
<div><h3>Netz nach Route</h3><table><thead><tr><th>Route</th><th class="num">Anfragen</th><th class="num">herein</th><th class="num">belegt</th></tr></thead><tbody>
${a.routes.slice(0, 9).map(r => `<tr><td>${esc(r.name)}</td><td class="num">${r.n}</td><td class="num">${kb(r.in)}</td><td class="num">${fmt(r.busy)}</td></tr>`).join('')}
</tbody></table></div>
</div>
</section>`
})

const counts = runs[0]?.counts ?? {}
const html = `<!doctype html><html lang="de"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Erster Login, voller Account</title>
<style>
:root{--bg:#fcfcfb;--fg:#0b0b0b;--fg2:#52514e;--muted:#8a8984;--grid:#e6e5e0;--card:#f4f3ef;
--net:#2a78d6;--crypto:#eb6834;--model:#1baf7a;--cache:#eda100;--step:#4a3aa7;--ms:#e34948;
--p0:#d9d8d2;--p1:#f2c9c9;--p2:#f6e3b0;--p3:#bfe6cf;
--k-artifacts:#e87ba4;--k-sessions_archived:#8a8984;--k-off:#eda100;--k-desk:#2a78d6;--k-online:#1baf7a;--k-items:#4a3aa7;--k-sessions_named:#008300}
@media (prefers-color-scheme: dark){:root:not([data-theme="light"]){--bg:#1a1a19;--fg:#fff;--fg2:#c3c2b7;--muted:#8f8e86;--grid:#33332f;--card:#252523;
--net:#3987e5;--crypto:#d95926;--model:#199e70;--cache:#c98500;--step:#9085e9;--ms:#e66767;--p0:#3a3a36;--p1:#5a2f2f;--p2:#5a4a20;--p3:#244a35;
--k-artifacts:#d55181;--k-sessions_archived:#8f8e86;--k-off:#c98500;--k-desk:#3987e5;--k-online:#199e70;--k-items:#9085e9;--k-sessions_named:#008300}}
:root[data-theme="dark"]{--bg:#1a1a19;--fg:#fff;--fg2:#c3c2b7;--muted:#8f8e86;--grid:#33332f;--card:#252523;--net:#3987e5;--crypto:#d95926;--model:#199e70;--cache:#c98500;--step:#9085e9;--ms:#e66767;--p0:#3a3a36;--p1:#5a2f2f;--p2:#5a4a20;--p3:#244a35;--k-artifacts:#d55181;--k-sessions_archived:#8f8e86;--k-off:#c98500;--k-desk:#3987e5;--k-online:#199e70;--k-items:#9085e9;--k-sessions_named:#008300}
body{margin:0;background:var(--bg);color:var(--fg);font:14px/1.45 system-ui,-apple-system,Segoe UI,sans-serif}
main{max-width:1180px;margin:0 auto;padding:20px 16px 40px}
h1{font-size:22px;margin:0 0 4px}h2{font-size:17px;margin:28px 0 8px}h3{font-size:14px;margin:12px 0 6px}
.muted{color:var(--fg2)}.small{font-size:11px}.err{color:var(--ms);font-size:13px}
.tiles{display:grid;grid-template-columns:repeat(auto-fit,minmax(140px,1fr));gap:8px;margin:8px 0}
.tile{background:var(--card);border-radius:8px;padding:8px 10px}.tile b{display:block;font-size:20px}.tile span{color:var(--fg2);font-size:12px}
.tile.strong b{color:var(--ms)}
.wf{display:block;margin:6px 0;overflow:visible}
.wf .lab{fill:var(--fg2);font-size:12px}.wf .sec{fill:var(--fg);font-size:12px;font-weight:600}.wf .small{fill:var(--muted);font-size:10px}
.wf .in{fill:var(--fg);font-size:11px;pointer-events:none}.wf .grid{stroke:var(--grid)}.wf .base{stroke:var(--grid)}
.wf .ms{stroke:var(--ms);stroke-dasharray:3 3}.wf .ms-l{fill:var(--ms);font-size:11px}
.l-net{fill:var(--net)}.l-crypto{fill:var(--crypto)}.l-model{fill:var(--model)}.l-cache{fill:var(--cache)}.l-step{fill:var(--step);opacity:.85}
.p0{fill:var(--p0)}.p1{fill:var(--p1)}.p2{fill:var(--p2)}.p3{fill:var(--p3)}
.kind{opacity:.75}${KINDS.map(([k]) => `.k-${k}{fill:var(--k-${k})}`).join('')}
.dot{stroke:var(--bg);stroke-width:2}.plot{fill:var(--card)}
.ik-artifacts{fill:#e87ba4;background:#e87ba4}.ik-off{fill:#eda100;background:#eda100}.ik-desk{fill:#2a78d6;background:#2a78d6}.ik-chat{fill:#4a3aa7;background:#4a3aa7}
.ik-registers{fill:#b9b8b0;background:#b9b8b0}.ik-names{fill:#008300;background:#008300}.ik-archived{fill:#52514e;background:#52514e}.ik-commits{fill:#eb6834;background:#eb6834}.ik-presence{fill:#1baf7a;background:#1baf7a}.ik-other{fill:#e34948;background:#e34948}
.rk-changes{fill:#2a78d6;background:#2a78d6}.rk-joins{fill:#eb6834;background:#eb6834}.rk-desk{fill:#1baf7a;background:#1baf7a}.rk-welcomes{fill:#eda100;background:#eda100}.rk-files{fill:#e87ba4;background:#e87ba4}.rk-other{fill:#b9b8b0;background:#b9b8b0}
i.sw{display:inline-block;width:10px;height:10px;border-radius:2px;margin-right:6px;vertical-align:-1px}
[data-tip]:hover{opacity:1;stroke:var(--fg);stroke-width:1}
.cols{display:grid;grid-template-columns:1fr 1fr;gap:20px}@media (max-width:760px){.cols{grid-template-columns:1fr}}
table{border-collapse:collapse;width:100%;font-size:12.5px}th,td{text-align:left;padding:3px 6px;border-bottom:1px solid var(--grid)}.num{text-align:right;font-variant-numeric:tabular-nums}
#tip{position:fixed;pointer-events:none;background:var(--fg);color:var(--bg);padding:5px 8px;border-radius:6px;font-size:12px;max-width:420px;display:none;z-index:9}
.legend{display:flex;flex-wrap:wrap;gap:12px;font-size:12px;color:var(--fg2)}.legend i{display:inline-block;width:10px;height:10px;border-radius:2px;margin-right:4px;vertical-align:-1px}
</style></head><body><main>
<h1>Erster Login in einen vollen Account</h1>
<p class="muted">Neues Gerät meldet sich mit dem Wiederherstellungscode an, lokaler Hub (Release-Build), ausgedachter Test-Account: ${counts.cards ?? '?'} Karten, ${counts.sessions ?? '?'} Sessions (${counts.archived ?? '?'} archiviert, ${counts.live_sessions ?? '?'} live), ${counts.agents ?? '?'} Agents, ${counts.humans ?? '?'} eigene Geräte, ${counts.artifacts ?? '?'} Artifacts, ${counts.pictures ?? '?'} Bilder, ${counts.chat ?? '?'} Chat-Zeilen.</p>
<div class="legend"><span><i style="background:var(--net)"></i>Netz</span><span><i style="background:var(--crypto)"></i>Krypto (MLS, Entschlüsseln)</span><span><i style="background:var(--model)"></i>Modell</span><span><i style="background:var(--cache)"></i>Cache/Speicher</span><span><i style="background:var(--step)"></i>Schritte des Clients</span><span><i style="background:var(--ms)"></i>Meilensteine</span></div>
${sections.join('\n')}
${process.env.TRACE_NOTES ? fs.readFileSync(process.env.TRACE_NOTES, 'utf8') : ''}
<div id="tip"></div>
</main>
<script>
const tip = document.getElementById('tip')
const all = [...document.querySelectorAll('.tips')].map(s => JSON.parse(s.textContent))
document.addEventListener('mousemove', e => {
  const t = e.target.closest('[data-tip]'), svg = e.target.closest('svg.wf')
  if (!t || !svg) { tip.style.display = 'none'; return }
  tip.textContent = all[+svg.dataset.run][+t.dataset.tip]
  tip.style.display = 'block'
  tip.style.left = Math.min(e.clientX + 12, innerWidth - 430) + 'px'; tip.style.top = (e.clientY + 14) + 'px'
})
</script></body></html>`
fs.writeFileSync(outFile, html)
console.log(JSON.stringify(runs.map(a => ({
  platform: a.platform, wall: a.wall, opened: a.opened, live: a.live, settled: a.settled,
  first: Object.fromEntries(a.kinds.map(k => [k.key, [k.first, k.full, k.final]])),
  requests: a.requests, bytesIn: a.bytesIn, bytesOut: a.bytesOut, decrypts: a.decrypts, commits: a.commits,
  top: a.top.slice(0, 12).map(t => [t.lane, t.name, Math.round(t.ms), t.n]),
  routes: a.routes.slice(0, 12).map(r => [r.name, r.n, r.in, Math.round(r.busy)]),
  calls: a.calls.slice(0, 12).map(c => [c.name, c.n, Math.round(c.ms)]),
  marks: a.marks.map(([m, t]) => [m, Math.round(t)]),
})), null, 1))
