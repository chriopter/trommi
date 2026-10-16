// Settings · Teams (/settings/teams): a card per team with what it holds, counted, and a bin that deletes the team
// with everything on it.
//
// The teams are the sidebar's (vm.sidebar.teams), what each holds the core's (vm.settings.team_counts: its board
// counted once read on this device), each board small the core's too (vm.thumbs, painted by whiteboard.mjs's
// controller "thumbs"), a delete under way with how far vm.settings.deleting. The confirm names exactly
// what goes and is held to delete: Action::RemoveTeam (POST /settings/teams/<id>/delete); the core runs every step,
// also one broken off before, and the team leaves the list when it is gone. ?del=<team id> opens a team's confirm on load.
import { Controller, controller, html, raw, settingsPage, sk } from './ui.mjs'
import { viewModule } from './app.mjs'

const pct = p => (p ? Math.round((100 * p.done) / Math.max(1, p.total)) : 0)
const num = (k, word) => html`<div class="dk-n"><b>${k ?? '…'}</b><small>${word}</small></div>`
function teamCard(d, c, p, last) {
  const name = d.name.en
  return html`<section class="dk-card" id="dk-${d.id}" aria-label="${name}" data-team-card="${d.id}"${p ? raw(' data-deleting=""') : ''}>
<canvas class="dk-thumb" width="520" height="200" data-thumb="${d.id}" aria-hidden="true"></canvas>
<header class="dk-card-head"><span class="set-ico">${sk('desk')}</span><b>${name}</b><button type="button" class="dk-card-del" data-action="teams#open" data-teams-id-param="${d.id}" title="${last ? 'Your only team stays' : `Delete ${name}…`}" aria-label="Delete ${name}…"${last ? raw(' disabled') : ''}>${sk('bin')}</button></header>
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
  return html`<dialog class="dk-dialog" id="team-del-${d.id}" aria-labelledby="team-del-title-${d.id}" data-team="${d.id}">
<div class="dk-dialog-in">
<h3 id="team-del-title-${d.id}">Delete ${name}?</h3>
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
<p class="dk-empty"${keys.some(k => c[k]) ? raw(' hidden') : ''}>Nothing stands on this team: only its name goes.</p>
<p class="dk-dialog-small">The server drops the encrypted data within 30 days. Your agents' computers keep nothing of it they can still open. This cannot be undone.</p>
<div class="dk-dialog-foot"><button type="button" class="dk-cancel" data-action="teams#cancel">Cancel</button><button type="button" class="dk-delete dk-hold" data-action="pointerdown->teams#hold pointerup->teams#release pointerleave->teams#release keydown->teams#holdKey keyup->teams#release"><span class="dk-hold-fill" aria-hidden="true"></span><span class="dk-hold-word">Hold to Delete</span></button></div>
</div>
<div class="dk-going" role="status" aria-live="polite"${p ? '' : raw(' hidden')}><span class="dk-bar"><i style="width:${pct(p)}%"></i></span><p class="dk-going-line">${p ? `Deleting… ${p.done} of ${p.total}` : 'Deleting…'}</p></div>
</div></dialog>`
}

export function register(t) {
  // (the boards small are painted by whiteboard.mjs's controller "thumbs": loaded with this page)
  viewModule('whiteboard').catch(err => console.warn('whiteboard', err))
  const main = url => {
    const vm = t.vm(), teams = vm.sidebar.teams
    const counts = new Map((vm.settings.team_counts ?? []).map(c => [c.team_id, c])), going = new Map((vm.settings.deleting ?? []).map(p => [p.team_id, p]))
    return settingsPage('Teams', html`<div class="dk-page" data-controller="teams" data-teams-open-value="${url?.searchParams.get('del') ?? ''}">
<div class="dk-cards" id="dk-cards" data-controller="thumbs">${teams.map(d => teamCard(d, counts.get(d.id) ?? {}, going.get(d.id), teams.length < 2))}</div>
<p class="set-note">Each team with its Scribble Board small. The bin deletes a team with everything on it; you see what goes before.</p>
<div id="dk-dialogs">${teams.map(d => confirmDialog(d, counts.get(d.id) ?? {}, going.get(d.id)))}</div></div>`, { lead: vm.settings.teams?.en ?? '' })
  }
  t.get(/^\/settings\/teams$/, ({ req, res, url }) => t.page(req, res, { title: 'Teams · Settings · Trommi', view: 'settings-teams', css: 'teams', main: main(url) }))
  t.post(/^\/settings\/teams\/([\w-]+)\/delete$/, ({ res, match }) => { t.act({ do: 'remove_team', team_id: match[1] }); t.done(res) })
}

// ---- controller "teams": the confirm, held to delete ----
controller('teams', class extends Controller {
  static values = { open: String }
  connect() { if (this.openValue) requestAnimationFrame(() => this.show(this.openValue)) }
  open({ params: { id } }) { this.show(id) }
  show(id) { this.element.querySelector(`#team-del-${CSS.escape(id)}`)?.showModal() }
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
  /** Held long enough: the core deletes the team; the dialog shows how far (vm.settings.deleting) and goes with it. */
  go(d) {
    d.querySelector('.dk-ask').hidden = true
    d.querySelector('.dk-going').hidden = false
    d.addEventListener('cancel', e => e.preventDefault())
    fetch(`/settings/teams/${encodeURIComponent(d.dataset.team)}/delete`, { method: 'POST' }).catch(err => console.warn('team delete', err))
  }
})
