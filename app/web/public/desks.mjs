// Settings · Desks (/settings/desks): every desk with what it holds, counted, and Delete with a confirm that lists
// exactly what goes. Three looks behind a query flag while he chooses (?v=rows | cards | table; rows is the default):
//   rows   a list as the other Settings pages; a desk opens its own page (/settings/desks/<id>) with the counts and
//          Delete Desk…; the confirm asks for the desk's name to be typed
//   cards  a card per desk with its Scribble Board small and the counts as numbers; the confirm is held to delete
//   table  one table, a desk per line, a number per column; the confirm asks for the desk's name to be typed
// ?del=<desk id> opens a desk's confirm on load (for the screenshots).
// What a desk holds: the sessions that stand on it (main sessions, helpers, archived), their cards (open, done),
// their files (Artifacts), its Scribble Board (counted on the device: controller "desk-boards", as the board cards of
// "All desks" are), its goals, and the agents that carry nothing on another desk.
// Delete is not wired yet: the confirm says what would go; the button answers that it is a proposal.
import { Controller, controller, html, raw, settingsPage, sk, toast, SET_CHEVRON } from './ui.mjs'
import { scribbleWire } from './app.mjs'
import { openCanvas, paintThumb } from './whiteboard.mjs'

const { deskBoard } = await scribbleWire()
const LOOKS = ['rows', 'cards', 'table']
const n = (k, one, many = `${one}s`) => `${k} ${k === 1 ? one : many}`

/** What desk `d` holds. m: the board model; members: the room's members (whose device is a person's). */
export function deskCounts(m, d, members = new Map()) {
  const deskOf = m.deskOf ?? (a => a.desk)
  const on = (m.everyone ?? []).filter(a => !a.removed && !a.own && deskOf(a) === d.id)
  const ids = new Set(on.map(a => a.id))
  const helper = a => Boolean(a.parent) || a.main === false
  const main = on.filter(a => !a.archived && !helper(a)), helpers = on.filter(a => !a.archived && helper(a)), archived = on.filter(a => a.archived)
  const cards = (m.state?.cards ?? []).filter(c => ids.has(c.agent))
  const open = cards.filter(c => c.status === 'open').length
  const files = (m.state?.assets ?? []).filter(x => ids.has(x.agent)).length
  // (an agent device that also carries a live session on another desk stays in the room)
  const elsewhere = dev => (m.everyone ?? []).some(a => !ids.has(a.id) && !a.removed && !a.archived && a.agent_device_id === dev)
  const devices = [...new Set(on.map(a => a.agent_device_id).filter(Boolean))].filter(x => members.get(x)?.device_role !== 'human' && !elsewhere(x))
  const goals = String(d.goals ?? '').split('\n').filter(l => l.trim()).length
  return { main: main.length, helpers: helpers.length, archived: archived.length, open, done: cards.length - open, files, agents: devices.length, goals, board: deskBoard(d.id) }
}

// The board's numbers, filled by controller "desk-boards" once the board is read on this device.
const boardSpan = (c, kind) => html`<span data-board-of="${c.board}" data-kind="${kind}">…</span>`
const boardLine = c => html`${boardSpan(c, 'all')} on the Scribble Board`

/** The confirm: everything that goes, in numbers; `how` 'type' (type the name) or 'hold' (hold the button). */
function confirmDialog(d, c, how, last) {
  const li = (k, word, rest = '') => (k ? html`<li><b>${k}</b><span>${word}${rest ? html` <small>${rest}</small>` : ''}</span></li>` : '')
  const empty = !c.main && !c.helpers && !c.archived && !c.open && !c.done && !c.files && !c.goals
  return html`<dialog class="dk-dialog" id="desk-del-${d.id}" aria-labelledby="desk-del-title-${d.id}" data-desk-del-target="dialog" data-name="${d.name}">
<form method="dialog" class="dk-dialog-in" data-action="submit->desk-del#confirm">
<h3 id="desk-del-title-${d.id}">Delete ${d.name}?</h3>
${last ? html`<p class="dk-dialog-lead">This is your only desk. Every account keeps one; make another desk first.</p>` : html`<p class="dk-dialog-lead">${empty ? 'Nothing stands on this desk: only its name goes.' : 'This goes for good, on every device:'}</p>
${empty ? '' : html`<ul class="dk-list">
${li(c.main + c.helpers + c.archived, (c.main + c.helpers + c.archived) === 1 ? 'session' : 'sessions', [[c.main && `${c.main} main`, c.helpers && n(c.helpers, 'helper'), c.archived && `${c.archived} archived`].filter(Boolean).join(', '), c.agents ? `${c.agents === 1 ? 'its agent is' : 'their agents are'} removed from your account` : ''].filter(Boolean).join('; '))}
${li(c.open + c.done, (c.open + c.done) === 1 ? 'card' : 'cards', [c.open && `${c.open} open`, c.done && `${c.done} done`].filter(Boolean).join(', '))}
<li class="dk-board-li"><b>${boardSpan(c, 'all')}</b><span>on the Scribble Board <small>${boardSpan(c, 'notes')}, ${boardSpan(c, 'pictures')}</small></span></li>
${li(c.files, c.files === 1 ? 'file' : 'files', 'Artifacts and share links')}
${li(c.goals, c.goals === 1 ? 'goal' : 'goals')}
</ul>`}
<p class="dk-dialog-small">Your agents' computers keep nothing of it they can still open. The hub deletes its sealed copies within 30 days. This cannot be undone.</p>
${how === 'type' ? html`<label class="dk-type"><span>Type <b>${d.name}</b> to confirm</span><input type="text" autocomplete="off" spellcheck="false" data-desk-del-target="field" data-action="input->desk-del#typed" aria-label="The desk's name"></label>` : ''}`}
<div class="dk-dialog-foot"><button type="button" class="dk-cancel" data-action="desk-del#cancel">Cancel</button>${last ? '' : how === 'hold'
    ? html`<button type="button" class="dk-delete dk-hold" data-action="pointerdown->desk-del#hold pointerup->desk-del#release pointerleave->desk-del#release keydown->desk-del#holdKey keyup->desk-del#release"><span class="dk-hold-fill" aria-hidden="true"></span><span class="dk-hold-word">Hold to Delete</span></button>`
    : html`<button type="submit" class="dk-delete" data-desk-del-target="go" disabled>Delete Desk</button>`}</div>
</form></dialog>`
}

// ---- look "rows": the list, and a desk's own page ----
const summary = c => html`${n(c.main, 'session')}${c.helpers ? ` +${c.helpers}` : ''} · ${n(c.open, 'open card')} · ${boardLine(c)}`
function rowsMain(desks, counts, q) {
  return html`<div class="set-group" id="desks-list">${desks.map(d => html`<a class="set-row set-row-two dk-row" href="/settings/desks/${d.id}?v=rows" data-nav><span class="set-ico">${sk('desk')}</span><span class="set-two"><b>${d.name}</b><small>${summary(counts.get(d.id))}</small></span><span class="set-detail"></span>${SET_CHEVRON}</a>`)}</div>
<p class="set-note">A desk holds its sessions, their cards and files, its Scribble Board and its goals. Open one to see it counted, or to delete it.</p>`
}
function deskPage(d, c, last) {
  const row = (icon, word, k, detail = '') => html`<div class="set-row dk-count"><span class="set-ico">${sk(icon)}</span><b>${word}</b><span class="set-detail">${detail}</span><span class="dk-num">${k}</span></div>`
  return html`<h3 class="set-label">On this desk</h3><div class="set-group">
${row('terminal', 'Sessions', c.main, [c.helpers && n(c.helpers, 'helper'), c.archived && `${c.archived} archived`].filter(Boolean).join(' · '))}
${row('tray', 'Cards', c.open + c.done, `${c.open} open · ${c.done} done`)}
<div class="set-row dk-count"><span class="set-ico">${sk('pen')}</span><b>Scribble Board</b><span class="set-detail">${boardSpan(c, 'notes')} · ${boardSpan(c, 'pictures')}</span><span class="dk-num">${boardSpan(c, 'all')}</span></div>
${row('page', 'Files', c.files, 'Artifacts')}
${row('tick', 'Goals', c.goals)}
</div>
<div class="set-group"><button type="button" class="set-row dk-danger-row" data-action="desk-del#open" data-desk-del-id-param="${d.id}"${last ? raw(' disabled') : ''}><span class="set-ico">${sk('bin')}</span><b>Delete Desk…</b><span class="set-detail">${last ? 'your only desk stays' : 'shows what goes first'}</span><span></span></button></div>`
}

// ---- look "cards": a card per desk, its board small ----
function cardsMain(desks, counts) {
  const num = (k, word) => html`<div class="dk-n"><b>${k}</b><small>${word}</small></div>`
  return html`<div class="dk-cards">${desks.map(d => { const c = counts.get(d.id); return html`<section class="dk-card" aria-label="${d.name}">
<canvas class="dk-thumb" width="520" height="200" data-thumb="${c.board}" aria-hidden="true"></canvas>
<header class="dk-card-head"><span class="set-ico">${sk('desk')}</span><b>${d.name}</b><button type="button" class="dk-card-del" data-action="desk-del#open" data-desk-del-id-param="${d.id}" title="Delete ${d.name}…" aria-label="Delete ${d.name}…"${desks.length < 2 ? raw(' disabled') : ''}>${sk('bin')}</button></header>
<div class="dk-nums">${num(c.main, c.main === 1 ? 'session' : 'sessions')}${num(c.helpers, c.helpers === 1 ? 'helper' : 'helpers')}${num(c.open, 'open cards')}${num(c.done, 'done')}${num(boardSpan(c, 'all'), 'on the board')}${num(c.files, c.files === 1 ? 'file' : 'files')}</div>
</section>` })}</div>
<p class="set-note">Each desk with its Scribble Board small. The bin deletes a desk; you see what goes before.</p>`
}

// ---- look "table": one table ----
function tableMain(desks, counts) {
  const sum = k => desks.reduce((s, d) => s + counts.get(d.id)[k], 0)
  return html`<div class="set-group dk-table-wrap"><table class="dk-table">
<thead><tr><th scope="col">Desk</th><th scope="col">Sessions</th><th scope="col">Helpers</th><th scope="col">Open</th><th scope="col">Done</th><th scope="col">Board</th><th scope="col">Files</th><th scope="col"><span class="sr-only">Delete</span></th></tr></thead>
<tbody>${desks.map(d => { const c = counts.get(d.id); return html`<tr><th scope="row"><span class="dk-tname">${sk('desk')}<b>${d.name}</b></span></th><td data-l="sessions">${c.main}</td><td data-l="helpers">${c.helpers}</td><td data-l="open">${c.open}</td><td data-l="done">${c.done}</td><td data-l="board">${boardSpan(c, 'all')}</td><td data-l="files">${c.files}</td><td><button type="button" class="dk-tdel" data-action="desk-del#open" data-desk-del-id-param="${d.id}" title="Delete ${d.name}…" aria-label="Delete ${d.name}…"${desks.length < 2 ? raw(' disabled') : ''}>${sk('bin')}</button></td></tr>` })}</tbody>
<tfoot><tr><th scope="row">All desks</th><td data-l="sessions">${sum('main')}</td><td data-l="helpers">${sum('helpers')}</td><td data-l="open">${sum('open')}</td><td data-l="done">${sum('done')}</td><td></td><td data-l="files">${sum('files')}</td><td></td></tr></tfoot>
</table></div>
<p class="set-note">Open and Done count cards; Board counts what is on the desk's Scribble Board.</p>`
}

export function register(t) {
  const { hub } = t
  const members = () => hub.client?.model?.members ?? new Map()
  const lookOf = url => (LOOKS.includes(url.searchParams.get('v')) ? url.searchParams.get('v') : 'rows')
  const wrap = (look, inner, dialogs, del) => html`<div class="dk-page" data-look="${look}" data-controller="desk-boards desk-del" data-desk-del-open-value="${del}">${inner}${dialogs}</div>`
  const tabs = (look, path) => html`<nav class="dk-looks" aria-label="Proposals">${LOOKS.map((l, i) => html`<a href="${path}?v=${l}" data-nav${l === look ? raw(' aria-current="page"') : ''}>${String.fromCharCode(65 + i)} · ${l}</a>`)}</nav>`
  t.get(/^\/settings\/desks$/, ({ req, res, url }) => {
    const m = t.model(), desks = m.state?.desks ?? [], look = lookOf(url), del = url.searchParams.get('del') ?? ''
    const counts = new Map(desks.map(d => [d.id, deskCounts(m, d, members())]))
    const inner = look === 'cards' ? cardsMain(desks, counts) : look === 'table' ? tableMain(desks, counts) : rowsMain(desks, counts)
    const dialogs = look === 'rows' ? '' : desks.map(d => confirmDialog(d, counts.get(d.id), look === 'cards' ? 'hold' : 'type', desks.length < 2))
    const total = k => desks.reduce((s, d) => s + counts.get(d.id)[k], 0)
    const lead = `${n(desks.length, 'desk')} · ${n(total('main'), 'session')}, ${n(total('helpers'), 'helper')}`
    t.page(req, res, { title: 'Desks · Settings · Trommi', view: 'settings-desks', css: 'desks', main: settingsPage('Desks', html`${tabs(look, '/settings/desks')}${wrap(look, inner, dialogs, del)}`, { lead, cls: `room dk-look-${look}` }) })
  })
  t.get(/^\/settings\/desks\/([^/]+)$/, ({ req, res, url, match }) => {
    const m = t.model(), desks = m.state?.desks ?? [], d = desks.find(x => x.id === decodeURIComponent(match[1]))
    if (!d) return t.redirect(res, '/settings/desks')
    const c = deskCounts(m, d, members()), del = url.searchParams.get('del') ? d.id : ''
    t.page(req, res, { title: `${d.name} · Desks · Trommi`, view: 'settings-desks', css: 'desks', main: settingsPage(d.name, wrap('rows', deskPage(d, c, desks.length < 2), confirmDialog(d, c, 'type', desks.length < 2), del), { back: '/settings/desks?v=rows', backWord: 'Desks'}) })
  })
}

// ---- controller "desk-boards": every desk's board read on this device, its numbers filled in, its card painted ----
controller('desk-boards', class extends Controller {
  connect() {
    const client = window.trommi?.client
    if (!client) return
    this.open = new Map()
    const ids = new Set([...this.element.querySelectorAll('[data-board-of]')].map(e => e.dataset.boardOf).concat([...this.element.querySelectorAll('[data-thumb]')].map(e => e.dataset.thumb)))
    for (const id of ids) {
      const canvas = openCanvas({ client, timeline_id: id, onRemote: () => this.fill(id) })
      this.open.set(id, canvas)
      this.fill(id)
    }
  }
  async fill(id) {
    let list
    try { list = (await this.open.get(id))?.records() } catch (e) { console.warn('desk board', e) }
    if (!list) return
    const k = { all: list.length, pictures: list.filter(r => r.type === 'image').length, notes: list.filter(r => r.type === 'sticky' || r.type === 'text').length }
    const say = { all: String(k.all), pictures: n(k.pictures, 'picture'), notes: n(k.notes, 'note') }
    for (const e of this.element.querySelectorAll(`[data-board-of="${id}"]`)) e.textContent = say[e.dataset.kind] ?? ''
    for (const cv of this.element.querySelectorAll(`canvas[data-thumb="${id}"]`)) paintThumb(cv, list)
  }
  disconnect() { for (const c of this.open?.values() ?? []) c.then(x => x.close(), () => {}) }
})

// ---- controller "desk-del": the confirm (type the name, or hold) ----
controller('desk-del', class extends Controller {
  static targets = ['dialog', 'field', 'go']
  static values = { open: String }
  connect() { if (this.openValue) requestAnimationFrame(() => this.show(this.openValue)) }
  open({ params: { id } }) { this.show(id) }
  show(id) {
    const d = this.element.querySelector(`#desk-del-${CSS.escape(id)}`)
    if (!d) return
    d.showModal()
    d.querySelector('input')?.focus()
  }
  typed(e) {
    const d = e.target.closest('dialog'), go = d.querySelector('.dk-delete')
    if (go) go.disabled = e.target.value.trim() !== d.dataset.name
  }
  cancel(e) { e.target.closest('dialog')?.close() }
  confirm(e) { e.preventDefault(); this.done(e.target.closest('dialog')) }
  hold(e) {
    const b = e.currentTarget
    b.classList.add('is-holding')
    clearTimeout(this.timer)
    this.timer = setTimeout(() => { b.classList.remove('is-holding'); this.done(b.closest('dialog')) }, 1600)
  }
  holdKey(e) { if ((e.key === ' ' || e.key === 'Enter') && !e.repeat) { e.preventDefault(); this.hold(e) } }
  release(e) { clearTimeout(this.timer); e.currentTarget.classList.remove('is-holding') }
  done(d) {
    d?.close()
    toast({ head: 'Not deleted', line: 'a proposal: Delete is not wired yet' })
  }
})
