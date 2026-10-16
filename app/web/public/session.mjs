// A session's page, drawn from the app core's view model (vm.sessions[key], SessionView): its heading, the talk, the
// field to write, the switch "All | Questions", the files drawer and a picture as a page of its own. The markup is the
// one app.css and session.css style.
//
//   GET  <base>/chat/<slug>                 the talk (the core's window of the newest rows; "Earlier messages" grows it)
//   GET  <base>/chat/<slug>/questions       its questions (SessionView.questions)
//   GET  <base>/chat/<slug>/files           the talk with the files drawer open (SessionView.files)
//   GET  <base>/chat/<slug>/files/<file>    one picture, large
//   GET  <base>/chat/<slug>/artifact/<id>   what it published: the viewer at /artifacts/<id> (media.mjs)
//   POST <base>/chat/<slug>/message         the field: text, files (t.attach), copied cards -> Action::Write
//   POST <base>/chat/<slug>/earlier         "Earlier messages" -> Action::ShowMore
//
// An open question stands in the talk as its Desk row (ui.mjs deskRow), whose text links to the card's page; what
// became of a question is one quiet line that links there too.
import { Controller, agoShort, agoSpan, avatar, controller, usageRings, deskRow, fileTile, handSvg, html, linkNote, mq, raw, revokeTiles, rich, ringSvg, runSection, sessionHeadEdit, sk, whoOf, assetGlyph } from './ui.mjs'
import { viewModule } from './app.mjs'
// (the Artifacts' drawing, the share control and the file kinds are media.mjs's: loaded before a page is drawn)
let media = null
const artifactCard = (...a) => media.artifactCard(...a), sizeText = n => media.sizeText(n), typeOf = f => media.typeOf(f), until = (...a) => media.until(...a)
const MAX_FILES = 12                 // as the hub takes them in one message

// ---- small pieces ----
const ICONS = {
  asked: ['M12 3.5a8.5 8.5 0 1 0 0 17 8.5 8.5 0 0 0 0-17z', 'M9.6 9.7a2.5 2.5 0 1 1 3.7 2.2c-.8.45-1.3 1-1.3 1.9', 'M12 16.7v.1'],
  decided: ['M5 12.5l4.5 4.5L19 7.5'],
  done: ['M2.5 12.5l4 4 8-9', 'M12 16l.5.5 9-9.5'],
  urgency: ['M13 3 5.5 13.5H11L10 21l8.5-10.5H13L13 3z'],
  reopened: ['M9 7 4.5 11.5 9 16', 'M4.5 11.5H14a5.5 5.5 0 0 1 0 11h-2'],
  next: ['M9.5 5.5 16 12l-6.5 6.5'],
  file: ['M7 3.5h7l4.5 4.5V19a1.5 1.5 0 0 1-1.5 1.5H7A1.5 1.5 0 0 1 5.5 19V5A1.5 1.5 0 0 1 7 3.5z', 'M13.5 3.5V8.5H18.5'],
  send: ['M12 19V5M5.5 11.5 12 5l6.5 6.5'],
  down: ['M12 5v14M5.5 12.5 12 19l6.5-6.5'],
  close: ['M6 6l12 12M18 6 6 18'],
  clip: ['M8 12.5l6.2-6.2a3 3 0 0 1 4.3 4.3l-7.6 7.6a5 5 0 0 1-7.1-7.1L11 3.9'],
  copy: ['M10.5 8.5h7a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2h-7a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2z', 'M15.5 5.5v-.5a2 2 0 0 0-2-2h-7a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2H7'],
}
ICONS.took_back = ICONS.handed_back = ICONS.reopened
const ico = (name, cls = 'ico') => raw(`<svg viewBox="0 0 24 24" class="${cls}" aria-hidden="true">${(ICONS[name] ?? ICONS.asked).map(d => `<path d="${d}"/>`).join('')}</svg>`)
// The session's own drawing (and its crown) beside its words, as in its heading; never shown greyed when it is offline.
const agentMark = s => avatar({ ...whoOf(s), online: true })

const two = n => String(n).padStart(2, '0')
const clock = ms => { const d = new Date(ms); return `${two(d.getHours())}:${two(d.getMinutes())}` }
const FULL = new Intl.DateTimeFormat('en-GB', { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' })
const timeNode = (ms, cls) => html`<time class="${cls}" datetime="${new Date(ms).toISOString()}" title="${FULL.format(ms)}">${clock(ms)}</time>`

const sessionPath = (s, base) => `${base}/chat/${encodeURIComponent(s.slug)}`
const cardPath = (card, s, base) => `${sessionPath(s, base)}/card/${encodeURIComponent(card.number || card.id)}`

// ---- files of a row ----
function attachments(ids, s, vm, t, from) {
  const all = (ids ?? []).map(id => ({ id, f: vm.files?.[id] })).filter(x => x.f)
  const of = k => all.filter(x => typeOf(x.f) === k)
  const images = of('image'), rest = all.filter(x => !['image', 'video', 'audio'].includes(typeOf(x.f)))
  return html`${all.filter(x => ['video', 'audio'].includes(typeOf(x.f))).map(({ id, f }) => { const url = t.fileUrl(id); return typeOf(f) === 'video'
    ? html`<figure class="media media-video"><video${url ? html` src="${url}#t=0.001"` : ''} controls preload="metadata" playsinline></video><figcaption>${f.name}</figcaption></figure>`
    : html`<figure class="media media-audio"><audio${url ? html` src="${url}"` : ''} controls preload="metadata"></audio><figcaption>${f.name}</figcaption></figure>` })}${images.length ? html`<div class="shots${images.length === 1 ? ' shots-one' : ''}">${images.map(({ id, f }) => { const url = t.fileUrl(id); return html`<a class="shot" data-nav href="${sessionPath(s, t.BASE)}/files/${id}?from=${from}" aria-label="Enlarge ${f.name}"><img${url ? html` src="${url}"` : ''} alt="${f.name}" loading="lazy" decoding="async" width="320" height="240"></a>` })}</div>` : ''}${rest.length ? html`<div class="files">${rest.map(({ id, f }) => { const url = t.fileUrl(id); return html`<a class="file-chip"${url ? html` href="${url}" download="${f.name}"` : ''}>${ico('file')}<span>${f.name}</span></a>` })}</div>` : ''}`
}

// ---- one quiet line for something that happened to a question (EventView) ----
function eventLine(id, ev, card, ms, s, base, { cont = false } = {}) {
  const kind = /^[a-z_]+$/.test(ev.kind ?? '') ? ev.kind : 'board'
  const cls = `event event-${kind}${cont ? ' cont' : ''}${ev.echo ? ' event-echo' : ''}`
  const inner = html`<span class="event-ico">${ico(ICONS[kind] ? kind : 'asked')}</span><span class="event-body"><span class="event-kind">${ev.label.en}</span>${ev.about ? html`<span class="event-about">${ev.about}</span>` : ''}${ev.text ? html`<span class="event-text">${ev.text}</span>` : ''}${(ev.said ?? []).map(x => html`<span class="event-text">${x.en}</span>`)}</span>${timeNode(ms, 'event-time')}`
  const at = id ? html` id="${id}"` : ''
  return card ? html`<a class="${cls}"${at} data-nav href="${cardPath(card, s, base)}" title="Nr. ${card.number}">${inner}</a>` : html`<div class="${cls}"${at}>${inner}</div>`
}

// ---- a turn's trail (WorkView), folded away ----
function workLine(x) {
  if (x.kind === 'text') return html`<li class="work-say">${words(x.text ?? '')}</li>`
  const state = x.state || 'running'
  const line = html`<span class="work-sign" title="${x.says?.en ?? ''}" aria-label="${x.says?.en ?? ''}">${state === 'running' ? html`<i class="work-run" aria-hidden="true"></i>` : { ok: '✓', failed: '✕', interrupted: '■' }[state] ?? ''}</span><b class="work-tool">${x.tool ?? 'Step'}</b>${x.what ? html`<span class="work-what">${x.what}</span>` : ''}${x.count ? html`<span class="work-count">${x.count.en}</span>` : ''}${x.took ? html`<span class="work-ms">${x.took}</span>` : ''}`
  return html`<li class="work-step is-${state}${x.kind === 'helper' ? ' is-helper' : ''}">${x.input || x.output
    ? html`<details class="work-more"><summary class="work-line">${line}</summary>${x.input ? html`<pre class="work-io">${x.input}</pre>` : ''}${x.output ? html`<pre class="work-io work-out">${x.output}</pre>` : ''}</details>`
    : html`<div class="work-line">${line}</div>`}</li>`
}
const workBlock = (r, w) => html`<article class="msg msg-work is-${w.state}" id="msg-${r.id}"><details class="work"><summary class="work-head"><span class="work-sign" aria-hidden="true">${w.state === 'running' ? html`<i class="work-run"></i>` : w.state === 'done' ? '✓' : w.state === 'failed' ? '✕' : '■'}</span><b>${w.head.en}</b>${w.sum ? html`<span class="work-sum">${w.sum.en}</span>` : ''}${w.now ? html`<span class="work-now">${w.now}</span>` : ''}${timeNode(r.time_ms, 'msg-time')}</summary>
<ol class="work-list">${w.lines.map(workLine)}</ol>${w.rest ? html`<p class="work-rest">${w.rest.en}</p>` : ''}</details></article>`

// The agent's words (ui.mjs rich), with its code blocks dressed: a head with the button that copies (controller "copy").
const CODE_HEAD = `<div class="code" data-controller="copy"><div class="code-head"><span class="code-lang">Code</span><button type="button" class="code-copy" data-action="copy#copy">${ico('copy')}<span data-copy-target="label">Copy</span></button></div><pre data-copy-target="source"><code>`
const words = (text, opts) => raw(String(rich(text, opts)).replaceAll('<pre><code>', CODE_HEAD).replaceAll('</code></pre>', '</code></pre></div>'))
/** Under a message he typed into the session's terminal: where it came from. */
const VIA_TERMINAL = html`<span class="msg-via" title="From the terminal"><span class="msg-via-sign" aria-hidden="true">&gt;_</span>from the terminal</span>`
/** The cards a row carries (copied cards): a chip each that opens the card. */
const cardChips = (r, s, base) => (r.chips?.length ? html`<div class="cardclip-row">${r.chips.map(c => html`<a class="cardclip-chip is-link" data-nav href="${cardPath(c, s, base)}" title="Nr. ${c.number} · ${c.title}"><b>Nr. ${c.number}</b><span class="cardclip-title">${c.title}</span></a>`)}</div>` : '')

// ---- one row of the talk (TalkRow) ----
function talkRow(r, s, vm, t) {
  const base = t.BASE, id = `msg-${r.id}`
  if (r.kind === 'event') return eventLine(id, r.event, r.card, r.time_ms, s, base, { cont: r.cont })
  if (r.kind === 'work') return workBlock(r, r.work)
  // An open question where it was asked: its Desk row (its own links keep the session in the address).
  if (r.kind === 'ask') return html`<div class="ask ask-card" id="${id}">${raw(String(deskRow(r.row, base)).replaceAll(`href="${base}/card/`, `href="${sessionPath(s, base)}/card/`))}</div>`
  const about = r.card ? html`<a class="msg-about" data-nav href="${cardPath(r.card, s, base)}"><span class="caps">About</span><span>${r.card.title}</span></a>` : ''
  // A note he sent from the Notes stack: on his side, as the slip of yellow paper it was, stuck on with tape.
  if (r.kind === 'note') return html`<article class="msg msg-user msg-note-at" id="${id}"><figure class="msg-note"><i class="msg-note-tape" aria-hidden="true"></i><figcaption class="msg-note-cap">${sk('page')}<span>Note</span>${r.written_ms ? html`<span class="msg-note-when">written ${clock(r.written_ms)}</span>` : ''}</figcaption><p>${r.text}</p>${attachments(r.files, s, vm, t, r.id)}</figure>${timeNode(r.time_ms, 'msg-time')}</article>`
  if (r.kind === 'user') {
    // A hand-back, a take-back, What??: a quiet line for the deed; what he wrote with it stays his bubble.
    const deed = r.deed ? eventLine(r.text || r.files?.length || r.chips?.length ? `${id}-deed` : id, r.deed, r.card, r.time_ms, s, base, { cont: r.cont }) : ''
    if (deed && !(r.text || r.files?.length || r.chips?.length)) return deed
    const scribble = r.scribble && r.files?.[0] ? t.fileUrl(r.files[0]) : null
    return html`${deed}<article class="msg msg-user${r.cont ? ' cont' : ''}${r.pending ? ' is-pending' : ''}" id="${id}">${r.scribble ? html`<a class="scribble-card"${scribble ? html` href="${scribble}"` : ''} target="_blank" rel="noopener" aria-label="Scribble sent: open the picture"><img${scribble ? html` src="${scribble}"` : ''} alt="" loading="lazy" decoding="async" width="280" height="210"><span>Scribble</span></a>` : attachments(r.files, s, vm, t, r.id)}${r.deed ? '' : about}${cardChips(r, s, base)}${r.text ? html`<div class="bubble"><p>${r.text}</p></div>` : ''}${r.terminal ? html`<span class="msg-foot">${VIA_TERMINAL}${timeNode(r.time_ms, 'msg-time')}</span>` : timeNode(r.time_ms, 'msg-time')}</article>`
  }
  // The agent's words (light markdown; a layout as html goes into the sandboxed frame), or what it published.
  const body = r.kind === 'artifact'
    ? artifactCard({ id: r.published, note: r.text, from: r.id }, base)
    : raw(String(words(r.text ?? '', { extra: r.html ?? '' })).replace(/<\/div>$/, () => `${attachments(r.files, s, vm, t, r.id)}</div>`))
  return html`<article class="msg msg-agent${r.cont ? ' cont' : ''}" id="${id}"${r.cont ? html` title="${FULL.format(r.time_ms)}"` : ''}>${r.cont ? '' : html`<header class="msg-head">${agentMark(s)}<span class="msg-name">${s.name.en}</span>${r.helper ? html`<span class="msg-helper">${r.helper}</span>` : ''}${timeNode(r.time_ms, 'msg-time')}</header>`}${about}${body}${cardChips(r, s, base)}${r.details ? html`<details class="msg-details"><summary>Details</summary>${words(r.details)}</details>` : ''}</article>`
}
// (every child of the log has an id: the page drawn again is morphed row by row)
const talk = (s, vm, t) => s.talk.map(r => html`${r.day ? html`<div class="day" id="day-${r.id}"><span>${r.day.en}</span></div>` : ''}${talkRow(r, s, vm, t)}`)

// ---- the heading ----
const DOTS = raw('<svg class="t-head-dots" viewBox="0 0 24 24" aria-hidden="true"><path d="M4.4 11.1Q5.9 9.9 7.2 11.3Q7.9 13.2 6.2 14Q4.3 14.2 3.9 12.6Q3.8 11.7 4.4 11.1Z"/><path d="M10.7 10.9Q12.4 10 13.5 11.4Q14 13.3 12.3 14Q10.5 14 10.1 12.4Q10.1 11.5 10.7 10.9Z"/><path d="M17 11.2Q18.5 10 19.8 11.2Q20.5 13 18.9 13.9Q17 14.2 16.5 12.7Q16.4 11.8 17 11.2Z"/></svg>')
const forms = (s, base) => `${base}/sessions/${encodeURIComponent(s.id)}`
/** The session's More menu: Files, Move to Team… and Copy Invite Link (not for a helper: it goes with its main), Archive (while it is away; "Back from the Archive"
 *  once in it), Delete…. */
function sessionMore(s, vm, base) {
  const name = s.name.en, n = s.files?.length ?? 0
  const files = n ? html`<a role="menuitem" class="desk-move-files" href="${sessionPath(s, base)}/files" data-action="files#toggle" aria-controls="files-drawer-${s.id}" title="Files">${ico('clip')}<span>Files</span><b>${n}</b></a><hr class="desk-move-rule">` : ''
  const others = (vm.sidebar?.teams ?? []).length < 2 ? [] : vm.sidebar.teams.filter(d => d.id !== s.team_id)
  const move = d => html`<form method="post" action="${forms(s, base)}/edit"><input type="hidden" name="stay" value="1"><input type="hidden" name="moved" value="1"><input type="hidden" name="leave" value="1"><button type="submit" role="menuitem" name="team" value="${d.id}">${sk('desk')}<span>${d.name.en || 'Personal'}</span></button></form>`
  const archive = s.online && !s.archived ? '' : html`<form method="post" action="${forms(s, base)}/edit"><input type="hidden" name="stay" value="1"><input type="hidden" name="archived" value="${s.archived ? '0' : '1'}"><button type="submit" role="menuitem">${sk('archive')}<span>${s.archived ? 'Back from the Archive' : 'Archive'}</span></button></form>`
  const del = html`<details class="desk-move-sub desk-move-delete"><summary role="menuitem">${sk('bin')}<span>Delete…</span></summary>
<form class="session-delete-ask" method="post" action="${forms(s, base)}/delete"><input type="hidden" name="stay" value="1"><p><b>Delete ${name}?</b> Its connector is removed from the room and the session moves to the archive with its helpers. Open questions are shredded.</p>
<div class="desk-duck-ways"><button type="submit" class="session-delete-yes">${sk('bin')}<span>Delete</span></button><button type="button" class="desk-duck-no" data-pop-close>Cancel</button></div></form></details>`
  return html`<details class="t-pick t-pick-desk"><summary class="t-head-desk t-head-more" title="More" aria-label="${name}: more">${DOTS}</summary>
<div class="desk-move t-pop" role="menu" aria-label="More for ${name}">${files}${others.length ? html`<details class="desk-move-sub"><summary role="menuitem">${sk('desk')}<span>Move to Team…</span></summary>${others.map(move)}</details>` : ''}
<form method="post" action="/pair" data-turbo-frame="_top"><input type="hidden" name="role" value="agent"><input type="hidden" name="continue" value="${s.id}"><button type="submit" role="menuitem">${sk('key')}<span>Copy Invite Link</span></button></form><hr class="desk-move-rule">${archive}${del}</div></details>`
}
/** The session's drawing and name (the mark opens the drawings, the crown crowns, the name renames: ui.mjs), its dot,
 *  its model, the More menu, and the raised red hand when it is stopped (else the quiet hint). */
function sessionWho(s, vm, base) {
  const d = s.dot
  return html`<div class="pane-who" id="session-who-${s.id}"${s.main ? raw(' data-main') : ''}><h2 class="pane-name offscreen">${s.name.en}</h2>${sessionHeadEdit({ ...whoOf(s), starred: s.crowned }, base, { stay: true })}<span class="head-dot${d.on ? ' is-on' : ''}"${d.link ? html` data-link="${d.link}"` : ''} role="img" aria-label="${d.says.en}" title="${d.word.en}"><i></i></span>${s.model_label ? html`<span class="head-model" title="${s.model}">${s.model_label}</span>` : ''}<span class="head-use-mini">${usageRings(s.usage, { mini: true })}</span>${sessionMore(s, vm, base)}${s.hand ? html`<span class="t-blocked" data-why="${s.hand.why}" role="status">${raw(handSvg())}<span>Stopped: ${s.hand.text.en}</span></span>` : s.quiet ? html`<span class="t-quiet" title="Quiet">${s.quiet.en}</span>` : ''}</div>`
}
/** The quiet two-way switch "All | Questions n" in the heading's line (a phone: its own row under the top line). */
export function sessionFilters(s, base, mode = '', { phone = false } = {}) {
  const on = mode === 'questions', n = s.open
  return html`<nav class="session-mode${phone ? ' is-phone' : ''}" id="session-filters-${phone ? 'phone-' : ''}${s.id}" aria-label="Show"><span class="session-seg"><a data-nav href="${sessionPath(s, base)}"${on ? '' : raw(' aria-current="page"')}>All</a><a data-nav href="${sessionPath(s, base)}/questions"${on ? raw(' aria-current="page"') : ''}${phone ? '' : raw(` data-controller="title" data-title-count-value="${n}"`)}>Questions${n ? html`<b aria-label="${n} waiting">${n}</b>` : ''}</a></span></nav>`
}

// ---- the helpers, the status lines and "N open" at the talk's end ----
/** The session's helpers (HelperListView): its agent's subagents, a row each with a dot while it works, name and
 *  state; no way into a chat of their own (their cards and words are this session's). */
const helperList = s => (s.helpers ? html`<section class="session-helpers" id="session-helpers-${s.id}" aria-label="${s.helpers.head.en}"><h3>${s.helpers.head.en} <small>${s.helpers.sub.en}</small></h3><ul>${s.helpers.rows.map(h => html`<li${h.working ? raw(' class="is-working"') : ''}><i aria-hidden="true"></i><b>${h.name}</b><span>${h.says.en}</span></li>`)}</ul></section>` : '')
const lineMark = l => html`<span class="status-mark">${l.state === 'working' ? raw(ringSvg({ drop: true })) : l.state === 'stale' ? html`<i class="status-still" aria-hidden="true"></i>` : l.state === 'decision' ? sk('knock') : l.state === 'later' ? sk('snooze') : sk('tick')}</span>`
/** One row of small chips (mark, title, age), never two rows; what does not fit lies on a stack "+N" (controller
 *  "status-lines"). A tap opens a chip under the row: its whole text, how it stands, the way to its card. */
function sessionStatus(s, base) {
  const lines = s.status_lines ?? []
  return html`<div class="session-status" id="session-status-${s.id}" data-controller="status-lines"${lines.length || s.answering ? '' : raw(' hidden')}>${s.answering ? html`<div class="working">${agentMark(s)}<span>${s.answering.en}</span><span class="dots"><i></i><i></i><i></i></span></div>` : ''}${lines.length ? html`<div class="status-chips" role="list">${lines.map(l => html`<button type="button" class="status-chip" role="listitem" data-state="${l.state}" data-key="${s.id}:${l.id}" data-label="${l.label}" data-detail="${l.detail}" data-said="${l.says.en}"${l.card ? html` data-href="${cardPath(l.card, s, base)}" data-card="${l.card.title}"` : ''} title="${l.label}" aria-expanded="false" aria-controls="status-open-${s.id}" data-action="status-lines#toggle">${lineMark(l)}${l.helper ? html`<small class="status-helper">${l.helper}</small>` : ''}<b>${l.label}</b>${l.updated_ms ? html`<span class="status-age" aria-hidden="true" data-ts="${l.updated_ms}">${agoShort(l.updated_ms)}</span>${agoSpan(l.updated_ms, 'offscreen')}` : ''}</button>`)}</div><div class="status-open" id="status-open-${s.id}" hidden></div>` : ''}</div>`
}
// The one chip open stays open when the page is drawn again.
const statusOpen = new Set()
// No sideways scroll (the owner, 13 October 2026): the chips that fit stand in the row; what does not fit lies on a
// small stack "+N" at its end, and a tap on the stack opens what lies on it under the row.
controller('status-lines', class extends Controller {
  connect() {
    const chip = [...this.element.querySelectorAll('.status-chip[data-key]')].find(c => statusOpen.has(c.dataset.key)); if (chip) this.show(chip)
    const row = this.element.querySelector('.status-chips')
    if (!row) return
    this.pile = Object.assign(document.createElement('button'), { type: 'button', className: 'status-pile', hidden: true })
    this.pile.setAttribute('aria-expanded', 'false')
    this.pile.addEventListener('click', () => this.openPile())
    row.append(this.pile)
    this.more = Object.assign(document.createElement('div'), { className: 'status-more', role: 'list', hidden: true })
    row.after(this.more)
    this.watch = new ResizeObserver(() => this.place()); this.watch.observe(row)
    this.place()
  }
  disconnect() { this.watch?.disconnect(); this.pile?.remove(); this.more?.remove() }
  place() {
    const row = this.element.querySelector('.status-chips'), chips = [...row.querySelectorAll(':scope > .status-chip')]
    for (const c of chips) c.hidden = false
    // (the stack's room is kept free while measuring: as wide as "+9")
    this.pile.hidden = false
    this.pile.innerHTML = '<span class="status-pile-n">+9</span>'
    const room = this.pile.getBoundingClientRect().width + 14, right = row.getBoundingClientRect().right
    const all = chips.every(c => c.getBoundingClientRect().right <= right)
    const over = all ? [] : chips.filter(c => c.getBoundingClientRect().right > right - room)
    for (const c of over) c.hidden = true
    this.pile.hidden = !over.length
    this.pile.innerHTML = `<span class="status-pile-n">+${over.length}</span>`
    this.pile.setAttribute('aria-label', `${over.length} more`)
    this.pile.style.setProperty('--behind', String(Math.min(over.length, 2)))
    this.over = over
    if (!this.more.hidden) { if (over.length) this.fill(); else { this.more.hidden = true; this.pile.setAttribute('aria-expanded', 'false') } }
  }
  fill() { this.more.replaceChildren(...this.over.map(c => { const k = c.cloneNode(true); k.hidden = false; return k })) }
  openPile() {
    if (!this.over?.length) return
    const open = this.more.hidden
    this.more.hidden = !open; this.pile.setAttribute('aria-expanded', String(open))
    if (open) this.fill()
  }
  toggle(e) {
    const chip = e.currentTarget, on = chip.getAttribute('aria-expanded') === 'true'
    for (const c of this.element.querySelectorAll('.status-chip[data-key]')) { c.setAttribute('aria-expanded', 'false'); statusOpen.delete(c.dataset.key) }
    const out = this.element.querySelector('.status-open')
    if (on) { if (out) out.hidden = true; return }
    this.show(chip)
  }
  show(chip) {
    const out = this.element.querySelector('.status-open')
    if (!out) return
    chip.setAttribute('aria-expanded', 'true'); statusOpen.add(chip.dataset.key)
    const d = chip.dataset
    out.innerHTML = String(html`<p class="status-open-text"><b>${d.label}</b>${d.detail ? html`: ${d.detail}` : ''}</p><p class="status-open-foot"><span>${d.said}</span>${d.href ? html`<a class="status-open-card" data-nav href="${d.href}">Open card<small>${d.card}</small></a>` : ''}</p>`)
    out.hidden = false
  }
})
/** "N open": the questions of this session that wait, and the way to the first of them. */
const sessionOpen = (s, base) => (s.open ? html`<a class="open-jump" id="session-open-${s.id}" data-nav href="${s.first_open ? `#msg-${s.first_open}` : `${sessionPath(s, base)}/questions`}"${s.first_open ? raw(' data-turbo="false" data-log-target="open" data-action="log#toOpen"') : ''} aria-label="${s.open_says?.en ?? ''}"><span data-log-target="openText">${s.open_word?.en ?? ''}</span>${ico('down')}</a>` : html`<span id="session-open-${s.id}" hidden></span>`)

// ---- the field to write ----
/** An empty talk: a quiet card from the agent that joined, and the starters as rows; a tap brings one into the field. */
const empty = (s, base) => html`<div class="empty-card" id="talk-empty"><div class="empty-card-who">${agentMark(s)}<div><b>${s.name.en}</b>${s.empty.meta ? html`<small>${s.empty.meta}</small>` : ''}</div></div><p>${s.empty.text.en}</p><div class="empty-rows">${s.empty.starters.map(x => html`<a class="empty-row" data-nav href="${sessionPath(s, base)}?say=${encodeURIComponent(x.en)}"><span>${x.en}</span>${ico('send')}</a>`)}</div></div>`
/** The field: a plain form. The controller "composer" adds Enter to send, the field that grows, the chosen files. */
const composer = (s, base, { text = '', focus = false } = {}) => html`<form class="composer" id="composer-${s.id}" method="post" action="${sessionPath(s, base)}/message" enctype="multipart/form-data" data-controller="composer" data-composer-agent-value="${s.id}"${focus ? raw(' data-composer-focus-value="true"') : ''} data-action="turbo:submit-start->composer#start turbo:submit-end->composer#sent click->composer#aim dragover@window->composer#over dragleave@window->composer#left drop@window->composer#drop">
<input type="hidden" name="stay" value="1">
<div class="composer-files" data-composer-target="chips"></div>
<label class="mic composer-clip" title="Attach">${sk('clip')}<input class="offscreen" type="file" name="files" multiple data-composer-target="picker" data-action="change->composer#paint" aria-label="Attach pictures or files (at most ${MAX_FILES})"></label>
<textarea name="text" id="composer-field-${s.id}" rows="1" data-composer-target="field" data-action="input->composer#typed keydown->composer#keys paste->composer#paste focus->composer#paint" autocomplete="off" enterkeyhint="enter" placeholder="${s.compose.en}" aria-label="Message to ${s.name.en}">${text}</textarea>
<button class="send" type="submit" aria-label="Send" data-composer-target="send">${ico('send')}</button>
</form>`
const sendError = (s, text = '') => html`<p class="send-error" id="session-error-${s.id}" role="alert"${text ? '' : raw(' hidden')}>${text}</p>`

// ---- the questions (SessionView.questions): what waits as Desk rows (SessionView.waiting), then every question ----
function questionList(s, base) {
  const open = s.waiting ?? []
  const line = q => html`<a class="event event-asked cont" id="q-${q.id}" data-nav href="${cardPath(q, s, base)}" title="Nr. ${q.number}"><span class="event-ico">${ico('asked')}</span><span class="event-body"><span class="event-kind">Nr. ${q.number}</span><span class="event-about">${q.title}</span></span></a>`
  return html`<div class="session-cards" id="session-questions-${s.id}"><header class="inbox-head"><div class="inbox-title"><h2>${s.open_word?.en ?? 'No question waits for you.'}</h2></div></header>
<div class="inbox-groups">${open.length ? runSection(whoOf(s), open.map(r => raw(String(deskRow(r, base)).replaceAll(`href="${base}/card/`, `href="${sessionPath(s, base)}/card/`))), open.length) : ''}</div>
${s.questions?.length ? html`<h3 class="hist-heading"><span>All questions</span><b>${s.questions.length}</b></h3><div class="session-past">${s.questions.map(line)}</div>` : ''}</div>`
}

// ---- the files (SessionView.files: every file of its cards, answers, chats and Artifacts, the newest first) ----
function fileThumb(id, f, s, t) {
  const type = typeOf(f), url = t.fileUrl(id)
  if (type === 'image') return html`<a class="files-thumb" data-nav href="${sessionPath(s, t.BASE)}/files/${id}" title="${f.name}"><img${url ? html` src="${url}"` : ''} alt="" loading="lazy" decoding="async" width="64" height="44"></a>`
  return html`<a class="files-thumb asset-preview" data-kind="${type}"${url ? html` href="${url}" download="${f.name}"` : ''} title="${f.name}">${assetGlyph(type)}</a>`
}
function filesList(s, vm, t) {
  const list = (s.files ?? []).map(id => ({ id, f: vm.files?.[id] })).filter(x => x.f)
  return html`<div class="files-list" id="session-files-${s.id}"><p class="files-sum">${list.length === 1 ? '1 file' : `${list.length} files`}</p>${list.length ? list.map(({ id, f }) => html`<section class="files-group" id="file-${id}"><div class="files-group-head"><span class="files-where"><b>${f.name}</b><span>${f.size ? sizeText(f.size) : ''}</span></span></div><div class="files-thumbs">${fileThumb(id, f, s, t)}</div></section>`) : html`<p class="files-empty">Nothing was sent here yet.</p>`}</div>`
}
/** The drawer from the right (a sheet from below on a phone), open at /chat/<slug>/files. */
const filesDrawer = (s, vm, t, open) => html`<aside class="files-drawer" id="files-drawer-${s.id}" data-files-target="drawer" aria-label="Files of this session"${open ? '' : raw(' hidden')}><header class="files-head"><h3>Files</h3><a class="files-close" data-nav href="${sessionPath(s, t.BASE)}" data-action="files#close" aria-label="Close the files">${ico('close')}</a></header><div class="files-frame">${open ? filesList(s, vm, t) : ''}</div></aside>`

// ---- the page ----
/** mode: '' (the talk) | 'questions' | 'files' (the talk, the drawer open). */
function sessionMain(s, vm, t, { mode = '', text = '', focus = false } = {}) {
  const base = t.BASE
  const body = mode === 'questions' ? html`<div class="pane-list pane-questions"><div class="column">${questionList(s, base)}</div></div>`
    : html`<div class="log" tabindex="-1" data-log-target="log" data-action="scroll->log#scrolled"><div class="column log-inner" role="log" aria-live="polite" aria-relevant="additions">${s.earlier ? html`<form class="log-earlier" id="earlier-${s.id}" method="post" action="${sessionPath(s, base)}/earlier"><button type="submit" class="log-earlier-link" data-log-target="earlier">${s.earlier.says.en}${s.earlier.left ? html`<b>${s.earlier.left}${s.earlier.hub ? '+' : ''}</b>` : ''}</button></form>` : ''}${s.talk.length ? talk(s, vm, t) : s.empty ? empty(s, base) : ''}<div id="log-end-${s.id}" hidden></div>${helperList(s)}${sessionStatus(s, base)}</div></div>`
  return html`<main id="session" aria-label="Session: ${s.name.en}" data-controller="files" data-action="keydown@document->files#key click@document->files#outside"${mode === 'files' ? raw(' data-files-open') : ''}>
<header class="pane-title">${sessionWho(s, vm, base)}${sessionFilters(s, base, mode)}<div class="head-use" id="session-use-${s.id}">${usageRings(s.usage)}</div></header>${sessionFilters(s, base, mode, { phone: true })}
<section id="chat" aria-label="Conversation"><div class="chat-pane" data-agent="${s.id}" data-controller="log">
<div class="chat-body">${body}</div>
<div class="dock">${mode === 'questions' ? '' : html`<button type="button" class="jump" hidden data-log-target="jump" data-action="log#toEnd">${ico('down')}<span data-log-target="jumpText">To the end</span></button>`}${mode === 'questions' ? '' : sessionOpen(s, base)}<div class="column session-compose">${sendError(s)}${s.note ? linkNote(s.note, { id: `session-link-${s.id}` }) : html`<aside class="link-note" id="session-link-${s.id}" hidden></aside>`}${composer(s, base, { text, focus })}</div></div>
</div></section>
${filesDrawer(s, vm, t, mode === 'files')}
</main>`
}

/** One picture of the session, large, at its own address; the arrows step through its pictures. */
function sessionPicture(s, vm, t, id, from) {
  const pics = (s.files ?? []).filter(x => typeOf(vm.files?.[x]) === 'image'), n = pics.length, i = Math.max(0, pics.indexOf(id))
  const f = vm.files?.[id], url = t.fileUrl(id), here = sessionPath(s, t.BASE), q = from ? `?from=${encodeURIComponent(from)}` : ''
  const back = from ? `${here}#msg-${from}` : `${here}/files`
  const arrow = d => raw(`<svg viewBox="0 0 24 24" class="focus-icon" aria-hidden="true"><path d="${d}"/></svg>`)
  return html`<div class="t-picture">
<header class="t-picture-bar"><a class="focus-back-desk t-picture-back" data-nav href="${back}" aria-label="Back to ${from ? 'the conversation' : 'the files'}">${arrow('M19 12H5M11 6l-6 6 6 6')}<span>${s.name.en}</span></a><span class="t-picture-where"><b>${i + 1} / ${n}</b> ${f?.name ?? ''}</span>${url ? html`<a class="page-chip" href="${url}" download="${f?.name ?? ''}">${sk('picture')}<b>${f?.name ?? ''}</b><i>open</i></a>` : ''}</header>
<a class="t-picture-view" data-nav href="${back}" aria-label="Close the picture"><img${url ? html` src="${url}"` : ''} alt="${f?.name ?? ''}" decoding="async"></a>
${n > 1 ? html`<a class="focus-stage-step is-prev" data-nav href="${here}/files/${pics[(i - 1 + n) % n]}${q}" data-turbo-action="replace" aria-label="The picture before">${arrow('M19 12H5M11 6l-6 6 6 6')}</a><a class="focus-stage-step is-next" data-nav href="${here}/files/${pics[(i + 1) % n]}${q}" data-turbo-action="replace" aria-label="The next picture">${arrow('M5 12h14M13 6l6 6-6 6')}</a>` : ''}
</div>`
}

// ---- routes and forms ----
export function register(t) {
  const { BASE } = t
  const decode = ref => { try { return decodeURIComponent(ref) } catch { return ref } }
  /** The session in view: by its slug (the address) or its key. */
  const find = async (req, res, ref) => {
    media ??= await viewModule('media')
    const id = decode(ref), s = Object.values(t.vm().sessions ?? {}).find(x => x.slug === id || x.id === id)
    if (!s) t.notFound(req, res, 'This session is not on the board.')
    return s
  }
  const send = (req, res, s, opts = {}, code = 200) => t.page(req, res, {
    title: `${s.open ? `(${s.open}) ` : ''}${s.name.en} · Trommi`, view: 'session', current: s.id, scope: s.id,
    bodyAttrs: opts.mode === 'questions' ? ' data-filter="questions"' : '', main: sessionMain(s, t.vm(), t, opts),
  }, code)

  t.get(/^\/chat\/([^/+]+)$/, async ({ req, res, url, match }) => {
    if (url.searchParams.get('only') === 'questions') return t.redirect(res, `${BASE}/chat/${match[1]}/questions`)
    const s = await find(req, res, match[1])
    if (s) send(req, res, s, { text: (url.searchParams.get('say') ?? '').slice(0, 2000), focus: url.searchParams.has('say') })
  })
  t.get(/^\/chat\/([^/+]+)\/(questions|files)$/, async ({ req, res, match }) => { const s = await find(req, res, match[1]); if (s) send(req, res, s, { mode: match[2] }) })
  t.get(/^\/chat\/([^/+]+)\/files\/([\w-]+)$/, async ({ req, res, url, match }) => {
    const s = await find(req, res, match[1])
    if (s) t.page(req, res, { title: `${s.name.en} · picture`, view: 'picture', sidebar: false, bodyAttrs: ' data-focus-page="card"', main: sessionPicture(s, t.vm(), t, match[2], url.searchParams.get('from') ?? '') })
  })
  // Something it published: the viewer of the Artifacts (the core holds them on that screen).
  t.get(/^\/chat\/([^/+]+)\/artifact\/([0-9a-f]{8,64})$/, ({ res, url, match }) => t.redirect(res, `${BASE}/artifacts/${match[2]}${url.search}`))
  // "Earlier messages": the core's window of the talk grows by a page (and reads older talk from the hub).
  t.post(/^\/chat\/([^/+]+)\/earlier$/, async ({ req, res, match }) => {
    const s = await find(req, res, match[1])
    if (!s) return
    t.act({ do: 'show_more', of: { list: 'talk', session: s.id } })
    t.done(res)
  })
  // The field: words, files (each attached to the core, sent once it is up) and copied cards.
  t.post(/^\/chat\/([^/+]+)\/message$/, async ({ req, res, match, form }) => {
    const s = await find(req, res, match[1])
    if (!s) return
    const stay = form.has('stay') && t.wantsStream(req)
    const text = String(form.get('text') ?? '').replace(/\r\n/g, '\n')
    const files = form.getAll('files').filter(f => typeof f === 'object' && f && f.size > 0)
    const cards = [...new Set(form.getAll('cards').map(String).filter(id => /^[0-9a-f]{8,64}$/.test(id)))].slice(0, 5)
    let error = ''
    if (files.length > MAX_FILES) error = `at most ${MAX_FILES} files at once`
    else if (!text.trim() && !files.length && !cards.length) error = 'write something first'
    else {
      const ids = await Promise.all(files.map(f => t.attach(f)))
      const up = await until(t.vm, v => (ids.every(id => ['ready', 'failed'].includes(v.files?.[id]?.state)) ? ids.map(id => v.files[id].state) : null), 10 * 60_000)
      if (!up || up.includes('failed')) error = 'a file did not upload'
      else t.act({ do: 'write', session_id: s.id, text, attachments: ids, copied_cards: cards })
    }
    // 422: not sent, so the composer gives the words and files back (controller "composer").
    if (stay) return t.sendStream(req, res, t.stream('replace', `session-error-${s.id}`, sendError(s, error && `Not sent: ${error}`)), error ? 422 : 200)
    if (error) return send(req, res, s, { text }, 422)
    t.redirect(res, sessionPath(s, BASE))
  })
}

// ---- controller "composer" ----
// The field of a session's page: a plain form that works by itself. This adds what a form cannot do alone: Enter
// sends (Shift+Enter is a new line; on a touch screen Enter stays a new line, and Ctrl/Cmd+Enter sends everywhere), the
// field grows with its words, the chosen files stand as chips that can be taken off, a pasted picture or a file dropped
// anywhere on the page is attached, a card copied elsewhere ("Copy" on a card, controller "clip") is offered and goes
// along as a chip, and what was typed survives a reload. What was sent shows at once: the core's echo.
const MAX_CARDS = 5
const CLIP_KEY = 'trommi-cardclip'   // written by controller "clip": { id, number, title, choice_label, text }
const fine = mq('(pointer: fine)')
const el = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e }
const held = () => { try { const c = JSON.parse(sessionStorage.getItem(CLIP_KEY) ?? 'null'); return c?.id ? c : null } catch { return null } }

controller('composer', class extends Controller {
  static targets = ['field', 'picker', 'chips', 'send']
  static values = { agent: String, focus: Boolean }

  connect() {
    this.cards = []
    if (!this.fieldTarget.value) { try { this.fieldTarget.value = localStorage.getItem(this.key) ?? '' } catch {} }
    this.paint({ fit: false })
    if (this.fieldTarget.value) requestAnimationFrame(() => setTimeout(() => { if (this.element.isConnected) this.fit() }, 0))
    if (this.focusValue) { this.fieldTarget.focus(); this.fieldTarget.setSelectionRange(this.fieldTarget.value.length, this.fieldTarget.value.length) }
  }
  disconnect() { this.revoke() }
  revoke() { if (this.hasChipsTarget) revokeTiles(this.chipsTarget) }
  get key() { return `trommi-draft:${this.agentValue}` }
  forget() { try { localStorage.removeItem(this.key) } catch {} }
  get empty() { return !this.fieldTarget.value.trim() && !this.pickerTarget.files.length && !this.cards.length }

  fit() {
    const field = this.fieldTarget
    if (!field.value) { field.style.height = ''; field.style.overflowY = ''; this.sendTarget.disabled = this.empty; return }
    field.style.height = 'auto'
    const max = Math.max(120, Math.min(260, innerHeight * 0.36))
    field.style.height = `${Math.min(field.scrollHeight, max)}px`
    field.style.overflowY = field.scrollHeight > max ? 'auto' : 'hidden'
    this.sendTarget.disabled = this.empty
  }
  typed() { this.fit(); try { localStorage.setItem(this.key, this.fieldTarget.value) } catch {} }
  keys(e) {
    if (e.key !== 'Enter' || e.shiftKey || e.isComposing || e.keyCode === 229) return
    if (!(fine.matches || e.ctrlKey || e.metaKey)) return
    e.preventDefault()
    if (!this.sendTarget.disabled) this.element.requestSubmit(this.sendTarget)
  }
  aim(e) { if (e.target === this.element) this.fieldTarget.focus() }

  // ---- sending: the field is empty at once; a refusal gives the words, files and cards back ----
  start() {
    this.sending = { text: this.fieldTarget.value, files: [...this.pickerTarget.files], cards: this.cards }
    if (this.cards.length) { try { sessionStorage.removeItem(CLIP_KEY) } catch {} }
    this.fieldTarget.value = ''
    this.cards = []
    this.forget()
    this.setFiles([])
  }
  sent(e) {
    const was = this.sending
    this.sending = null
    if (!was || e.detail.success) return
    if (!this.fieldTarget.value) this.fieldTarget.value = was.text
    if (!this.cards.length) this.cards = was.cards
    if (!this.pickerTarget.files.length) this.setFiles(was.files)
    else this.paint()
    this.typed()
  }

  // ---- files ----
  setFiles(list) {
    const dt = new DataTransfer()
    for (const f of list.slice(0, MAX_FILES)) dt.items.add(f)
    this.pickerTarget.files = dt.files
    this.paint()
  }
  add(files) { if (files.length) this.setFiles([...this.pickerTarget.files, ...files]) }
  off({ params: { at } }) { this.setFiles([...this.pickerTarget.files].filter((_, n) => n !== at)) }

  // ---- copied cards ----
  takeCard() {
    const c = held()
    if (c && !this.cards.some(x => x.id === c.id) && this.cards.length < MAX_CARDS) this.cards = [...this.cards, c]
    this.paint()
    this.fieldTarget.focus()
  }
  dropOffer() { try { sessionStorage.removeItem(CLIP_KEY) } catch {} this.paint() }
  cardOff({ params: { id } }) { this.cards = this.cards.filter(c => c.id !== id); this.paint() }

  paint({ fit = true } = {}) {
    this.revoke()
    const chips = [...this.pickerTarget.files].map((file, at) => {
      const chip = fileTile(file), off = chip.querySelector('.att-off')
      off.dataset.action = 'composer#off'
      off.dataset.composerAtParam = String(at)
      return chip
    })
    for (const c of this.cards) {
      const chip = el('span', 'cardclip-chip')
      const off = el('button', 'cardclip-off', '×')
      off.type = 'button'
      off.setAttribute('aria-label', `Nr. ${c.number}: take it off`)
      off.dataset.action = 'composer#cardOff'
      off.dataset.composerIdParam = c.id
      const input = el('input')
      input.type = 'hidden'; input.name = 'cards'; input.value = c.id
      chip.title = `Nr. ${c.number} · ${c.title}${c.choice_label ? ` → ${c.choice_label}` : ''}`
      chip.append(el('b', '', `Nr. ${c.number}`), el('span', 'cardclip-title', c.title), ...(c.choice_label ? [el('span', 'cardclip-answer', `→ ${c.choice_label}`)] : []), off, input)
      chips.push(chip)
    }
    // A card copied elsewhere and not attached yet: offered, one click attaches it.
    const c = held()
    if (c && !this.cards.some(x => x.id === c.id) && this.cards.length < MAX_CARDS) {
      const offer = el('span', 'cardclip-offer')
      const take = el('button', 'cardclip-paste', `Paste Nr. ${c.number}`)
      take.type = 'button'; take.title = c.title; take.dataset.action = 'composer#takeCard'
      const drop = el('button', 'cardclip-off', '×')
      drop.type = 'button'; drop.setAttribute('aria-label', 'Forget the copied card'); drop.dataset.action = 'composer#dropOffer'
      offer.append(take, drop)
      chips.push(offer)
    }
    this.chipsTarget.replaceChildren(...chips)
    this.element.toggleAttribute('data-files', this.pickerTarget.files.length > 0 || this.cards.length > 0)
    if (fit) this.fit(); else this.sendTarget.disabled = this.empty
  }
  paste(e) {
    // The line a card's "Copy" wrote: the card itself, as a chip.
    const c = held(), said = e.clipboardData?.getData('text/plain')?.trim()
    if (c && said && said === String(c.text ?? '').trim()) { e.preventDefault(); this.takeCard(); return }
    const files = [...(e.clipboardData?.files ?? [])].map(f => (f.name && f.name !== 'image.png' ? f : new File([f], `pasted-${Date.now()}.${(f.type.split('/')[1] ?? 'png').replace('jpeg', 'jpg')}`, { type: f.type })))
    if (files.length) { e.preventDefault(); this.add(files) }
  }
  over(e) { if (e.dataTransfer?.types.includes('Files')) { e.preventDefault(); this.element.classList.add('is-drop') } }
  left(e) { if (!e.relatedTarget) this.element.classList.remove('is-drop') }
  drop(e) {
    this.element.classList.remove('is-drop')
    const files = [...(e.dataTransfer?.files ?? [])]
    if (files.length) { e.preventDefault(); this.add(files); this.fieldTarget.focus() }
  }
})

// ---- controller "log" ----
// A session's talk, on .chat-pane. The log stands at its end by itself (css: a reversed column). This adds what only
// the browser knows: "To the end", with the number of rows that arrived while one was reading further up, and the
// view that stays where it is when they arrive; the "N open" chip only while an open question of the talk is out of
// sight, and the way to the next; "Earlier messages" pressed by itself when it comes near while scrolling up.
controller('log', class extends Controller {
  static targets = ['log', 'jump', 'jumpText', 'open', 'openText', 'earlier']

  connect() {
    this.pinned = true
    this.unread = 0
    if (!this.hasLogTarget) return
    this.height = null
    this.seen = new MutationObserver(list => this.arrived(list))
    this.seen.observe(this.logTarget, { childList: true, subtree: true })
    this.sizes = new ResizeObserver(() => this.paint())
    this.sizes.observe(this.logTarget)
    this.asks = new Map()     // .ask-card -> { away, below }
    this.askSeen = new IntersectionObserver(seen => {
      for (const e of seen) this.asks.set(e.target, { away: !e.isIntersecting, below: e.boundingClientRect.top > (e.rootBounds?.top ?? 0) })
      this.paintOpen()
    }, { root: this.logTarget, rootMargin: '-48px 0px -48px 0px' })
    for (const n of this.logTarget.querySelectorAll('.ask-card')) this.askSeen.observe(n)
    requestAnimationFrame(() => setTimeout(() => { if (!this.hasLogTarget || !this.logTarget.isConnected) return; this.height ??= this.logTarget.scrollHeight; this.paint() }, 0))
  }
  disconnect() { this.seen?.disconnect(); this.sizes?.disconnect(); this.near?.disconnect(); this.askSeen?.disconnect() }

  // "Earlier messages" near the top of what is in view: pressed (only once the reader scrolls).
  earlierTargetConnected(button) {
    if (!this.hasLogTarget) return
    this.near ??= new IntersectionObserver(seen => {
      for (const e of seen) if (e.isIntersecting && e.target.isConnected) { this.near.unobserve(e.target); e.target.click() }
    }, { root: this.logTarget, rootMargin: '900px 0px 0px 0px' })
    if (this.moved) this.near.observe(button)
    else this.waiting = button
  }
  earlierTargetDisconnected(button) { this.near?.unobserve(button) }

  // (The log is a reversed column: its end is scrollTop 0, further up is negative.)
  get atEnd() { return Math.abs(this.logTarget.scrollTop) < 72 }
  scrolled() {
    this.moved = true
    if (this.waiting?.isConnected) this.near?.observe(this.waiting)
    this.waiting = null
    this.pinned = this.atEnd
    if (this.pinned) this.unread = 0
    this.paint()
  }
  toEnd() {
    this.pinned = true
    this.unread = 0
    this.logTarget.scrollTo({ top: 0, behavior: matchMedia('(prefers-reduced-motion: reduce)').matches ? 'instant' : 'smooth' })
    this.paint()
  }
  arrived(list) {
    const log = this.logTarget
    let fresh = 0
    for (const change of list) for (const node of change.removedNodes) if (node.nodeType === 1) for (const n of node.matches('.ask-card') ? [node] : node.querySelectorAll('.ask-card')) { this.asks.delete(n); this.askSeen.unobserve(n) }
    for (const change of list) for (const node of change.addedNodes) {
      if (node.nodeType !== 1) continue
      for (const n of node.matches('.ask-card') ? [node] : node.querySelectorAll('.ask-card')) this.askSeen.observe(n)
      // What is put in at the log's end (before its end mark) is new; his own rows bring the view to the end.
      if (change.nextSibling?.id?.startsWith?.('log-end-')) { if (node.matches('.msg-user')) { this.pinned = true; this.unread = 0 } else if (node.matches('.msg-agent, .ask, .event')) fresh++ }
    }
    if (this.pinned) { this.height = null; this.measureLater() }
    else if (fresh) {
      const grew = this.height == null ? 0 : log.scrollHeight - this.height
      this.height = log.scrollHeight
      this.unread += fresh; if (grew > 0) log.scrollTop -= grew   // reading further up: what is in view stays in view
    } else { this.height = null; this.measureLater() }
    this.paint()
  }
  measureLater() {
    if (this.measuring) return
    this.measuring = true
    requestAnimationFrame(() => setTimeout(() => { this.measuring = false; if (this.hasLogTarget && this.logTarget.isConnected && this.height == null) this.height = this.logTarget.scrollHeight }, 0))
  }
  paint() {
    if (!this.hasLogTarget) return
    if (this.hasJumpTarget) {
      this.jumpTarget.hidden = this.pinned
      this.jumpTarget.classList.toggle('has-unread', this.unread > 0)
      this.jumpTextTarget.textContent = this.unread === 0 ? 'To the end' : this.unread === 1 ? '1 new message' : `${this.unread} new messages`
    }
    this.paintOpen()
  }
  openTargetConnected() { if (this.hasLogTarget) this.paintOpen() }
  // The open questions of the talk that are out of sight, and which of them comes next.
  paintOpen() {
    if (!this.hasOpenTarget) return
    const away = [...(this.asks ?? [])].filter(([node, r]) => r.away && node.isConnected).map(([node, r]) => ({ node, r }))
      .sort((a, b) => (a.node.compareDocumentPosition(b.node) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1))
    const below = away.find(({ r }) => r.below)
    this.next = (below ?? away.at(-1))?.node ?? null
    this.openTarget.hidden = !this.next
    if (!this.next) return
    this.openTarget.classList.toggle('is-up', !below)
    this.openTarget.setAttribute('href', `#${this.next.id}`)
    if (this.hasOpenTextTarget) this.openTextTarget.textContent = `${away.length} open`
    this.openTarget.setAttribute('aria-label', away.length === 1 ? 'One open question out of sight: go to it' : `${away.length} open questions out of sight: go to the next one`)
  }
  toOpen(e) {
    if (!this.next) return
    e.preventDefault()
    this.pinned = false
    this.next.scrollIntoView({ block: 'center', behavior: matchMedia('(prefers-reduced-motion: reduce)').matches ? 'instant' : 'smooth' })
    this.next.querySelector('.inbox-row')?.focus({ preventScroll: true })
  }
})

// ---- controller "files" ----
// A session's files drawer: "Files" in the More menu opens it (/chat/<slug>/files), the × again, Escape or a click
// beside it close it (back to /chat/<slug>).
controller('files', class extends Controller {
  static targets = ['drawer']
  get isOpen() { return this.hasDrawerTarget && !this.drawerTarget.hidden }
  toggle(event) { if (this.isOpen) { event.preventDefault(); this.close() } else event?.target.closest('details')?.removeAttribute('open') }
  close(event) {
    event?.preventDefault()
    if (!this.isOpen) return
    this.drawerTarget.hidden = true
    if (/\/files$/.test(location.pathname)) window.Turbo?.visit(location.pathname.replace(/\/files$/, ''), { action: 'replace' })
  }
  key(event) { if (event.key === 'Escape' && this.isOpen && !event.defaultPrevented) { event.preventDefault(); this.close() } }
  outside(event) {
    if (!this.isOpen || !event.target.isConnected) return
    if (this.drawerTarget.contains(event.target) || event.target.closest('.desk-move-files, .session-mode')) return
    this.close()
  }
})
