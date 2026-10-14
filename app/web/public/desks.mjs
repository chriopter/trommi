// Settings · Desks (/settings/desks): a card per desk with what it holds, counted, and a bin that deletes the desk
// with everything on it.
//
// The desks are the sidebar's (vm.sidebar.desks), what each holds the core's (vm.settings.desk_counts: its board
// counted once read on this device), a delete under way with how far vm.settings.deleting. The confirm names exactly
// what goes and is held to delete: Action::RemoveDesk (POST /settings/desks/<id>/delete); the core runs every step,
// also one broken off before, and the desk leaves the list when it is gone. ?del=<desk id> opens a desk's confirm on load.
import { Controller, controller, html, raw, settingsPage, sk } from './ui.mjs'

const pct = p => (p ? Math.round((100 * p.done) / Math.max(1, p.total)) : 0)
const num = (k, word) => html`<div class="dk-n"><b>${k ?? '…'}</b><small>${word}</small></div>`
function deskCard(d, c, p, last) {
  const name = d.name.en
  return html`<section class="dk-card" id="dk-${d.id}" aria-label="${name}" data-desk-card="${d.id}"${p ? raw(' data-deleting=""') : ''}>
<canvas class="dk-thumb" width="520" height="200" data-thumb="${d.id}" aria-hidden="true"></canvas>
<header class="dk-card-head"><span class="set-ico">${sk('desk')}</span><b>${name}</b><button type="button" class="dk-card-del" data-action="desks#open" data-desks-id-param="${d.id}" title="${last ? 'Your only desk stays' : `Delete ${name}…`}" aria-label="Delete ${name}…"${last ? raw(' disabled') : ''}>${sk('bin')}</button></header>
<div class="dk-nums">${num(c.sessions, 'sessions')}${num(c.helpers, 'helpers')}${num(c.open_cards, 'open cards')}${num(c.done_cards, 'done')}${num(c.board, 'on the board')}${num(c.files, 'files')}</div>
<div class="dk-progress" role="status" aria-live="polite"${p ? '' : raw(' hidden')}><span class="dk-bar"><i style="width:${pct(p)}%"></i></span><small>Deleting…</small></div>
</section>`
}
/** The confirm: every number the core counts, a row each (a row of nothing hides), held to delete; while the delete
 *  runs, how far it is. */
function confirmDialog(d, c, p) {
  const name = d.name.en
  const row = (key, one, many, rest = '') => html`<li data-row="${key}"${c[key] ? '' : raw(' hidden')}><b>${c[key] ?? '…'}</b><span><span>${c[key] === 1 ? one : many}</span>${rest ? html` <small>${rest}</small>` : ''}</span></li>`
  const keys = ['sessions', 'helpers', 'archived', 'agents', 'open_cards', 'done_cards', 'board', 'files', 'goals']
  return html`<dialog class="dk-dialog" id="desk-del-${d.id}" aria-labelledby="desk-del-title-${d.id}" data-desk="${d.id}">
<div class="dk-dialog-in">
<h3 id="desk-del-title-${d.id}">Delete ${name}?</h3>
<div class="dk-ask"${p ? raw(' hidden') : ''}>
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
<p class="dk-empty"${keys.some(k => c[k]) ? raw(' hidden') : ''}>Nothing stands on this desk: only its name goes.</p>
<p class="dk-dialog-small">The server drops the encrypted data within 30 days. Your agents' computers keep nothing of it they can still open. This cannot be undone.</p>
<div class="dk-dialog-foot"><button type="button" class="dk-cancel" data-action="desks#cancel">Cancel</button><button type="button" class="dk-delete dk-hold" data-action="pointerdown->desks#hold pointerup->desks#release pointerleave->desks#release keydown->desks#holdKey keyup->desks#release"><span class="dk-hold-fill" aria-hidden="true"></span><span class="dk-hold-word">Hold to Delete</span></button></div>
</div>
<div class="dk-going" role="status" aria-live="polite"${p ? '' : raw(' hidden')}><span class="dk-bar"><i style="width:${pct(p)}%"></i></span><p class="dk-going-line">${p ? `Deleting… ${p.done} of ${p.total}` : 'Deleting…'}</p></div>
</div></dialog>`
}

export function register(t) {
  const main = url => {
    const vm = t.vm(), desks = vm.sidebar.desks
    const counts = new Map((vm.settings.desk_counts ?? []).map(c => [c.desk_id, c])), going = new Map((vm.settings.deleting ?? []).map(p => [p.desk_id, p]))
    return settingsPage('Desks', html`<div class="dk-page" data-controller="desks" data-desks-open-value="${url?.searchParams.get('del') ?? ''}">
<div class="dk-cards" id="dk-cards">${desks.map(d => deskCard(d, counts.get(d.id) ?? {}, going.get(d.id), desks.length < 2))}</div>
<p class="set-note">Each desk with its Scribble Board small. The bin deletes a desk with everything on it; you see what goes before.</p>
<div id="dk-dialogs">${desks.map(d => confirmDialog(d, counts.get(d.id) ?? {}, going.get(d.id)))}</div></div>`, { lead: vm.settings.desks?.en ?? '' })
  }
  t.get(/^\/settings\/desks$/, ({ req, res, url }) => t.page(req, res, { title: 'Desks · Settings · Trommi', view: 'settings-desks', css: 'desks', main: main(url) }))
  t.post(/^\/settings\/desks\/([\w-]+)\/delete$/, ({ res, match }) => { t.act({ do: 'remove_desk', desk_id: match[1] }); t.done(res) })
}

// ---- controller "desks": the confirm, held to delete ----
controller('desks', class extends Controller {
  static values = { open: String }
  connect() { if (this.openValue) requestAnimationFrame(() => this.show(this.openValue)) }
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
  /** Held long enough: the core deletes the desk; the dialog shows how far (vm.settings.deleting) and goes with it. */
  go(d) {
    d.querySelector('.dk-ask').hidden = true
    d.querySelector('.dk-going').hidden = false
    d.addEventListener('cancel', e => e.preventDefault())
    fetch(`/settings/desks/${encodeURIComponent(d.dataset.desk)}/delete`, { method: 'POST' }).catch(err => console.warn('desk delete', err))
  }
})
