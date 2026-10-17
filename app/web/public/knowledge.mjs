// ---- the team's knowledge ----
// The page Knowledge of the team in view (vm.knowledge, KnowledgeView: the core's order, search, filter and words):
//   GET  /knowledge                    every entry, pinned first then the one changed last (?q=<words>: the search;
//                                      ?filter=pinned: the pinned ones; ?team=<id>: that team first; ?add=1: the
//                                      editor of a new entry open)
//   POST /knowledge/save               an entry written by the person: new, or anew (Action::KnowledgeSave)
//   POST /knowledge/<id>/pin           pinned or not (Action::KnowledgePin; on=1|0)
//   POST /knowledge/<id>/delete        taken away (Action::KnowledgeDelete)
// Agents propose entries with a yes/no card; the core writes one on his Yes. Nothing here decides: the shell draws.
import { Controller, controller, html, raw, sk } from './ui.mjs'

const path = (q, filter) => {
  const p = new URLSearchParams()
  if (q) p.set('q', q)
  if (filter) p.set('filter', filter)
  const s = p.toString()
  return `/knowledge${s ? `?${s}` : ''}`
}
const back = form => { const b = String(form.get('back') ?? ''); return b.startsWith('/knowledge') ? b : '/knowledge' }

/** The editor of an entry (none: a new one): its title, its text, pinned, Save and Cancel. */
function editor(k, e, here) {
  const w = k.words
  return html`<form class="kn-edit" method="post" action="/knowledge/save" data-knowledge-target="editor"${e ? html` data-for="${e.id}"` : raw(' id="kn-new"')} hidden>
<input type="hidden" name="team_id" value="${k.team_id}">${e ? html`<input type="hidden" name="entry_id" value="${e.id}">` : ''}<input type="hidden" name="back" value="${here}">
<input class="kn-edit-title" name="title" value="${e?.title ?? ''}" maxlength="${k.title_max}" placeholder="${w.title_hint.en}" aria-label="${w.title_hint.en}" autocomplete="off" required>
<textarea class="kn-edit-text" name="text" rows="3" maxlength="${k.text_max}" placeholder="${w.text_hint.en}" aria-label="${w.text_hint.en}">${e?.text ?? ''}</textarea>
<div class="kn-edit-foot"><label class="kn-edit-pin"><input type="checkbox" name="pinned" value="1"${e?.pinned ? raw(' checked') : ''}>${sk('pin')}<span>${w.pin.en}</span></label><span class="kn-edit-acts"><button type="button" class="kn-btn" data-action="knowledge#cancel">${w.cancel.en}</button><button type="submit" class="kn-btn is-main">${w.save.en}</button></span></div>
</form>`
}

/** One entry: folded, its title, two lines of its text, who brought it and when, the pin; open, all of it and
 *  Edit, Pin or Unpin, Delete (asked once more). */
function entry(k, e, here) {
  const w = k.words
  return html`<li class="kn-item${e.pinned ? ' is-pinned' : ''}" id="kn-${e.id}">
<details class="kn-entry" data-knowledge-target="entry"><summary><span class="kn-ico" aria-hidden="true">${sk('book')}</span><span class="kn-words"><b class="kn-title">${e.title}</b>${e.text ? html`<span class="kn-text">${e.text}</span>` : ''}<small class="kn-by">${e.by.en}</small></span>${e.pinned ? html`<span class="kn-pinned" title="Pinned" aria-label="Pinned">${sk('pin')}</span>` : ''}</summary>
<div class="kn-acts"><button type="button" class="kn-btn" data-action="knowledge#edit" data-knowledge-id-param="${e.id}">${w.edit.en}</button><form method="post" action="/knowledge/${e.id}/pin"><input type="hidden" name="on" value="${e.pinned ? '0' : '1'}"><input type="hidden" name="back" value="${here}"><button type="submit" class="kn-btn">${e.pinned ? w.unpin.en : w.pin.en}</button></form><details class="kn-del"><summary class="kn-btn is-danger">${w.delete.en}</summary><form method="post" action="/knowledge/${e.id}/delete" class="kn-del-ask"><input type="hidden" name="back" value="${here}"><span>${w.delete_ask.en}</span><button type="submit" class="kn-btn is-danger-main">${w.delete.en}</button></form></details></div>
</details>${editor(k, e, here)}</li>`
}

function page(k, addOpen) {
  const here = path(k.query, k.filter), w = k.words
  const filters = k.filters.map(f => html`<a data-nav href="${path(k.query, f.key)}"${f.on ? raw(' aria-current="true"') : ''}>${f.label.en} <span class="kn-count">${f.count}</span></a>`)
  return html`<main id="knowledge" class="kn-page" aria-label="${k.title.en} · ${k.team.en}"><div class="kn-column" data-controller="knowledge" data-knowledge-add-value="${addOpen ? '1' : ''}">
<header class="kn-head page-head"><h2>${k.title.en} <span class="kn-total" aria-label="${k.summary.en}">(${k.count})</span></h2><p>${k.lead.en}</p></header>
<div class="kn-tools"><form class="kn-search" method="get" action="/knowledge" role="search" data-knowledge-target="search"><label>${sk('search')}<span class="offscreen">${k.search.en}</span><input type="search" name="q" value="${k.query}" placeholder="${k.search.en}" autocomplete="off" data-action="input->knowledge#find"${k.query ? raw(' autofocus') : ''}></label>${k.filter ? html`<input type="hidden" name="filter" value="${k.filter}">` : ''}</form>
<nav class="art-kinds kn-filter" aria-label="Filter">${filters}</nav>
<button type="button" class="kn-add" data-action="knowledge#add"${k.full ? raw(' disabled') : ''}><span aria-hidden="true">+</span> ${w.add.en}</button></div>
${k.full ? html`<p class="kn-full" role="note">${w.full.en}</p>` : ''}
${editor(k, null, here)}
<ol class="kn-list" id="kn-list">${k.rows.map(e => entry(k, e, here))}</ol>
${k.none ? html`<p class="kn-none" id="kn-none">${k.none.en}</p>` : ''}
</div></main>`
}

export function register(t) {
  t.get(/^\/knowledge$/, ({ req, res, url }) => {
    const team = url.searchParams.get('team')
    if (team != null) { t.setTeam(team); url.searchParams.delete('team'); return t.redirect(res, `/knowledge${url.search}`) }
    const k = t.vm().knowledge
    if (!k) return t.notFound(req, res, 'Knowledge is still being read.')
    t.page(req, res, { title: `${k.title.en} · ${k.team.en} · Trommi`, view: 'knowledge', bodyAttrs: ' data-page="knowledge"', main: page(k, url.searchParams.has('add')) })
  })
  t.post(/^\/knowledge\/save$/, ({ res, form }) => {
    const entry = String(form.get('entry_id') ?? '')
    t.act({ do: 'knowledge_save', team_id: String(form.get('team_id') ?? ''), ...(entry ? { entry_id: entry } : {}), title: String(form.get('title') ?? ''), text: String(form.get('text') ?? ''), pinned: form.get('pinned') === '1' })
    t.redirect(res, back(form))
  })
  t.post(/^\/knowledge\/([0-9a-f]{32})\/pin$/, ({ res, match, form }) => { t.act({ do: 'knowledge_pin', entry_id: match[1], on: form.get('on') === '1' }); t.redirect(res, back(form)) })
  t.post(/^\/knowledge\/([0-9a-f]{32})\/delete$/, ({ res, match, form }) => { t.act({ do: 'knowledge_delete', entry_id: match[1] }); t.redirect(res, back(form)) })
}

// ---- controller "knowledge": the search as one types, the editors ----
controller('knowledge', class extends Controller {
  static targets = ['search', 'editor', 'entry']
  static values = { add: String }
  connect() {
    // (the search goes on where it was typed: the page is drawn again with the words, the field keeps the keyboard)
    const q = this.searchTarget.querySelector('input[name="q"]')
    if (q && document.activeElement === q) q.setSelectionRange(q.value.length, q.value.length)
    if (this.addValue) this.add()
  }
  disconnect() { clearTimeout(this.timer) }
  find(e) {
    clearTimeout(this.timer)
    const form = this.searchTarget, q = e.target.value.trim(), filter = form.querySelector('input[name="filter"]')?.value ?? ''
    this.timer = setTimeout(() => window.Turbo?.visit(path(q, filter), { action: 'replace' }), 220)
  }
  /** The editor of a new entry, at the head of the list. */
  add() { this.open(this.editorTargets.find(f => f.id === 'kn-new')) }
  /** An entry's editor in its place. */
  edit({ params: { id } }) {
    const f = this.editorTargets.find(x => x.dataset.for === id)
    f?.previousElementSibling?.setAttribute('hidden', '')
    this.open(f)
  }
  open(f) {
    if (!f) return
    for (const o of this.editorTargets) if (o !== f && !o.hidden) this.shut(o)
    f.hidden = false
    const title = f.querySelector('input[name="title"]')
    title?.focus()
    if (title) title.setSelectionRange(title.value.length, title.value.length)
  }
  cancel(e) { this.shut(e.target.closest('form')) }
  shut(f) {
    f.reset()
    f.hidden = true
    f.previousElementSibling?.removeAttribute('hidden')
  }
})
