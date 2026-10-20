// ---- wakeups ----
// The page Wakeups of the team in view (vm.wakeups, WakeupsView: the core's order and words): when the team's agents
// start again on their own, what they will do then, how often, who set it, and what became of the last run.
//   GET  /wakeups                          every wakeup of the team, the next first (?team=<id>: that team first)
//   POST /wakeups/<session>/<id>/cancel    cancelled (Action::WakeupCancel)
//   POST /wakeups/<session>/<id>/move      another time (Action::WakeupMove; at=<local datetime>)
// The agents set wakeups themselves (wake_me); their connector keeps the clock on the person's computer. Nothing here
// decides: the shell draws.
import { Controller, avatar, controller, html, raw, sk, whoOf } from './ui.mjs'

const local = ms => { const d = new Date(ms), p = n => String(n).padStart(2, '0'); return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}` }

function row(w, words) {
  const who = whoOf({ id: w.session_id, mark: w.mark, name: w.who.en })
  return html`<li class="wk-item${w.pending ? ' is-pending' : ''}" id="wk-${w.id}">
<span class="wk-who">${avatar({ ...who, online: true }, { crown: false })}</span>
<div class="wk-words"><p class="wk-next"><b>${w.next.en}</b><span class="wk-repeat${w.recurring ? ' is-again' : ''}">${w.how_often.en}</span></p>
<p class="wk-note">${w.note}</p>
<small class="wk-by">${w.who.en}${w.helper ? html` · ${w.helper}` : ''}${w.last ? html` · ${w.last.en}` : ''}</small>${w.pending ? html`<small class="wk-pending">${w.pending.en}</small>` : ''}</div>
<div class="wk-acts"${w.pending ? raw(' hidden') : ''}><details class="wk-move"><summary class="kn-btn">${words.move_to.en}</summary><form method="post" action="/wakeups/${w.session_id}/${w.id}/move"><input type="datetime-local" name="at" value="${local(w.at_ms)}" required aria-label="${words.move_to.en}"><button type="submit" class="kn-btn is-main">${words.save.en}</button></form></details><details class="kn-del"><summary class="kn-btn is-danger">${words.cancel.en}</summary><form method="post" action="/wakeups/${w.session_id}/${w.id}/cancel" class="kn-del-ask"><span>${words.cancel_ask.en}</span><button type="submit" class="kn-btn is-danger-main">${words.cancel.en}</button></form></details></div></li>`
}

function page(v) {
  return html`<main id="wakeups" class="kn-page wk-page" aria-label="${v.title.en} · ${v.team.en}"><div class="kn-column">
<header class="kn-head page-head"><p class="kn-team">${sk('desk')}<span>${v.team.en}</span></p><h2>${v.title.en} <span class="kn-total" aria-label="${v.summary.en}">(${v.count})</span></h2><p>${v.lead.en}</p></header>
<ol class="kn-list wk-list" id="wk-list">${(v.rows ?? []).map(w => row(w, v.words))}</ol>
${v.none ? html`<p class="kn-none" id="wk-none">${v.none.en}</p>` : ''}
</div></main>`
}

export function register(t) {
  t.get(/^\/wakeups$/, ({ req, res, url }) => {
    const team = url.searchParams.get('team')
    if (team != null) { t.setTeam(team); return t.redirect(res, '/wakeups') }
    const v = t.vm().wakeups
    if (!v) return t.notFound(req, res, 'Wakeups are still being read.')
    t.page(req, res, { title: `${v.title.en} · ${v.team.en} · Trommi`, view: 'wakeups', bodyAttrs: ' data-page="knowledge"', main: page(v) })
  })
  t.post(/^\/wakeups\/([0-9a-f]{32})\/([0-9a-f]{32})\/cancel$/, ({ res, match }) => { t.act({ do: 'wakeup_cancel', session_id: match[1], wakeup_id: match[2] }); t.redirect(res, '/wakeups') })
  t.post(/^\/wakeups\/([0-9a-f]{32})\/([0-9a-f]{32})\/move$/, ({ res, match, form }) => {
    const at = new Date(String(form.get('at') ?? '')).getTime()
    if (Number.isFinite(at)) t.act({ do: 'wakeup_move', session_id: match[1], wakeup_id: match[2], at_ms: at })
    t.redirect(res, '/wakeups')
  })
}
