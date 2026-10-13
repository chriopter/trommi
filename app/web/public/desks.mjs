// Settings · Desks (/settings/desks): a card per desk with its Scribble Board small and what it holds, counted, and
// a bin that deletes the desk with everything on it.
//
// The numbers are the client core's (core/client/src/desk_delete.rs `desk_contents`, through `client.deskContents`), once
// the desk's board is read on this device (controller "desks"); the page is drawn first with the same numbers counted
// here from the board model (`deskCounts`), which the core's then replace. The confirm names exactly what goes and is
// held to delete.
//
// Deleting runs the core's steps (`client.deskDeleteStep`, one at a time) until the desk is gone: mark the desk,
// shred its open cards, archive its sessions, remove its agents, erase its board, delete its files, write an empty
// snapshot, take the desk away. The board's steps are done here (openCanvas). A delete broken off (a tab closed, a
// device gone) is finished on the next start of any of his devices (`resumeDeskDeletes`, from app.mjs); while it runs
// its card shows how far it is. ?del=<desk id> opens a desk's confirm on load.
import { Controller, controller, html, raw, settingsPage, sk, toast } from './ui.mjs'
import { scribbleWire, viewModule } from './app.mjs'

const { deskBoard } = await scribbleWire()
// (the Scribble Board's canvas and thumbnail: whiteboard.mjs)
const { openCanvas, paintThumb } = await viewModule('whiteboard')
const n = (k, one, many = `${one}s`) => `${k} ${k === 1 ? one : many}`

/** What desk `d` holds, counted from the board model, in the core's names (`Counts`): the numbers the page shows
 *  until the core's come. m: the board model; members: the room's members. */
export function deskCounts(m, d, members = new Map()) {
  const deskOf = m.deskOf ?? (a => a.desk)
  const on = (m.everyone ?? []).filter(a => !a.removed && !a.own && deskOf(a) === d.id)
  const ids = new Set(on.map(a => a.id))
  const helper = a => Boolean(a.parent) || a.main === false
  const cards = (m.state?.cards ?? []).filter(c => ids.has(c.agent))
  const open = cards.filter(c => c.status === 'open').length
  const artifacts = (m.state?.assets ?? []).filter(x => ids.has(x.agent)).length
  const elsewhere = dev => (m.everyone ?? []).some(a => !ids.has(a.id) && !a.removed && !a.archived && a.agent_device_id === dev)
  const agents = new Set(on.map(a => a.agent_device_id).filter(x => x && members.get(x)?.device_role === 'agent' && !elsewhere(x))).size
  return {
    sessions: on.filter(a => !a.archived && !helper(a)).length, helpers: on.filter(a => !a.archived && helper(a)).length, archived: on.filter(a => a.archived).length,
    open_cards: open, done_cards: cards.length - open, agents, board: null, pictures: null, notes: null, files: artifacts, artifacts,
    goals: String(d.goals ?? '').split('\n').filter(l => l.trim()).length,
  }
}

// ---- deleting: the core's steps, the board's done here ----
const running = new Map()   // desk id -> the run's promise (one per desk in this page)
const progress = new Map()  // desk id -> { done, total }
const boardKnown = canvas => {
  const list = canvas.records()
  return { shapes: list.map(r => String(r.id)), files: [...new Set(list.filter(r => r.type === 'image' && /^[0-9a-f]{32}$/.test(r.blob ?? '')).map(r => r.blob))], notes: list.filter(r => r.type === 'sticky' || r.type === 'text').length, snapshot_empty: false }
}
const tell = (id, p) => { progress.set(id, p); document.dispatchEvent(new CustomEvent('trommi:desk-delete', { detail: { id, ...p } })) }

/** Deletes desk `id` with everything on it, or finishes a delete begun before (on any device). One run per desk in
 *  this browser (a lock across its tabs). Resolves when the desk is gone. */
export function deleteDesk(client, id) {
  if (running.has(id)) return running.get(id)
  const run = async () => {
    let canvas = null, began = false, last = '', same = 0
    const known = { files_gone: [], board: null }
    try {
      for (;;) {
        const p = await client.deskDeleteStep(id, known)
        if (p.files_gone?.length) known.files_gone.push(...p.files_gone)
        tell(id, { done: p.done, total: p.total, step: p.step })
        if (p.step === 'done' || (p.step === 'refused' && p.code === 'no-desk' && began)) break
        if (p.step === 'refused') throw Object.assign(new Error(p.code === 'last-desk' ? 'your only desk stays' : 'no such desk'), { code: p.code })
        began = true
        if (p.step === 'load_board' || p.step === 'erase_board' || p.step === 'write_snapshot') {
          canvas ??= await openCanvas({ client, timeline_id: deskBoard(id) })
          if (p.step === 'erase_board') await canvas.eraseAll()
          const empty = p.step === 'write_snapshot' ? await canvas.snapshotEmpty() : known.board?.snapshot_empty ?? false
          known.board = { ...boardKnown(canvas), snapshot_empty: empty && !canvas.records().length }
        }
        // (a step the model does not show done yet: the hub's answer is still on its way; a step that never shows
        //  done is given up, and the next start goes on with it)
        const key = `${p.step}:${p.done}`
        same = key === last ? same + 1 : 0
        last = key
        if (same >= 3) await new Promise(r => setTimeout(r, 600 * same))
        if (same >= 8) throw Object.assign(new Error(`the step "${p.step}" does not finish`), { code: 'stuck' })
      }
    } finally { canvas?.close(); running.delete(id); progress.delete(id) }
  }
  const locks = globalThis.navigator?.locks
  const p = locks ? locks.request(`trommi-desk-delete/${id}`, run) : run()
  running.set(id, p)
  return p
}

/** On start: every desk a delete was begun on (by any device) is finished. */
export async function resumeDeskDeletes(client) {
  if (typeof client?.desksDeleting !== 'function') return
  const ids = await client.desksDeleting()
  for (const id of ids) {
    const name = client.model?.human?.desks?.get(id)?.name || 'A desk'
    toast({ head: 'Finishing a delete', line: name })
    deleteDesk(client, id).then(() => toast({ head: 'Deleted', line: name }), err => toast({ head: 'Not deleted yet', line: `${name}: ${err?.message ?? err}. It goes on at the next start.`, role: 'alert' }))
  }
}

// ---- the page ----
const num = (id, key, k, word) => html`<div class="dk-n"><b data-desk="${id}" data-n="${key}">${k ?? '…'}</b><small>${word}</small></div>`
function deskCard(d, c, last) {
  const p = progress.get(d.id)
  return html`<section class="dk-card" aria-label="${d.name}" data-desk-card="${d.id}"${p ? raw(' data-deleting=""') : ''}>
<canvas class="dk-thumb" width="520" height="200" data-thumb="${d.id}" aria-hidden="true"></canvas>
<header class="dk-card-head"><span class="set-ico">${sk('desk')}</span><b>${d.name}</b><button type="button" class="dk-card-del" data-action="desks#open" data-desks-id-param="${d.id}" title="${last ? 'Your only desk stays' : `Delete ${d.name}…`}" aria-label="Delete ${d.name}…"${last ? raw(' disabled') : ''}>${sk('bin')}</button></header>
<div class="dk-nums">${num(d.id, 'sessions', c.sessions, 'sessions')}${num(d.id, 'helpers', c.helpers, 'helpers')}${num(d.id, 'open_cards', c.open_cards, 'open cards')}${num(d.id, 'done_cards', c.done_cards, 'done')}${num(d.id, 'board', c.board, 'on the board')}${num(d.id, 'files', c.files, 'files')}</div>
<div class="dk-progress" role="status" aria-live="polite"${p ? '' : raw(' hidden')}><span class="dk-bar"><i style="width:${p ? Math.round((100 * p.done) / Math.max(1, p.total)) : 0}%"></i></span><small>Deleting…</small></div>
</section>`
}
/** The confirm: every number the core counts, a row each (a row of nothing hides), held to delete. */
function confirmDialog(d, c) {
  const row = (key, one, many, rest = '') => html`<li data-row="${key}"${c[key] ? '' : raw(' hidden')}><b data-desk="${d.id}" data-n="${key}">${c[key] ?? '…'}</b><span><span data-word="${key}" data-one="${one}" data-many="${many}">${c[key] === 1 ? one : many}</span>${rest ? html` <small>${rest}</small>` : ''}</span></li>`
  return html`<dialog class="dk-dialog" id="desk-del-${d.id}" aria-labelledby="desk-del-title-${d.id}" data-desk="${d.id}" data-name="${d.name}">
<div class="dk-dialog-in">
<h3 id="desk-del-title-${d.id}">Delete ${d.name}?</h3>
<div class="dk-ask">
<p class="dk-dialog-lead">This goes for good, on every device:</p>
<ul class="dk-list">
${row('sessions', 'session', 'sessions', 'archived')}
${row('helpers', 'helper', 'helpers', 'archived')}
${row('archived', 'archived session', 'archived sessions')}
${row('agents', 'agent removed from your account', 'agents removed from your account', 'their keys stop working')}
${row('open_cards', 'open card', 'open cards', 'shredded')}
${row('done_cards', 'done card', 'done cards')}
${row('board', 'thing on the Scribble Board', 'things on the Scribble Board')}
${row('files', 'file', 'files', 'pictures, Artifacts and share links, deleted at once')}
${row('goals', 'goal', 'goals')}
</ul>
<p class="dk-empty" hidden>Nothing stands on this desk: only its name goes.</p>
<p class="dk-dialog-small">The server drops the encrypted data within 30 days. Your agents' computers keep nothing of it they can still open. This cannot be undone.</p>
<div class="dk-dialog-foot"><button type="button" class="dk-cancel" data-action="desks#cancel">Cancel</button><button type="button" class="dk-delete dk-hold" data-action="pointerdown->desks#hold pointerup->desks#release pointerleave->desks#release keydown->desks#holdKey keyup->desks#release"><span class="dk-hold-fill" aria-hidden="true"></span><span class="dk-hold-word">Hold to Delete</span></button></div>
</div>
<div class="dk-going" role="status" aria-live="polite" hidden><span class="dk-bar"><i></i></span><p class="dk-going-line">Deleting…</p></div>
</div></dialog>`
}

export function register(t) {
  const { hub } = t
  const members = () => hub.client?.model?.members ?? new Map()
  const main = (url = null) => {
    const m = t.model(), desks = m.state?.desks ?? [], counts = new Map(desks.map(d => [d.id, deskCounts(m, d, members())]))
    const total = k => desks.reduce((s, d) => s + counts.get(d.id)[k], 0)
    const lead = `${n(desks.length, 'desk')} · ${n(total('sessions'), 'session')}, ${n(total('helpers'), 'helper')}`
    return settingsPage('Desks', html`<div class="dk-page" data-controller="desks" data-desks-open-value="${url?.searchParams.get('del') ?? ''}">
<div class="dk-cards">${desks.map(d => deskCard(d, counts.get(d.id), desks.length < 2))}</div>
<p class="set-note">Each desk with its Scribble Board small. The bin deletes a desk with everything on it; you see what goes before.</p>
${desks.map(d => confirmDialog(d, counts.get(d.id)))}</div>`, { lead })
  }
  t.get(/^\/settings\/desks$/, ({ req, res, url }) => t.page(req, res, { title: 'Desks · Settings · Trommi', view: 'settings-desks', css: 'desks', main: main(url) }))
  // (a desk made or gone on another device: the page is drawn again; the numbers are the controller's)
  t.live('settings-desks', { take: m => (m.state?.desks ?? []).map(d => `${d.id}:${d.name}`).join('|'), diff: (was, now) => (was !== now ? String(t.stream('refresh')) : '') })
}

// ---- controller "desks": the boards read, the core's numbers, the confirm, the delete ----
controller('desks', class extends Controller {
  static values = { open: String }
  connect() {
    this.client = window.trommi?.client
    this.boards = new Map()
    this.onProgress = e => this.showProgress(e.detail)
    document.addEventListener('trommi:desk-delete', this.onProgress)
    if (this.client) for (const card of this.element.querySelectorAll('[data-desk-card]')) this.read(card.dataset.deskCard)
    if (this.openValue) requestAnimationFrame(() => this.show(this.openValue))
  }
  disconnect() {
    document.removeEventListener('trommi:desk-delete', this.onProgress)
    for (const c of this.boards.values()) c.then(x => x.close(), () => {})
  }
  /** Desk `id`'s board read (and kept current), its card painted, the numbers counted again. */
  read(id) {
    const canvas = openCanvas({ client: this.client, timeline_id: deskBoard(id), onRemote: () => this.count(id) })
    this.boards.set(id, canvas)
    this.count(id)
  }
  async count(id) {
    let canvas
    try { canvas = await this.boards.get(id) } catch (e) { console.warn('desk board', e); return }
    if (!canvas || !this.element.isConnected) return
    for (const cv of this.element.querySelectorAll(`canvas[data-thumb="${CSS.escape(id)}"]`)) paintThumb(cv, canvas.records())
    const board = boardKnown(canvas)
    let c = null
    try { c = typeof this.client.deskContents === 'function' ? await this.client.deskContents(id, board) : null } catch (e) { console.warn('desk contents', e) }
    // (the demo room has no core: its own numbers stand, with the board's)
    c ??= { board: board.shapes.length, pictures: board.files.length, notes: board.notes }
    this.fill(id, c)
  }
  fill(id, c) {
    for (const el of this.element.querySelectorAll(`[data-desk="${CSS.escape(id)}"][data-n]`)) {
      const k = c[el.dataset.n]
      if (k === undefined || k === null) continue
      el.textContent = String(k)
      const li = el.closest('li[data-row]')
      if (li) { li.hidden = !k; const w = li.querySelector('[data-word]'); if (w) w.textContent = k === 1 ? w.dataset.one : w.dataset.many }
    }
    const d = this.element.querySelector(`#desk-del-${CSS.escape(id)}`)
    if (d) d.querySelector('.dk-empty').hidden = [...d.querySelectorAll('li[data-row]')].some(li => !li.hidden)
  }
  open({ params: { id } }) { this.show(id) }
  show(id) { this.element.querySelector(`#desk-del-${CSS.escape(id)}`)?.showModal() }
  cancel(e) { e.target.closest('dialog')?.close() }
  hold(e) {
    if (e.button > 0) return
    const b = e.currentTarget
    b.classList.add('is-holding')
    clearTimeout(this.timer)
    this.timer = setTimeout(() => { b.classList.remove('is-holding'); this.go(b.closest('dialog')) }, 1600)
  }
  holdKey(e) { if ((e.key === ' ' || e.key === 'Enter') && !e.repeat) { e.preventDefault(); this.hold(e) } }
  release(e) { clearTimeout(this.timer); e.currentTarget.classList.remove('is-holding') }
  /** Held long enough: the delete runs, the dialog shows how far it is, and closes when the desk is gone. */
  async go(d) {
    const id = d.dataset.desk, name = d.dataset.name
    if (typeof this.client?.deskDeleteStep !== 'function') { d.close(); toast({ head: 'Not in the demo', line: 'deleting a desk needs your own account' }); return }
    d.querySelector('.dk-ask').hidden = true
    d.querySelector('.dk-going').hidden = false
    d.addEventListener('cancel', e => e.preventDefault())
    try {
      await deleteDesk(this.client, id)
      d.close()
      toast({ head: 'Deleted', line: name })
    } catch (err) {
      d.querySelector('.dk-going-line').textContent = `Stopped: ${err?.message ?? err}. It goes on at the next start.`
      d.querySelector('.dk-ask').hidden = false
    }
  }
  showProgress({ id, done, total, step }) {
    const pct = `${Math.round((100 * done) / Math.max(1, total))}%`
    const card = this.element.querySelector(`[data-desk-card="${CSS.escape(id)}"]`)
    if (card) { card.toggleAttribute('data-deleting', step !== 'done'); const p = card.querySelector('.dk-progress'); p.hidden = step === 'done'; p.querySelector('i').style.width = pct }
    const d = this.element.querySelector(`#desk-del-${CSS.escape(id)}`)
    if (d && !d.querySelector('.dk-going').hidden) {
      d.querySelector('.dk-going i').style.width = pct
      d.querySelector('.dk-going-line').textContent = step === 'done' ? 'Deleted.' : `Deleting… ${done} of ${total}`
    }
  }
})
