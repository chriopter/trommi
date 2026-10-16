// The frame around every page: the sidebar (#agents: one row per session, its helpers a small line in it), the
// Desk box with the Trommi menu, the phone's bars and drawer, and the corner note. Everything it shows is the app
// core's view model (vm.sidebar, vm.notes, vm.settings); what the person does goes to the core as an action (t.act).
// The markup is the one app.css and sidebar.css style.
import { BELL, Controller, PLUS, paintTopStrip, nextThemeMode, setThemeMode, avatar, badge, controller, crownSvg, edgeQuirk, el, html, linkCap, raw, sk, sketchSvg, showToast, whoOf, fileSrc } from './ui.mjs'

/** The board's toolkit (register): the controllers send their actions with it. */
let T = null

/** The one quiet sign of a calm row (SessionRowView.signal), for a wide screen. */
const SIGNAL_SAYS = { stopped: 'Stopped', waiting: 'Waits for you', unread: 'New' }
const signalDot = r => (r.signal ? html`<i class="agent-signal" data-signal="${r.signal}" role="img" aria-label="${SIGNAL_SAYS[r.signal] ?? ''}"></i>` : '')
/** One row of the sidebar (SessionRowView): on a wide screen a calm chat row (drawing, name, what was said last, one
 *  sign); a phone keeps its counts, helper dots and caps (sidebar.css). */
function row(r, base) {
  const who = whoOf(r), name = r.name.en, working = Boolean(r.online && r.working)
  const cls = ['agent-row', r.main && 'is-main', r.open || r.hand ? 'has-badge' : '', !r.online && 'is-offline'].filter(Boolean).join(' ')
  const order = r.group || ''
  return html`<div class="${cls}" id="agent-${r.id}" data-unit="${r.id}" data-slug="${r.slug}"${order ? html` data-order="${order}"` : ''}>
<a class="agent-entry" data-nav href="${base}/chat/${encodeURIComponent(r.slug)}" draggable="false"${order ? raw(' aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown"') : ''} title="${working ? 'Working' : ''}">${avatar(who, { working })}<span class="agent-text"><strong>${name}</strong>${working ? html`<span class="sr-only"> (working)</span>` : ''}${linkCap(r.cap)}<span class="agent-teaser">${r.last?.text ?? ''}</span></span>${signalDot(r)}</a>
${badge(r, base, true)}
</div>`
}

// ---- the session in view, marked in the sidebar ----
// The rows are the same markup on every page (so a page change keeps the sidebar's nodes as they are, app.mjs
// paintBody); which one is the session in view is set on them here, after every page and whenever rows are put in:
// .is-active and aria-current on its row. The session: its key or its slug.
let inView = null
function paintCurrent(root = document) {
  const rows = [...root.querySelectorAll('#agents .agent-row[data-unit]')]
  const here = inView == null ? null : rows.find(r => r.dataset.unit === inView || r.dataset.slug === inView) ?? null
  for (const r of rows) {
    const on = r === here, entry = r.querySelector(':scope > .agent-entry')
    r.classList.toggle('is-active', on)
    if (on) entry?.setAttribute('aria-current', 'page'); else entry?.removeAttribute('aria-current')
  }
}
let watching = null
/** The session in view (its key or slug; null: none): marked in the sidebar now and in every row put in later. */
export function markCurrent(current) {
  inView = current ?? null
  paintCurrent()
  watching ??= new MutationObserver(list => { if (list.some(ch => [...ch.addedNodes].some(n => n.nodeType === 1 && (n.matches('.agent-row, #agents') || n.querySelector?.('.agent-row'))))) paintCurrent() })
  const agents = document.getElementById('agents')
  if (agents && watching.target !== agents) { watching.disconnect(); watching.observe(agents, { childList: true, subtree: true }); watching.target = agents }
}

const inviteAgentButton = () => html`<form method="post" action="/pair" class="agent-invite" id="agent-invite"><input type="hidden" name="role" value="computer"><button type="submit" class="agent-invite-go" id="sidebar-invite" title="Approve a computer" aria-label="Approve a computer">${PLUS}<span class="agent-invite-label">Approve Computer…</span></button></form>`

// ---- the note: one only, at the bottom right ----
// One drawn yellow sticky at the window's bottom-right, a part of the frame beside the sidebar (app.mjs bodyParts).
// Folded it is the sticky alone, and the sticky says how it stands: lines on it when the note holds something, blank
// when it is empty; a click unfolds it there, growing upward, into a field that grows with the words (Enter: a new
// line, Ctrl/Cmd+Enter sends, Esc folds), with the paperclip, the bin and the crown (send straight to the crown). Sent
// or thrown away, it is empty again. It is the core's corner note (vm.notes.corner).
// (The field's words and whether it is written are values of the box, not its markup: the page is morphed after every
// render of the core, and the field under the hand that writes, or the box's open state, must not be replaced.)
const NOTE_ICON = raw('<svg viewBox="0 0 52 52" class="corner-note-ico" aria-hidden="true"><path class="note-fill" d="M9.5 11.2 Q25 9.6 42.6 10.6 Q43.4 25 42.8 38.4 L35.4 45.4 Q21 46.6 9.8 45.8 Q8.6 28 9.5 11.2 Z"/><path class="note-ink" d="M7.6 9.4 Q24 8.2 41.4 8.8 Q42.4 23.6 41.6 37.2 L34.2 44.2 Q20.4 45.2 8.2 44.4 Q6.8 27 7.6 9.4 Z"/><path class="note-ink" d="M41.6 37.2 Q37.2 36.6 34.8 37.6 Q34.1 40.8 34.2 44.2"/><path class="note-lines" d="M14.2 19.4 Q22 18.8 30.6 19.2 M14 25.6 Q20 25.1 26.4 25.5 M14.3 31.6 Q18.6 31.2 22.4 31.5"/></svg>')
const BIN = raw('<svg viewBox="0 0 24 24" class="sketch" aria-hidden="true"><path d="M5 7.2 Q12 6.8 19 7.3"/><path d="M9.6 6.9 Q9.8 4.8 12 4.7 Q14.3 4.8 14.4 6.9"/><path d="M6.6 7.6 Q7.4 14 8.2 20.2 Q12 20.6 15.8 20.2 Q16.6 14 17.4 7.6"/></svg>')
const CLIP = raw('<svg viewBox="0 0 24 24" class="sketch" aria-hidden="true"><path d="M15.6 7.2 Q11 12 8.4 14.8 Q7 16.6 8.6 17.8 Q10.2 18.8 11.6 17.2 Q15.6 12.8 18.2 9.8 Q20.4 7 18.2 5 Q16 3.4 13.8 5.6 Q9.4 10.4 6.4 13.8 Q3.8 17 6.4 19.6 Q9 21.8 12 19"/></svg>')
const RING = raw('<i class="note-up" aria-hidden="true"><svg viewBox="0 0 24 24"><path d="M12 3.2A8.8 8.8 0 1 1 3.4 10.4"/></svg></i>')
/** Files attached on this page and not on the note yet: local id -> { file, name, preview } (vm.files[id] says how
 *  the upload stands). This browser's own, drawn with the note. */
const pending = new Map()
/** One file of the note: a picture as a small thumbnail, any other file by its name. up: a file of this page on its way. */
function noteFile({ id, name, picture, src, up = null }) {
  const state = up?.state === 'failed' ? ' is-failed' : up && up.state !== 'ready' ? ' is-uploading' : ''
  return html`<button type="button" class="corner-note-file${picture ? ' is-pic' : ''}${state}${up ? ' is-pending' : ''}" id="note-file-${id}" data-file="${id}"${up ? raw(' data-action="corner-note#unclip"') : ''} title="${name}">${picture ? html`<img${src ? html` src="${src}"` : ''} alt="" loading="lazy">` : html`<span>${name}</span>`}${state === ' is-uploading' ? html`${RING}<b class="note-up-word">uploading…</b>` : state ? html`<b class="note-up-word">upload failed</b><span class="note-up-again" role="button" data-action="click->corner-note#retry">retry</span>` : html`<i>×</i>`}</button>`
}
// Ctrl+Enter sends the note (Cmd+Enter on a Mac), anywhere on it; the keys stand small under the envelope. (The
// envelope's hover says to whom, the app's own label (notes.css data-name), not the browser's tooltip, which would
// stand half outside the note.)
const MAC = /Mac|iPhone|iPad/.test(globalThis.navigator?.platform ?? '')
const SEND_KEYS = MAC ? 'Meta+Enter' : 'Control+Enter', SEND_WORD = MAC ? '⌘ Enter' : 'Ctrl+Enter'

export function cornerNote(vm, base) {
  const n = vm.notes ?? {}
  const note = (n.rows ?? []).find(x => x.id === n.corner) ?? null
  const files = [
    ...(note?.files ?? []).map(f => noteFile({ id: f.file_id, name: f.name, picture: f.picture, src: f.picture ? fileSrc(f.file_id) : null })),
    ...[...pending].map(([id, p]) => noteFile({ id, name: p.name, picture: Boolean(p.preview), src: p.preview, up: vm.files?.[id] ?? { state: 'uploading' } })),
  ]
  const to = n.to ?? null, who = to?.name || 'Main Session'
  const written = Boolean(n.written)
  return html`<section class="corner-note-box" id="corner-note-box" aria-label="Your note" data-controller="corner-note" data-corner-note-id-value="${note?.id ?? ''}" data-corner-note-text-value="${note?.text ?? ''}" data-corner-note-written-value="${String(written)}">
<button type="button" class="corner-note-head" data-action="corner-note#open" data-tip="${written ? 'Note (N)' : 'New note (N)'}" aria-label="${n.says?.en ?? 'New note'}" aria-expanded="false">${NOTE_ICON}</button>
<div class="corner-note-body" hidden data-action="paste->corner-note#paste dragover->corner-note#over dragleave->corner-note#out drop->corner-note#drop keydown->corner-note#key">${to ? html`<span class="corner-note-to">${avatar(whoOf(to), { crown: false })}<b>${who}</b></span>` : ''}<textarea class="corner-note-field" rows="2" aria-label="Your note" aria-keyshortcuts="${SEND_KEYS}" data-action="input->corner-note#typed"></textarea><div class="corner-note-files">${files}</div>
<footer class="corner-note-foot"><button type="button" class="corner-note-clip" data-action="corner-note#pick" title="Attach" aria-label="Attach a picture or a file">${CLIP}</button><button type="button" class="corner-note-bin" data-action="corner-note#bin" title="Throw away" aria-label="Throw the note away">${BIN}</button><span class="corner-note-park" data-tip="To the board" aria-hidden="true">${raw(sketchSvg('go'))}<b>onto the board</b></span><i></i>${to ? html`<span class="corner-note-sending"><button type="button" class="note-send corner-note-send" data-action="corner-note#send" data-to="${to.session_id}" data-name="${who}" aria-label="${n.send_says?.en ?? `Send to ${who}`}" aria-keyshortcuts="${SEND_KEYS}">${raw(crownSvg())}</button><kbd class="corner-note-keys" aria-hidden="true">${SEND_WORD}</kbd></span>` : html`<a class="corner-note-nocrown" data-nav href="${base}/settings/sessions">${n.no_crown?.en ?? 'Give a session the crown to send'}</a>`}</footer></div>
</section>`
}
/** The notes' routes (notes.mjs), from the corner note's controller. */
const postNote = (path, fields = {}) => fetch(path, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(fields) }).catch(err => console.warn('note', err))
const forget = id => { const p = pending.get(id); if (p?.preview) URL.revokeObjectURL(p.preview); pending.delete(id) }
controller('corner-note', class extends Controller {
  static values = { id: String, text: String, written: Boolean }
  get field() { return this.element.querySelector('.corner-note-field') }
  connect() {
    this.field.value = this.textValue
    this.paintWords()
    this.write = () => this.open()
    document.addEventListener('trommi:note', this.write)
    this.park()
  }
  disconnect() { document.removeEventListener('trommi:note', this.write); document.removeEventListener('pointerdown', this.away); clearTimeout(this.timer) }
  isOpen() { return this.element.classList.contains('is-open') }
  // (the core's words come in while the note is folded, or open with the keyboard elsewhere: another device wrote them)
  textValueChanged(v) { const f = this.field; if (!f || (this.isOpen() && document.activeElement === f) || f.value === v) return; f.value = v; this.paintWords() }
  writtenValueChanged() { this.paintWords() }
  // (a note made here: its id comes with the core's next render; what was typed meanwhile is kept with it)
  idValueChanged(v) { if (v && this.making) { this.making = false; if (this.field.value !== this.textValue) this.save() } }
  paintWords() { if (this.field) this.element.classList.toggle('has-words', Boolean(this.field.value.trim() || this.files().length)) }
  files() { return [...this.element.querySelectorAll('.corner-note-file:not(.is-failed)')] }
  // ---- park the note on the Scribble Board: there, the note can be dragged onto the board (mouse or finger) and
  // dropped; it stays where it was dropped as a sticky with its words, its pictures laid under it (whiteboard.mjs),
  // and the corner is empty again. Closed, the corner's sticky is what is dragged (a press without a drag opens the
  // note, as everywhere); open, the sheet is taken by its paper (its edge, its foot, the grip "onto the board"), never
  // by the words, which are selected as in any field. Files that are no pictures stay with the corner's note.
  pictures() { return [...this.element.querySelectorAll('.corner-note-file.is-pic:not(.is-uploading):not(.is-failed)')].map(c => ({ url: c.querySelector('img')?.src ?? '', name: c.title, file_id: c.dataset.file })).filter(p => p.url) }
  park() {
    const head = this.element.querySelector('.corner-note-head'), body = this.element.querySelector('.corner-note-body')
    let drag = null
    const ghostAt = e => { drag.ghost.style.left = `${e.clientX}px`; drag.ghost.style.top = `${e.clientY}px` }
    const down = e => {
      if (e.button > 0 || document.body.dataset.tView !== 'whiteboard' || !(this.field.value.trim() || this.pictures().length)) return
      if (e.currentTarget === body && e.target.closest('textarea, button, input, a')) return
      if (e.currentTarget === body) e.preventDefault()   // (no words are selected along the way)
      drag = { x: e.clientX, y: e.clientY, id: e.pointerId, ghost: null }
      e.currentTarget.setPointerCapture(e.pointerId)
    }
    const move = e => {
      if (!drag || e.pointerId !== drag.id) return
      if (!drag.ghost) {
        if (Math.hypot(e.clientX - drag.x, e.clientY - drag.y) < 8) return
        // (what is carried is the note as a conversation shows it: the paper with its strip of tape, notes.css .msg-note)
        const words = this.field.value.trim(), n = this.pictures().length
        drag.ghost = Object.assign(document.createElement('figure'), { className: 'msg-note corner-note-ghost' })
        drag.ghost.append(Object.assign(document.createElement('i'), { className: 'msg-note-tape' }), Object.assign(document.createElement('p'), { textContent: words || (n > 1 ? `${n} pictures` : 'A picture') }))
        document.body.append(drag.ghost)
        this.element.classList.add('is-parking')
      }
      ghostAt(e)
    }
    const end = e => {
      const d = drag; drag = null
      if (!d?.ghost) return
      d.ghost.remove()
      this.element.classList.remove('is-parking')
      this.skipOpen = true; setTimeout(() => { this.skipOpen = false }, 0)   // (the click that follows a drag opens nothing)
      if (e.type === 'pointerup' && Math.hypot(e.clientX - d.x, e.clientY - d.y) > 48) this.parked(e.clientX, e.clientY)
    }
    for (const from of [head, body]) { from.addEventListener('pointerdown', down); from.addEventListener('pointermove', move); from.addEventListener('pointerup', end); from.addEventListener('pointercancel', end) }
  }
  async parked(x, y) {
    // (the board answers in the event: taken, and placed: the promise of the pictures it could lay down, by their url)
    const detail = { text: this.field.value, pictures: this.pictures(), x, y, taken: false, placed: null }
    document.dispatchEvent(new CustomEvent('trommi:park-note', { detail }))
    if (!detail.taken) return
    this.field.value = ''
    const placed = new Set(await detail.placed ?? [])
    for (const p of detail.pictures) if (placed.has(p.url) && pending.has(p.file_id)) forget(p.file_id)
    if (this.isOpen()) this.close(); else { this.paintWords(); this.save() }
  }
  open() {
    if (this.skipOpen) return
    this.element.classList.add('is-open')
    this.element.querySelector('.corner-note-head').setAttribute('aria-expanded', 'true')
    this.element.querySelector('.corner-note-body').hidden = false
    this.fit(); this.field.focus(); this.field.setSelectionRange(this.field.value.length, this.field.value.length)
    this.away = e => { if (this.element.contains(e.target)) return; if (e.target.closest?.('.tab[data-tab="note"]')) this.element.dataset.tabClosed = '1'; this.close() }
    setTimeout(() => document.addEventListener('pointerdown', this.away), 0)
  }
  close(keep = true) {
    document.removeEventListener('pointerdown', this.away)
    this.element.classList.remove('is-open')
    this.element.querySelector('.corner-note-head').setAttribute('aria-expanded', 'false')
    this.element.querySelector('.corner-note-body').hidden = true
    this.paintWords()
    if (keep) this.save()
  }
  fit() { this.field.style.height = 'auto'; this.field.style.height = `${Math.min(this.field.scrollHeight + 2, 320)}px` }
  typed() { this.fit(); this.paintWords(); clearTimeout(this.timer); this.timer = setTimeout(() => this.save(), 600) }
  key(e) {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); this.send() }
    else if (e.key === 'Escape') { e.preventDefault(); this.close() }
  }
  // The words are kept as they stand: the note is made the first time there are words, gone when empty.
  save() {
    clearTimeout(this.timer)
    const text = this.field.value, files = [...pending.keys()].join(',')
    if (!this.idValue) {
      if (this.making || !(text.trim() || files)) return
      this.making = true
      setTimeout(() => { this.making = false }, 3000)
      return postNote('/notes', { text, files })
    }
    if (!text.trim() && !this.files().length) return postNote(`/notes/${this.idValue}/bin`)
    if (text !== this.textValue || files) return postNote(`/notes/${this.idValue}/keep`, { text, files })
  }
  /** The id of a note made here, once the core drew it (null after a while). */
  async made() {
    for (let i = 0; i < 60 && !this.idValue; i++) await new Promise(r => setTimeout(r, 50))
    return this.idValue || null
  }
  // ---- attachments: uploaded by the core (t.attach), drawn with the note while they go ----
  async attach(list) {
    const got = [...list].filter(f => f instanceof File)
    if (!got.length || !T) return
    for (const f of got) {
      const id = await T.attach(f)
      pending.set(id, { file: f, name: f.name || 'file', preview: /^image\//.test(f.type) ? URL.createObjectURL(f) : null })
    }
    this.save()
  }
  retry(e) { e.stopPropagation(); const id = e.currentTarget.closest('.corner-note-file')?.dataset.file, p = pending.get(id); if (!p) return; forget(id); this.attach([p.file]) }
  pick() {
    let input = this.element.querySelector('input[type=file]')
    if (!input) {
      input = Object.assign(document.createElement('input'), { type: 'file', multiple: true, hidden: true, tabIndex: -1 })
      input.addEventListener('change', async () => { await this.attach(input.files); input.value = ''; this.field.focus() })
      this.element.append(input)
    }
    input.click()
  }
  unclip(e) { const chip = e.currentTarget; forget(chip.dataset.file); chip.remove(); this.paintWords() }
  paste(e) { if (e.clipboardData?.files?.length) { e.preventDefault(); this.attach(e.clipboardData.files) } }
  over(e) { if (e.dataTransfer?.types?.includes('Files')) { e.preventDefault(); this.element.classList.add('is-drop') } }
  out() { this.element.classList.remove('is-drop') }
  drop(e) { this.out(); if (e.dataTransfer?.files?.length) { e.preventDefault(); this.attach(e.dataTransfer.files) } }
  async send() {
    const text = this.field.value
    if (!text.trim() && !this.files().length) return this.field.focus()
    const to = this.element.querySelector('.corner-note-send')?.dataset.to || null
    if (!this.idValue) this.save(); else if (text !== this.textValue || pending.size) await this.save()
    const id = await this.made()
    if (!id) return
    this.field.value = ''
    for (const k of [...pending.keys()]) forget(k)
    this.close(false)
    // (the core holds it a moment and says so in a toast with its Undo)
    await postNote(`/notes/${id}/send`, to ? { to } : {})
  }
  async bin() {
    const id = this.idValue, text = this.field.value
    this.field.value = ''
    for (const k of [...pending.keys()]) forget(k)
    this.close(false)
    if (!id) return
    await postNote(`/notes/${id}/bin`)
    if (text.trim()) showToast({ head: 'Note thrown away', line: text.trim().replace(/\s+/g, ' ').slice(0, 80), undo: () => postNote('/notes', { text }) })
  }
})

/** A new session of an agent's own machine that asks to join (self-join, spec/v1.md 12.4; vm.account.asks), as a row
 *  where sessions live: what the vouching agent says it is, with Allow and Deny on a line of their own under the words. */
export function joinRow(ask) {
  const button = (value, label, cls) => html`<form method="post" action="/vouch/${ask.nonce}" class="join-answer"><input type="hidden" name="allow" value="${value}"><button type="submit" class="${cls}">${label}</button></form>`
  const state = ask.joining ? 'joining' : 'ask', name = ask.name || ask.folder || 'New session'
  const line = ask.joining ? 'joining…' : `wants to join${ask.host ? ` · ${ask.host}` : ''}`
  return html`<div class="agent-row join-row" id="join-${ask.nonce}" data-state="${state}" title="${ask.says.en}"><span class="agent-entry"><span class="join-mark" aria-hidden="true">${PLUS}</span><span class="agent-text"><strong>${name}</strong><small class="join-line">${line}</small>${ask.vouched_by ? html`<small class="join-by">vouched by ${ask.vouched_by}</small>` : ''}</span></span>${ask.joining ? '' : html`<span class="join-acts" role="group" aria-label="${`Allow new ${ask.kind || 'agent'} session ${name}?`}">${button('yes', 'Allow', 'join-allow')}${button('no', 'Deny', 'join-deny')}</span>`}</div>`
}

/** The rows of #agents. A wide screen has two levels (SidebarView.level): every team as a stack (the first), or inside
 *  the team in view: the way back, its Desk and Scribble Board, its chats. A phone shows the chats of the team in view
 *  alone (sidebar.css). Then the self-join asks and New Agent…. */
export function sidebarRows(vm, base) {
  const sb = vm?.sidebar ?? {}, rows = sb.sessions ?? [], stacks = sb.stacks ?? []
  const level = stacks.length && sb.level !== 'team' ? 'teams' : 'team'
  const invite = html`${(vm?.account?.asks ?? []).map(joinRow)}${inviteAgentButton()}`
  const team = stacks.find(st => st.selected)
  const head = level === 'teams'
    ? html`<span class="side-level" data-level="teams" hidden></span>${stacks.map(st => teamStack(st, base))}`
    : html`<span class="side-level" data-level="team" hidden></span>${team ? teamInside(team, vm, base) : ''}`
  return html`${head}${rows.map(r => row(r, base))}${invite}${inDemo() ? raw('<span class="side-demo" id="side-demo">Demo · <a href="/screens?mock=1" target="_blank" rel="noopener" title="All screens">All screens</a> · <a href="/?mock=0" data-turbo="false" title="Leave demo">leave</a></span>') : ''}`
}

// ---- the wide sidebar's two levels (SidebarView.level, TeamStackView) ----
// First level: every team as a stack, its head (desk drawing, name, what waits) over a fan of its sessions' card
// edges, a little crooked, each in its session's colour. A click goes into the team: it becomes the team in view and
// the sidebar shows its chats (Focus.in_team, kept per device by app.mjs); "‹ Teams"
// goes back out. The bell at the top is home: the Desk, and the sidebar's first level.
function teamStack(st, base) {
  const name = st.name.en, edges = st.edges ?? []
  const cls = ['team-head', 'is-stack', st.selected && 'is-in-view', st.knocks && 'is-knock', !edges.length && 'is-empty'].filter(Boolean).join(' ')
  return html`<a class="${cls}" id="team-${st.id}" data-nav draggable="false" href="${base}/?team=${encodeURIComponent(st.id)}" data-team-in="${st.id}" style="--n:${edges.length}" title="${name}" aria-label="${name}: ${st.says.en}"><span class="team-go"><span class="team-lamp">${deskMark(st.waiting > 0)}</span><strong>${name}</strong>${st.waiting ? html`<i class="team-n" aria-hidden="true">${st.waiting}</i>` : ''}</span><span class="team-edges" aria-hidden="true">${edges.map((e, i) => { const q = edgeQuirk(e.id); return html`<i style="--i:${i};--hue:${whoOf(e).hue};--tilt:${q.tilt}deg;--dx:${q.dx}px">${raw(q.svg)}</i>` })}</span></a>`
}
const BACK = raw('<svg viewBox="0 0 24 24" class="sketch" aria-hidden="true"><path d="M14.6 6.2Q11.6 9.2 8.8 12.1Q11.7 14.9 14.4 17.9"/></svg>')
/** Inside a team: the way back to every team, the team's name, and its places (its Desk, its Scribble Board). */
function teamInside(st, vm, base) {
  const name = st.name.en, desk = location.pathname === `${base}/` || location.pathname === '/'
  return html`<div class="team-inside" id="team-inside-${st.id}"><button type="button" class="team-out" data-team-out aria-label="Back to all teams" title="Teams">${BACK}<span>Teams</span></button><h2 class="team-title">${name}</h2></div>
<a class="team-place-row" id="team-desk" data-nav draggable="false" href="${base}/"${desk ? raw(' aria-current="page"') : ''}><span class="team-lamp">${deskMark(st.waiting > 0)}</span><b>Desk</b>${st.waiting ? html`<i class="team-n${st.knocks ? ' is-knock' : ''}">${st.waiting}</i>` : ''}</a>
<a class="team-place-row" id="team-scribble" data-nav draggable="false" href="${base}/scribble-board">${sk('pen')}<b>Scribble</b></a>
<span class="team-chats-h" id="team-chats-h">Chats</span>`
}

/** The Desk box's drawing: the desk with its lamp lit while something waits on it (the open questions of the desk in
 *  view), dark when it is clear. */
const deskLamp = vm => deskMark((vm?.sidebar?.waiting ?? 0) > 0)


const $ = (sel, root = document) => root.querySelector(sel)

// ---- the sessions' order: a row is picked up and dragged up or down within its part (its desk, connected or
// not). A mouse lifts it as soon as it moves, a finger after a short hold (a swipe still
// scrolls). A ghost of the row follows the pointer; where it would land the row's place stays open, outlined in
// pencil, and the others make room. Escape puts it back. On a focused row Alt+↑/↓ moves it one place. Let go, and
// the new place goes to the core (PlaceSession { before | after }: the sessions' registers, on every device). ----
controller('order', class extends Controller {
  connect() {
    this.grab = e => this.down(e)
    this.keys = e => this.key(e)
    this.noClick = e => { if (this.dragged) { e.preventDefault(); e.stopImmediatePropagation(); this.dragged = false } }
    this.element.addEventListener('pointerdown', this.grab)
    this.element.addEventListener('keydown', this.keys)
    this.element.addEventListener('click', this.noClick, true)
  }
  disconnect() {
    this.cancel?.()
    this.element.removeEventListener('pointerdown', this.grab)
    this.element.removeEventListener('keydown', this.keys)
    this.element.removeEventListener('click', this.noClick, true)
  }
  /** The rows of one part, in the order they stand. */
  peers(row) { return [...this.element.querySelectorAll(`:scope > .agent-row[data-order="${CSS.escape(row.dataset.order)}"]`)] }
  /** The rows that move with a row: itself. */
  block(row) { return [row] }
  /** Puts a block directly before `ref` (null: after the last row of `last`'s block), the others sliding into place. */
  place(rows, ref, peers) {
    const was = new Map(peers.map(p => [p, p.getBoundingClientRect().top]))
    const anchor = ref ?? this.block(peers.at(-1)).at(-1).nextSibling
    for (const r of rows) this.element.insertBefore(r, anchor)
    for (const [p, top] of was) { const dy = top - p.getBoundingClientRect().top; if (dy && !p.classList.contains('is-carried')) p.animate?.([{ translate: `0 ${dy}px` }, { translate: '0 0' }], { duration: 140, easing: 'ease-out' }) }
  }
  /** Where the row stands now among its peers: { before } the next one, or { after } the one above, or null alone. */
  spot(row) { const p = this.peers(row), i = p.indexOf(row); return p[i + 1] ? { before: p[i + 1].dataset.unit } : p[i - 1] ? { after: p[i - 1].dataset.unit } : null }
  /** Sends the row's new place (PlaceSession) and puts the rows back as they were painted (kids): the core's next
   *  render draws the new order, and the page's morph finds its rows where it left them. */
  save(row, kids) {
    const at = this.spot(row), id = row.dataset.unit
    for (const k of kids) this.element.append(k)
    if (at) T?.act({ do: 'place_session', session_id: id, ...at })
  }

  key(e) {
    if (!e.altKey || e.ctrlKey || e.metaKey || !['ArrowUp', 'ArrowDown'].includes(e.key)) return
    const row = e.target.closest?.('.agent-row[data-order]')
    if (!row || row.parentElement !== this.element) return
    e.preventDefault()
    const peers = this.peers(row), i = peers.indexOf(row), j = e.key === 'ArrowUp' ? i - 1 : i + 1
    if (j < 0 || j >= peers.length) return
    const kids = [...this.element.children]
    this.place(this.block(row), e.key === 'ArrowUp' ? peers[j] : peers[j + 1] ?? null, peers)
    this.save(row, kids)
    // (the core's render draws the sidebar in its new order: the keyboard stays on the row)
    const id = row.dataset.unit, until = Date.now() + 2000
    const back = () => { const entry = document.getElementById(`agent-${id}`)?.querySelector(':scope > .agent-entry'); if (entry && document.activeElement !== entry) entry.focus(); if (Date.now() < until) requestAnimationFrame(back) }
    requestAnimationFrame(back)
  }

  down(event) {
    const row = event.target.closest?.('.agent-row[data-order]')
    if (!row || row.parentElement !== this.element || event.button > 0 || event.target.closest('button, .agent-badge')) return
    if (document.documentElement.dataset.rail === 'folded' || this.peers(row).length < 2) return
    const id = event.pointerId, touch = event.pointerType !== 'mouse', x0 = event.clientX, y0 = event.clientY
    let live = false, timer = 0, ghost = null, grabAt = 0, home = null, start = '', kids = []
    const rows = this.block(row)
    const lift = () => {
      live = true
      try { row.setPointerCapture(id) } catch {}
      start = this.peers(row).map(r => r.dataset.unit).join()
      kids = [...this.element.children]
      home = rows.at(-1).nextSibling
      const box = row.getBoundingClientRect()
      grabAt = y0 - box.top
      ghost = row.cloneNode(true)
      for (const n of [ghost, ...ghost.querySelectorAll('[id], [data-controller], [data-action]')]) { n.removeAttribute('id'); n.removeAttribute('data-controller'); n.removeAttribute('data-action') }
      delete ghost.dataset.unit; delete ghost.dataset.order
      ghost.classList.add('order-ghost'); ghost.classList.remove('is-active', 'has-active')
      ghost.setAttribute('aria-hidden', 'true'); ghost.inert = true
      Object.assign(ghost.style, { left: `${box.left}px`, top: `${box.top}px`, width: `${box.width}px`, height: `${box.height}px` })
      this.element.append(ghost)
      row.classList.add('is-carried')
      for (const r of rows.slice(1)) r.classList.add('is-carried-sub')
      this.element.classList.add('is-ordering')
      if (touch) navigator.vibrate?.(8)
    }
    const move = e => {
      if (e.pointerId !== id) return
      if (!live) {
        if (touch) { if (Math.hypot(e.clientX - x0, e.clientY - y0) > 8) end(e, true); return }
        if (Math.abs(e.clientY - y0) < 5) return
        lift()
      }
      e.preventDefault()
      if (!row.isConnected) return end(e, true)   // (the sidebar was drawn anew under the pointer)
      ghost.style.top = `${e.clientY - grabAt}px`
      const peers = this.peers(row), others = peers.filter(p => p !== row)
      const next = others.find(p => { const b = p.getBoundingClientRect(); return e.clientY < b.top + b.height / 2 }) ?? null
      const now = peers[peers.indexOf(row) + 1] ?? null
      if (next !== now) this.place(rows, next, peers)
    }
    const stopScroll = e => { if (live) e.preventDefault() }
    const esc = e => { if (e.key === 'Escape' && live) { e.preventDefault(); e.stopPropagation(); end({ pointerId: id, type: 'escape' }, true) } }
    const end = (e, cancel = false) => {
      if (e.pointerId !== id) return
      clearTimeout(timer)
      removeEventListener('pointermove', move); removeEventListener('pointerup', end); removeEventListener('pointercancel', end)
      removeEventListener('touchmove', stopScroll); removeEventListener('keydown', esc, true)
      this.cancel = null
      if (!live) return
      this.dragged = e.type !== 'escape'; setTimeout(() => { this.dragged = false }, 0)
      cancel ||= e.type === 'pointercancel'
      if (cancel && row.isConnected) for (const r of rows) this.element.insertBefore(r, home?.parentNode === this.element ? home : null)
      ghost?.remove()
      row.classList.remove('is-carried')
      for (const r of rows.slice(1)) r.classList.remove('is-carried-sub')
      this.element.classList.remove('is-ordering')
      if (!cancel && row.isConnected && this.peers(row).map(r => r.dataset.unit).join() !== start) this.save(row, kids)
    }
    if (touch) timer = setTimeout(lift, 300)
    this.cancel = () => end({ pointerId: id, type: 'escape' }, true)
    addEventListener('pointermove', move, { passive: false }); addEventListener('pointerup', end); addEventListener('pointercancel', end)
    addEventListener('touchmove', stopScroll, { passive: false }); addEventListener('keydown', esc, true)
  }
})

// ---- menu ----
// The Trommi menu (what opens from the row at the sidebar's foot; on a phone the sidebar is a drawer;
// the menu itself has no search field), and the sheet a long press on a Desk row brings up on a phone. app.css and sidebar.css style the
// menu; the controller "menu" adds the arrows and a new team, the Desk's row menu (desk.mjs) the long press. Opening and closing the
// menu and the theme: ui.mjs.

/** Makes a team (AddTeam) and gives it once the core drew it: its id is the one the core's toast "Team added" names
 *  (its Undo is RemoveTeam of it); null after a while. */
async function addTeam(name) {
  const had = new Set(T.vm().sidebar.teams.map(d => d.id))
  T.act({ do: 'add_team', name })
  for (let i = 0; i < 60; i++) {
    await new Promise(r => setTimeout(r, 50))
    const vm = T.vm(), id = (vm.toasts ?? []).map(x => x.action).find(a => a?.do === 'remove_team' && !had.has(a.team_id))?.team_id
    const made = id && vm.sidebar.teams.find(d => d.id === id)
    if (made) return made
  }
  return null
}

/** The newest day of What's new (app.mjs; null until it is loaded). Taken when the frame is wired: app.mjs imports
 *  this module, and this one draws in Node's tests without the app. */
let newsLatestDay = () => null
/** Whether the newest day of What's new was opened on this device (news.mjs remembers it; the row's dot until then). */
const newsSeen = () => { const latest = newsLatestDay(); if (latest == null) return true; try { return localStorage.getItem('trommi-news-seen') === latest } catch { return true } }
// (the newest day arrives after the first page was drawn: the dot then shows where it is unseen)
globalThis.document?.addEventListener('trommi:news', () => { const dot = document.getElementById('menu-news-dot'); if (dot) dot.hidden = newsSeen() })
/** Whether this tab shows the demo room (app.mjs: ?mock=1, remembered for the tab; ?mock=0 ends it). */
const inDemo = () => { try { return Boolean(globalThis.sessionStorage?.getItem('trommi-mock')) } catch { return false } }
// Small drawings of the menu's own, in the pen's line: the plus of "New team", and the light of the desk lamp.
const NEW_TEAM = raw('<svg viewBox="0 0 24 24" class="sketch" aria-hidden="true" style="rotate:3deg"><path d="M12.2 5.2Q11.8 12 12 18.8"/><path d="M5.3 12.3Q12 11.7 18.7 12.1"/></svg>')
// The lamp switched on, drawn under the desk's lines: the shade glowing, a soft cone of light down onto the top, three short rays.
const LIGHT = '<g class="lamp-light"><path class="lamp-glow" d="M14.9 2.4Q11.7 3.7 10.7 4.7Q9.6 5.7 9 6.6Q8.5 7.4 9 7.6Q9.5 7.9 12.4 7Q15.2 6.1 15.6 6.1Q15.9 6.1 15.8 4.7Q15.7 3.4 14.9 2.4Z"/><path class="lamp-cone" d="M9.2 7.9Q12.4 7.1 15.6 6.3L17.3 11.9Q12 12.1 6.4 12.2Z"/><path d="M7.6 9Q5.9 10.1 4.3 11.2"/><path d="M7.1 7.2Q5.3 7.3 3.5 7.5"/><path d="M7.9 5Q6.4 4.2 4.9 3.5"/></g>'
/** The desk drawing of the menu's team rows and the Desk box: its lamp lit while something waits on that team. */
const deskMark = lit => raw(lit ? sketchSvg('desk', 'menu-lamp is-lit').replace(/(<svg[^>]*>)/, `$1${LIGHT}`) : sketchSvg('desk', 'menu-lamp'))
/** The team rows of the menu (vm.sidebar.teams): the team in view checked (the marked row), what waits on each as its
 *  small number; a team's lamp is lit while something waits on it. */
function menuTeamRows(vm, base) {
  // (a row: the link to the team, and beside it the pencil that renames it: menu#rename puts a field in the name's place)
  return html`<span class="menu-team-rows" id="menu-team-rows">${vm.sidebar.teams.map(d => html`<span class="menu-team-row" data-team="${d.id}"><a role="menuitemradio" class="menu-team" data-nav draggable="false" href="${base}/?team=${d.id}" data-team="${d.id}" aria-checked="${String(d.selected)}">${deskMark(d.waiting > 0)}<b>${d.name.en}</b>${d.waiting ? html`<i class="menu-n">${d.waiting}</i>` : ''}</a><button type="button" class="menu-team-pen" data-action="click->menu#rename" data-menu-id-param="${d.id}" title="Rename ${d.name.en}" aria-label="Rename the team ${d.name.en}">${sk('pen')}</button></span>`)}</span>`
}

const NOTE_EMPTY = raw('<svg viewBox="0 0 24 24" class="sketch" aria-hidden="true"><path d="M5.4 4.6Q12 4.2 18.8 4.7Q19.3 10 19.1 14.6L14.4 19.4Q9.8 19.7 5.2 19.3Q4.8 12 5.4 4.6Z"/><path d="M19.1 14.6Q15.6 14.3 14.6 15Q14.3 17 14.4 19.4"/></svg>'), NOTE_WRITTEN = raw('<svg viewBox="0 0 24 24" class="sketch" aria-hidden="true"><path d="M5.4 4.6Q12 4.2 18.8 4.7Q19.3 10 19.1 14.6L14.4 19.4Q9.8 19.7 5.2 19.3Q4.8 12 5.4 4.6Z"/><path d="M19.1 14.6Q15.6 14.3 14.6 15Q14.3 17 14.4 19.4"/><path d="M8 8.6Q12 8.2 15.8 8.5"/><path d="M8 11.7Q11.6 11.4 15.6 11.6"/><path d="M8 14.8Q10 14.6 11.8 14.8"/></svg>')
const noteGlyph = vm => html`<span class="tab-note-ico" id="tab-note-ico">${vm.notes?.written ? NOTE_WRITTEN : NOTE_EMPTY}</span>`
/** The phone's bar ("Menu" is no tab: the menu is the pill at the top left):
 *  a small glass capsule at the foot, centred: Chat · Desk · Note. The Desk carries what waits as its number (red
 *  while something knocks); Note opens the corner note to the crowned session. On a card's own page it stands back. */
export function tabBar(vm, base, view) {
  const sb = vm.sidebar, chat = lastChat(vm)
  // (monochrome pen drawings; Chat opens the last chat, Desk carries what waits, Note opens the note; inside a chat or
  // a card the capsule stands back)
  // (in a chat the capsule stays; the composer waits folded into a round glass pen beside it)
  const pen = view === 'session' ? html`<button type="button" class="compose-fab" id="compose-fab" aria-label="Write a message" title="Write a message">${sk('pen')}</button>` : ''
  return html`${pen}<nav class="tabbar" id="tabbar" aria-label="Chat, Desk, Note"${['card', 'picture', 'whiteboard'].includes(view) ? raw(' hidden') : ''}>
<a class="tab" data-tab="chat" data-nav draggable="false" href="${chat ? `${base}/chat/${encodeURIComponent(chat)}` : `${base}/chats`}"${view === 'chats' ? raw(' aria-current="page"') : ''}>${sk('bubble')}<span>Chat</span><i class="tab-badge" id="chat-badge" hidden></i></a>
<a class="tab" data-tab="desk" data-nav draggable="false" href="${base}/"${view === 'desk' ? raw(' aria-current="page"') : ''}>${sk('desk')}<span>Desk</span>${waitingBadge(sb.badge, sb.knocking > 0)}</a>
<button type="button" class="tab" data-tab="note" aria-controls="corner-note-box" aria-expanded="false">${noteGlyph(vm)}<span>Note</span></button>
</nav>`
}
const waitingBadge = (n, knocks) => html`<i class="tab-badge${knocks ? ' is-knock' : ''}" id="tab-badge"${n ? '' : raw(' hidden')}>${n > 99 ? '99+' : n}</i>`

const PUSH_WORDS = { all: 'Yes', knocking: 'Only knocking', off: 'No' }
// A gear in the pen's line (Settings).
const GEAR = raw('<svg viewBox="0 0 24 24" class="sketch" aria-hidden="true"><path d="M12 3.2L13.4 5.6L16.1 4.9L16.8 7.6L19.4 8.4L18.6 11.1L20.6 13L18.5 14.8L19.1 17.5L16.4 18.1L15.4 20.7L12.9 19.6L10.6 21.1L9.3 18.6L6.5 18.8L6.4 16L4 14.6L5.5 12.2L4.4 9.6L7 8.6L7.4 5.8L10.2 6.1Z"/><path d="M12.1 9.3Q14.7 9.6 14.8 12Q14.6 14.6 12 14.7Q9.4 14.5 9.3 12Q9.5 9.4 12.1 9.3Z"/></svg>')
/** The Trommi menu (#brand-doors): the wide screen's (from the sidebar's foot) and the phone's (from the pill), one
 *  component: the teams (each with its drawing and what waits, New Team…), Settings, and one row of small buttons for
 *  this device: Push (Off → All → Only knocking), Theme (Light → Dark → System), Keyboard Shortcuts, Demo, What's new
 *  (news.mjs, a dot while its newest day is unseen here), and Log out
 *  (/logout, the same page that asks first as Settings · Account's Log Out). A phone also has its places here
 *  (Scribble, Artifacts). */
function menuDoors(vm, base) {
  const push = vm.settings?.push ?? null, level = push?.level ?? 'off'
  return html`<nav class="sidedoors" id="brand-doors" role="menu" aria-label="Teams and settings" data-controller="menu" data-menu-team-value="${base}/" data-action="keydown->menu#walk click->menu#chosen" data-owns-keys hidden>
<div class="menu-teams" id="menu-teams">${menuTeamRows(vm, base)}
<button type="button" role="menuitem" class="menu-team-add is-plus" id="team-add" data-action="click->menu#newTeam" aria-label="New Team…" title="New Team…">${NEW_TEAM}</button>
<form class="menu-team-form" id="team-new" data-menu-target="teamForm" data-action="submit->menu#makeTeam" hidden><input class="menu-team-field" data-menu-target="teamName" data-action="keydown->menu#teamKey" maxlength="40" placeholder="Name of the new team" aria-label="Name of the new team" autocomplete="off"><button type="submit">Add</button></form>
<p class="menu-team-error" data-menu-target="teamError" role="alert"></p></div>
<a role="menuitem" class="menu-settings" href="${base}/settings" data-nav draggable="false" id="menu-settings">${GEAR}<span>Settings</span></a>
<button role="menuitemcheckbox" type="button" id="demo-toggle" class="menu-demo-row demo-toggle" aria-checked="${String(inDemo())}" title="${inDemo() ? 'Leave demo' : 'Demo'}">${sk('play')}<span>Demo</span><i class="demo-switch" aria-hidden="true"><b></b></i></button>
<a role="menuitem" class="menu-news-row" href="${base}/whats-new" data-nav draggable="false" id="menu-news">${sk('spark')}<span>What's new</span><i class="menu-news-dot" id="menu-news-dot" title="New"${newsSeen() ? raw(' hidden') : ''}></i></a>
<a role="menuitem" class="menu-logout-row" href="${base}/logout" data-nav draggable="false" id="menu-logout">${sk('logout')}<span>Log out</span></a>
<div class="menu-places">${[['scribble-board', 'pen', 'Scribble'], ['artifacts', 'picture', 'Artifacts']].map(([p, icon, word]) => html`<a role="menuitem" href="${base}/${p}" data-nav draggable="false">${sk(icon)}<span>${word}</span></a>`)}</div>
<div class="menu-icons" role="group" aria-label="This device">
<button role="menuitem" type="button" id="push-toggle" class="menu-ico" data-level="${level}" aria-checked="${String(level !== 'off')}"${push?.busy ? raw(' aria-busy="true"') : ''} aria-label="Push on this device: ${PUSH_WORDS[level]}" title="Push: ${PUSH_WORDS[level]}">${sk('bell')}${raw('<svg class="push-mark push-off" viewBox="0 0 24 24" aria-hidden="true"><path d="M4.2 3.8Q12 12.2 19.9 20.4"/></svg><svg class="push-mark push-knock" viewBox="0 0 24 24" aria-hidden="true"><path d="M12 7.6Q11.9 10.4 12.1 12.9"/><path d="M12 15.6Q12.1 15.8 12 16"/></svg>')}</button>
<button role="menuitem" type="button" id="theme-toggle" class="menu-ico menu-theme-row" aria-label="Theme: Light, Dark or System (T)" title="Theme (T)">${raw(sketchSvg('moon', 'ico-moon'))}${raw(sketchSvg('sun', 'ico-sun'))}<i class="ico-auto" aria-hidden="true">A</i></button>
<button role="menuitem" type="button" id="keys-open" class="menu-ico" data-action="click->menu#keys" aria-haspopup="dialog" aria-keyshortcuts="?" aria-label="Keyboard Shortcuts (?)" title="Shortcuts (?)">${sk('question')}</button>

</div>
<p class="menu-push-note" id="menu-push-note" role="status" hidden></p>
</nav>`
}

// ---- the sidebar's frame: the Desk box at its top with the Trommi menu's button, and its foot ----
// The foot (wide screens; sidebar.css): the ground under the sessions, which scroll above it. The menu's button stands
// on its left (a child of the Desk box's header), the
// button that folds the sidebar to a rail at its right end, and left of it the theme switch (Light, Dark, System:
// on a wide window this is its one place; a phone has no foot and keeps the switch in the Trommi menu).
export const SIDE_FOOT = raw(`<div class="side-foot"><button type="button" class="side-theme" aria-label="Theme: Light, Dark or System (T)" title="Theme (T)">${sketchSvg('moon', 'ico-moon')}${sketchSvg('sun', 'ico-sun')}<i class="ico-auto" aria-hidden="true">A</i></button><button type="button" class="rail-fold" data-controller="rail" data-action="click->rail#toggle pointerover@document->rail#tip focusin@document->rail#tip focusout@document->rail#untip turbo:before-cache@document->rail#untip" title="Fold ( [ )" aria-label="Fold the sidebar to a rail ( [ )" aria-pressed="false"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5.3 4.4Q4.8 11.6 5.4 19.7"/><path d="M15.1 6.1Q12.2 9.2 9.1 12.1Q12.1 14.7 14.8 18"/></svg></button></div><div class="side-resize" data-controller="side-resize" data-action="pointerdown->side-resize#down keydown->side-resize#key dblclick->side-resize#flip" role="separator" aria-orientation="vertical" aria-label="Sidebar width: drag, or the arrow keys" tabindex="0"></div>`)

/** The team's name on the rail's tag: two short lines at most, whole words (a word too long is cut by its line). */
function tagLines(name) {
  const words = String(name).trim().split(/\s+/), lines = []
  while (words.length && lines.length < 2) { let line = words.shift(); while (words.length && (line + ' ' + words[0]).length <= 6) line += ` ${words.shift()}`; lines.push(line) }
  if (words.length) lines[1] += '…'
  return lines
}
/** How big the team's name may stand in the Desk box: s (as "Desk"), m (a little smaller), l (two smaller lines). */
const nameSize = name => { const n = [...String(name)].length; return n <= 6 ? 's' : n <= 11 ? 'm' : 'l' }
/** The head's words: the place's name (vm.sidebar.place: the team's name) and the chevron that opens the team
 *  switch, the Trommi menu with the teams at its top (always there, so New team is reachable with one team too). */
const placeName = vm => vm?.sidebar?.place?.en || 'Personal'
/** A session by its key or its slug: its sidebar row, else its page (SessionView). */
const sessionOf = (vm, id) => (id == null ? null : vm.sidebar.sessions.find(r => r.id === id || r.slug === id) ?? Object.values(vm.sessions ?? {}).find(s => s.id === id || s.slug === id) ?? null)
/** The pill's words (the menu's button on a wide screen): the session in view with its drawing, else the desk drawing
 *  and the team's name. */
const pillPlace = (vm, s = null) => (s ? html`<span class="pill-place" id="pill-place">${avatar(whoOf(s), { crown: false })}<b>${s.name.en}</b></span>` : html`<span class="pill-place" id="pill-place"><span class="desk-lamp">${deskLamp(vm)}</span><b>${placeName(vm)}</b></span>`)
const teamPlace = vm => html`<span class="team-place" id="team-place"><span class="team-name" data-size="${nameSize(placeName(vm))}">${placeName(vm)}</span></span>`
const teamChevron = html`<button type="button" class="team-switch-open" aria-haspopup="menu" aria-expanded="false" aria-controls="brand-doors" title="Teams and settings" aria-label="Switch team, settings">${sk('unfold')}</button>`
/** The top of the sidebar: the Desk box, the Trommi menu's button and the menu (the team switch), the rail's tag.
 *  isDesk: the Desk is the page; session: the session in view (its key or slug). */
export function topbar(vm, base, isDesk, session = null) {
  const s = sessionOf(vm, session), place = placeName(vm)
  return html`<header class="topbar"><div class="brand">
<h1 class="deskpill"><a href="${base}/" data-nav draggable="false" class="desk-go" id="desk-go" title="${place}"${isDesk ? raw(' aria-current=""') : ''}><span class="desk-lamp" id="desk-lamp">${deskLamp(vm)}</span>${BELL}${teamPlace(vm)}</a>${teamChevron}
<button type="button" class="brand-open" id="brand-menu"${s ? html` data-session="${s.id}"` : ''} aria-haspopup="menu" aria-expanded="false" aria-controls="brand-doors" aria-label="Where you are: ${place}. Menu: teams, places, settings" title="Menu (Ctrl K)">${raw(String(BELL).replace('class="brand-mark"', 'class="brand-mark open-mark"'))}<span class="open-me" aria-hidden="true"><svg viewBox="0 0 24 24"><path d="M12.1 6.3Q14.6 6.4 14.7 9Q14.5 11.6 12 11.7Q9.5 11.5 9.4 9Q9.5 6.4 12.1 6.3Z"/><path d="M6.6 18.6Q7.6 14.2 12 14.1Q16.4 14.2 17.4 18.6"/></svg></span><b class="open-word">Trommi</b><b class="open-word-wide">Settings</b>${pillPlace(vm, s)}<span class="conn open-conn" id="conn" data-state="connecting" role="status"><i aria-hidden="true"></i><span id="conn-text" class="tc-sr">Connecting</span></span><span class="brand-fold">${sk('unfold')}</span></button>${inDemo() ? html`<a class="pill-demo" href="${base}/screens?mock=1" target="_blank" rel="noopener" title="Demo">Demo · All screens</a>` : ''}</h1>
<div class="side-top"><a class="side-home" href="${base}/" data-nav draggable="false" title="Desk" aria-label="Home: the Desk and every team">${BELL}</a><span class="side-tools"><a class="side-tool" href="${base}/stacks/off" data-nav draggable="false" title="Search" aria-label="Search">${sk('search')}</a><form method="post" action="/pair" class="side-tool-form"><input type="hidden" name="role" value="computer"><button type="submit" class="side-tool" title="New agent" aria-label="Approve a computer: a new agent">${PLUS}</button></form></span></div>
${menuDoors(vm, base)}
<button type="button" class="rail-tag" aria-haspopup="menu" aria-controls="brand-doors" title="Switch team" aria-label="Team ${place}: switch teams"><span class="rail-tag-string" aria-hidden="true"></span><span class="rail-tag-paper"><b>${tagLines(place).map(l => html`<span>${l}</span>`)}</b></span></button>
</div></header>`
}

// The phone's menu: the pill opens it, a choice or a tap beside closes it; New Team… makes a team (AddTeam, as the
// wide screen's menu); All Sessions… the whole list as a sheet from below.
controller('phone-menu', class extends Controller {
  connect() {
    this.pill = document.getElementById('phone-pill')
    this.onPill = e => { e.stopPropagation(); this.toggle() }
    this.pill?.addEventListener('click', this.onPill)
    this.beside = e => { if (!this.element.hidden && !this.element.contains(e.target) && e.target !== this.pill && !this.pill?.contains(e.target)) this.close() }
    document.addEventListener('click', this.beside, true)
    this.keys = e => { if (e.key === 'Escape' && !this.element.hidden) this.close() }
    document.addEventListener('keydown', this.keys)
    this.element.addEventListener('click', e => { if (e.target.closest('a[href]')) this.close() })
  }
  disconnect() { this.pill?.removeEventListener('click', this.onPill); document.removeEventListener('click', this.beside, true); document.removeEventListener('keydown', this.keys) }
  toggle() { if (this.element.hidden) this.open(); else this.close() }
  open() { this.element.hidden = false; this.pill?.setAttribute('aria-expanded', 'true'); requestAnimationFrame(() => this.element.classList.add('is-open')) }
  close() { this.element.classList.remove('is-open'); this.pill?.setAttribute('aria-expanded', 'false'); this.element.hidden = true; const f = this.element.querySelector('.pm-team-form'); if (f) f.hidden = true }
  newTeam() { const f = this.element.querySelector('.pm-team-form'); f.hidden = false; f.querySelector('input').focus() }
  async makeTeam(e) {
    e.preventDefault()
    const f = e.currentTarget, name = f.querySelector('input').value.trim(), err = f.querySelector('.pm-error')
    if (!name) return
    const made = await addTeam(name)
    if (!made) { err.textContent = 'Not made: the team did not come.'; return }
    this.close()
    window.trommi?.router?.visit(`/?team=${encodeURIComponent(made.id)}`)
  }
})
// After a team picked in the menu the page is drawn anew: the menu opens again where it was.
if (typeof document !== 'undefined') document.addEventListener('turbo:load', () => {
  let keep = null
  try { keep = sessionStorage.getItem('trommi-menu-keep'); sessionStorage.removeItem('trommi-menu-keep') } catch {}
  if (!keep) return
  requestAnimationFrame(() => { const doors = document.getElementById('brand-doors'); if (!doors) return; doors.hidden = false; for (const b of document.querySelectorAll('#brand-menu, .team-switch-open, #desk-pill')) if (b.offsetParent || b.id === 'brand-menu') b.setAttribute('aria-expanded', 'true') })
})
// The phone's pill opens the Trommi menu (#brand-doors), the same as the sidebar's foot.
if (typeof document !== 'undefined') document.addEventListener('click', e => {
  const pill = e.target instanceof Element ? e.target.closest('#desk-pill') : null
  if (!pill) return
  e.stopImmediatePropagation()
  document.getElementById('brand-menu')?.click()
  requestAnimationFrame(() => pill.setAttribute('aria-expanded', String(document.getElementById('brand-doors')?.hidden === false)))
})
// A phone's chat: the pen opens the composer and gives it the keyboard; leaving it folds it again (a draft marks the pen).
if (typeof document !== 'undefined') {
  const root = document.documentElement, field = () => document.querySelector('#session .session-compose textarea')
  document.addEventListener('click', e => {
    if (!(e.target instanceof Element) || !e.target.closest('#compose-fab')) return
    root.dataset.compose = ''
    requestAnimationFrame(() => field()?.focus())
  })
  document.addEventListener('focusout', e => {
    if (!(e.target instanceof Element) || !e.target.closest('#session .session-compose')) return
    setTimeout(() => {
      if (document.activeElement?.closest?.('#session .session-compose')) return
      delete root.dataset.compose
      document.getElementById('compose-fab')?.toggleAttribute('data-draft', Boolean(field()?.value.trim()))
    }, 120)
  })
  document.addEventListener('turbo:load', () => { delete root.dataset.compose; document.getElementById('compose-fab')?.toggleAttribute('data-draft', Boolean(field()?.value.trim())) })
}
// The capsule stands back while the keyboard is up (a phone: a text field has the focus).
if (typeof document !== 'undefined') {
  const field = n => n instanceof Element && n.matches('input:not([type=checkbox]):not([type=radio]):not([type=button]):not([type=submit]), textarea, [contenteditable=""], [contenteditable="true"]')
  document.addEventListener('focusin', e => { if (field(e.target)) document.documentElement.dataset.keyboard = '' })
  document.addEventListener('focusout', e => { if (field(e.target)) delete document.documentElement.dataset.keyboard })
}

// ---- a phone's top line and the drawer (sidebar.css "A phone") ----
// A phone has no room for the sidebar beside the page: the same sidebar (the Desk box, the sessions, the foot with the
// Trommi menu) is a drawer that slides in from the left over the page. The slim line at the top holds its handle (three
// pen lines; a red dot while a session is stopped or a card knocks) and the name of the place in view: the team, or
// the session with its drawing. Wide screens show neither.
const PLACES = { off: 'Off your mind', agents: 'Settings', room: 'Settings', artifacts: 'Artifacts', whiteboard: 'Scribble Board' }
/** In a chat (a phone): its title, the drawing and the name with ▾, opens the sessions of its team to switch to. */
function chatSwitch(vm, base, s) {
  const mates = vm.sidebar.sessions.filter(r => r.team_id === s.team_id)
  const team = vm.sidebar.teams.find(d => d.id === s.team_id)
  return html`<button type="button" class="phone-pill chat-title" id="phone-pill" aria-haspopup="menu" aria-controls="phone-menu" aria-expanded="false" aria-label="${s.name.en}: switch to another agent">${avatar(whoOf(s), { crown: false })}<b>${s.name.en}</b>${sk('unfold')}</button>
<div class="phone-menu" id="phone-menu" role="menu" aria-label="Agents of this team" data-controller="phone-menu" hidden><p class="pm-h">${team?.name.en ?? 'This team'}</p>${mates.map(a => html`<a role="menuitem" class="pm-row" data-nav draggable="false" href="${base}/chat/${encodeURIComponent(a.slug)}"${a.id === s.id ? raw(' aria-current="true"') : ''}>${avatar(whoOf(a), { crown: false })}<b>${a.name.en}</b>${a.open ? html`<i class="pm-n">${a.open}</i>` : ''}${a.id === s.id ? html`<span class="pm-check">${sk('tick')}</span>` : ''}</a>`)}</div>`
}

// The chats (a phone's Chat tab): the last chat opened (the crowned session's at first).
const CHAT_KEY = 'trommi-last-chat'
export function chatOpened(slug) { try { localStorage.setItem(CHAT_KEY, slug) } catch {} }
const lastChat = vm => { let id = null; try { id = localStorage.getItem(CHAT_KEY) } catch {} ; return sessionOf(vm, id)?.slug ?? vm.sidebar.sessions.find(r => r.crowned)?.slug ?? vm.sidebar.sessions[0]?.slug ?? null }
/** The page of chats: the sessions of the team in view, as the sidebar has them. */
export function chatsMain(vm, base) {
  return html`<main id="chats" class="chats-page" aria-label="Chats"><h1 class="chats-h">Chats</h1><ul class="chats-list">${vm.sidebar.sessions.map(a => html`<li id="chat-${a.id}"><a class="chats-row" data-nav draggable="false" href="${base}/chat/${encodeURIComponent(a.slug)}">${avatar(whoOf(a), { crown: false })}<span class="chats-words"><b>${a.name.en}</b><small>${a.cap?.word.en ?? ''}</small></span><span class="chats-side">${a.open ? html`<i class="pm-n">${a.open}</i>` : ''}</span></a></li>`)}</ul></main>`
}

/** The phone's top line: in a chat, back to the chats and its title (current: the session, its key or slug);
 *  elsewhere the pill with the desk drawing and the team's name (the Trommi menu), and the place's name. */
export function phoneBar(vm, base, { view = '', current = null, title = '' } = {}) {
  const waits = vm.sidebar.stopped + vm.sidebar.knocking > 0
  const s = view === 'session' ? sessionOf(vm, current) : null
  if (s) return html`<div class="phone-bar is-chat" id="phone-bar"><a class="phone-back" data-nav draggable="false" href="${base}/chats" aria-label="Chats">${raw('<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M14.8 5.6Q11 9 7.6 12.1Q11 15.4 14.9 18.6"/></svg>')}</a>${chatSwitch(vm, base, s)}</div>`
  const place = ['desk', 'card', 'picture', 'chats'].includes(view) ? ''
    : html`<span class="phone-place"><b>${PLACES[view] ?? String(title).replace(/^\(\d+\) /, '').replace(/ · Trommi$/, '')}</b></span>`
  return html`<div class="phone-bar" id="phone-bar"><button type="button" class="phone-pill" id="desk-pill" data-opens="brand-doors" aria-haspopup="menu" aria-controls="brand-doors" aria-expanded="false" aria-label="${placeName(vm)}: teams and places" title="Teams and places"${waits ? raw(' data-waits') : ''}><span class="desk-lamp" id="phone-lamp">${deskMark(false)}</span><b>${placeName(vm)}</b>${sk('unfold')}</button>${place}</div>`
}
export const DRAWER_VEIL = raw('<div class="drawer-veil" id="drawer-veil" aria-hidden="true"></div>')

/** The drawer's switch: <html data-drawer="open">. The handle and the veil open and close it, Escape and a choice made
 *  in it close it; a finger opens it from the left edge and pushes it back (the drawer follows the finger: --dx). */
function drawer() {
  const root = document.documentElement, phone = matchMedia('(max-width: 860px)')
  const isOpen = () => root.dataset.drawer === 'open'
  requestAnimationFrame(paintTopStrip)
  const width = () => $('#agents')?.offsetWidth || 300
  const set = (open, { focus = true } = {}) => {
    if (open === isOpen()) return
    if (open) root.dataset.drawer = 'open'; else delete root.dataset.drawer
    $('#drawer-open')?.setAttribute('aria-expanded', String(open))
    paintTopStrip()
    // (what lies under the veil is out of the keyboard's and a screen reader's way while the drawer is open)
    for (const el of document.querySelectorAll('body > main, #phone-bar, #corner-note-box, .curl')) el.inert = open
    if (!focus) return
    if (open) ($('#agents .agent-entry[aria-current="page"]') ?? $('#desk-go'))?.focus({ preventScroll: true })
    else if (document.activeElement?.closest?.('#agents, .topbar')) $('#drawer-open')?.focus({ preventScroll: true })
  }
  document.addEventListener('click', e => {
    const t = e.target instanceof Element ? e.target : null
    if (!t) return
    if (t.closest('#drawer-open')) return set(!isOpen())
    if (!isOpen()) return
    if (t.closest('#drawer-veil')) return set(false)
    // a place chosen in the drawer: a session, the Desk, a team or a page of the menu (not the fold switches, not the menu's own button)
    if (t.closest('#agents a[href], #desk-go, #brand-doors a[href], #sidebar-invite')) set(false, { focus: false })
  })
  document.addEventListener('keydown', e => { if (e.key === 'Escape' && isOpen() && $('#brand-doors')?.hidden !== false) { e.stopPropagation(); set(false) } }, true)
  document.addEventListener('turbo:load', () => { if (isOpen()) set(false, { focus: false }) })
  phone.addEventListener('change', () => set(false, { focus: false }))
  // The finger: from the left edge the drawer comes along; on the open drawer (or the veil) a push to the left takes it back.
  let drag = null
  const end = () => { root.classList.remove('is-drawer-drag'); root.style.removeProperty('--dx'); root.style.removeProperty('--veil'); drag = null }
  addEventListener('touchstart', e => {
    if (!phone.matches || e.touches.length !== 1) return
    const p = e.touches[0], open = isOpen()
    if (open ? p.clientX > width() + 80 : p.clientX > 22 || e.target.closest?.('#whiteboard, input, textarea')) return
    drag = { x: p.clientX, y: p.clientY, open, w: width(), on: false, at: p.clientX, t: e.timeStamp, v: 0 }
  }, { passive: true })
  addEventListener('touchmove', e => {
    if (!drag) return
    const p = e.touches[0], dx = p.clientX - drag.x, dy = p.clientY - drag.y
    if (!drag.on) {
      if (Math.abs(dx) < 8 && Math.abs(dy) < 8) return
      if (Math.abs(dy) > Math.abs(dx) || (drag.open ? dx > 0 : dx < 0)) return end()
      drag.on = true
      root.classList.add('is-drawer-drag')
    }
    const x = Math.max(-drag.w, Math.min(0, (drag.open ? 0 : -drag.w) + dx))
    drag.v = (p.clientX - drag.at) / Math.max(1, e.timeStamp - drag.t); drag.at = p.clientX; drag.t = e.timeStamp
    drag.now = x
    root.style.setProperty('--dx', `${x}px`)
    root.style.setProperty('--veil', (1 + x / drag.w).toFixed(3))
  }, { passive: true })
  const up = () => {
    if (!drag) return
    const d = drag
    end()
    if (d.on) set(Math.abs(d.v) > .35 ? d.v > 0 : d.now > -d.w / 2)
  }
  addEventListener('touchend', up, { passive: true })
  addEventListener('touchcancel', up, { passive: true })
}

// ---- controller "menu" ----
// The Trommi menu (sidebar.mjs renders it; ui.mjs opens and closes it and switches the theme).
// Here: the arrows through the entries, a new team, and "#jump" in the address, which opens it (the menu has no
// search field; Ctrl K opens the menu with the keyboard on the team in view).

/** Go to a page the way a click on a link does (Turbo for the pages rendered here, a whole load for the others). */
function go(path) {
  const a = document.createElement('a')
  a.href = path
  a.style.display = 'none'
  document.body.append(a)
  a.click()
  a.remove()
}

export const MenuController = class extends Controller {
  static targets = ['teamForm', 'teamName', 'teamError']
  static values = { team: String }

  connect() {
    // Opened (its button, Ctrl+K, G then J): the menu takes the keyboard; the first arrow goes to the team in view.
    // Closed, the keyboard is back on the button, unless it has gone to something else on the page.
    this.element.tabIndex = -1
    this.watch = new MutationObserver(() => {
      const at = document.activeElement
      if (!this.element.hidden) { if (!this.element.contains(at)) this.element.focus({ preventScroll: true }) }
      else if (!at || at === document.body || this.element.contains(at)) this.opener?.focus({ preventScroll: true })
    })
    this.watch.observe(this.element, { attributes: true, attributeFilter: ['hidden'] })
    // A refresh of the page (the live stream's "refresh" morphs it) must not shut the menu or the new team's line under the hand.
    this.keep = e => {
      const t = e.target, name = e.detail?.attributeName
      if ((t === this.element && name === 'hidden') || (t.id === 'brand-menu' && name === 'aria-expanded') || (t.id === 'team-new' && name === 'hidden')) e.preventDefault()
    }
    document.addEventListener('turbo:before-morph-attribute', this.keep)
    this.grab = this.grab.bind(this)
    this.element.addEventListener('pointerdown', this.grab)
    // (a row that was dragged is not also a click: the menu stays open and the team is not switched)
    this.noClick = e => { if (this.dragged) { e.preventDefault(); e.stopImmediatePropagation(); this.dragged = false } }
    this.element.addEventListener('click', this.noClick, true)
    this.away = e => { if (!this.element.hidden && e.target instanceof Element && !e.target.closest('.brand')) this.close() }
    document.addEventListener('focusin', this.away)
    if (location.hash === '#jump') {
      history.replaceState(history.state, '', location.pathname + location.search)
      this.element.hidden = false
      this.opener?.setAttribute('aria-expanded', 'true')
    }
  }
  disconnect() {
    this.watch.disconnect()
    document.removeEventListener('focusin', this.away)
    document.removeEventListener('turbo:before-morph-attribute', this.keep)
    clearTimeout(this.timer)
  }
  // ---- the teams' order: a row is picked up and dragged up or down (a mouse: as soon as it moves; a finger: held a
  // moment first, so a swipe still scrolls). The others make room; let go, the order goes to the core (OrderTeams:
  // each team's register holds its place, on every device) and the number keys 1…9 follow it. ----
  grab(event) {
    const row = event.target.closest?.('.menu-team-row')
    if (!row || event.button > 0 || row.classList.contains('is-renaming') || event.target.closest('.menu-team-pen, input')) return
    const list = row.parentElement, rows = () => [...list.querySelectorAll(':scope > .menu-team-row')]
    if (rows().length < 2) return
    const y0 = event.clientY, x0 = event.clientX, touch = event.pointerType !== 'mouse', id = event.pointerId
    let live = false, timer = 0, grabAt = null
    const kids = rows(), start = kids.map(r => r.dataset.team).join()
    const lift = () => {
      live = true
      try { row.setPointerCapture(id) } catch {}
      row.classList.add('is-dragging'); list.classList.add('is-sorting')
      if (touch) navigator.vibrate?.(8)
    }
    if (touch) timer = setTimeout(lift, 280)
    const renumber = () => rows().forEach((r, i) => { const k = r.querySelector('kbd'); if (k) k.textContent = i < 9 ? String(i + 1) : '' })
    const move = e => {
      if (e.pointerId !== id) return
      const dy = e.clientY - y0
      if (!live) {
        if (touch) { if (Math.hypot(e.clientX - x0, dy) > 8) end(e, true); return }
        if (Math.abs(dy) < 5) return
        lift()
      }
      e.preventDefault()
      // the row follows the pointer; past the middle of a neighbour it takes that one's place
      row.style.translate = ''
      grabAt ??= y0 - row.getBoundingClientRect().top
      const before = rows().filter(r => r !== row).find(r => { const b = r.getBoundingClientRect(); return e.clientY < b.top + b.height / 2 }) ?? null
      if (before !== row.nextElementSibling) { list.insertBefore(row, before); renumber() }
      row.style.translate = `0 ${(e.clientY - grabAt - row.getBoundingClientRect().top).toFixed(1)}px`
    }
    const end = (e, cancel = false) => {
      if (e.pointerId !== id) return
      clearTimeout(timer)
      removeEventListener('pointermove', move); removeEventListener('pointerup', end); removeEventListener('pointercancel', end)
      if (!live) return
      this.dragged = true; setTimeout(() => { this.dragged = false }, 0)
      row.classList.remove('is-dragging'); list.classList.remove('is-sorting')
      row.animate?.([{ translate: row.style.translate || '0 0' }, { translate: '0 0' }], { duration: 160, easing: 'cubic-bezier(.3, 1.4, .5, 1)' })
      row.style.translate = ''
      const order = rows().map(r => r.dataset.team)
      // (the rows stand as they were painted again: the core's render draws the new order)
      for (const r of kids) list.append(r)
      if (cancel || e.type === 'pointercancel' || order.join() === start) return
      T?.act({ do: 'order_teams', team_ids: order })
    }
    addEventListener('pointermove', move, { passive: false }); addEventListener('pointerup', end); addEventListener('pointercancel', end)
  }
  get opener() { return document.getElementById('brand-menu') }
  get items() { return [...this.element.querySelectorAll('[role^="menuitem"], [role="option"]')].filter(n => n.checkVisibility()) }
  close() {
    this.element.hidden = true
    this.opener?.setAttribute('aria-expanded', 'false')
    delete this.element.dataset.from
  }

  /** Where the keyboard starts: the team in view (its lamp on), else the first entry. */
  home() { return this.element.querySelector('.menu-team[aria-checked="true"]') ?? this.items[0] }
  // The arrows walk the entries, round at both ends.
  walk(event) {
    const all = this.items, at = all.indexOf(document.activeElement)
    if (at < 0 && ['ArrowDown', 'ArrowUp'].includes(event.key)) { event.preventDefault(); return this.home()?.focus() }
    const to = { ArrowDown: at + 1, ArrowUp: at - 1, Home: 0, End: all.length - 1 }[event.key]
    if (to == null) return
    event.preventDefault()
    all[(to + all.length) % all.length]?.focus()
  }

  // ---- entries ----
  // A choice closes the menu; a switch (theme, push) leaves it open.
  // ("New team" opens its line and leaves the menu open.)
  // (Push and Theme and picking a team keep the menu open, the board behind changes live; what goes to another page
  // closes it; Esc and a click beside it close it too)
  chosen(event) {
    const t = event.target
    if (t.closest('#push-toggle, #theme-toggle, #team-add, .menu-team-pen, .menu-team-form')) return
    // (only a team picked in the open menu keeps it open: the number keys 1…9 click the same entry in the closed
    // menu, and that switch must leave it shut, or the menu comes up and takes the next key)
    if (t.closest('.menu-team[data-team]')) { if (!this.element.hidden) try { sessionStorage.setItem('trommi-menu-keep', '1') } catch {} return }
    if (t.closest('[role="menuitem"]:not([data-menu-body-param]), [role="menuitemradio"], [role="option"]')) this.close()
  }

  // ---- a new team: "+" opens a line for its name; Enter makes it (AddTeam) and goes there ----
  newTeam() {
    this.teamFormTarget.hidden = !this.teamFormTarget.hidden
    this.teamErrorTarget.textContent = ''
    if (!this.teamFormTarget.hidden) this.teamNameTarget.focus()
  }
  teamKey(event) {
    event.stopPropagation()   // the arrows and keys of the menu are not this line's
    if (event.key === 'Escape') { event.preventDefault(); this.teamNameTarget.value = ''; this.teamFormTarget.hidden = true; this.teamErrorTarget.textContent = ''; this.element.querySelector('#team-add')?.focus() }
  }
  async makeTeam(event) {
    event.preventDefault()
    const name = this.teamNameTarget.value.trim()
    if (!name) return this.teamNameTarget.focus()
    const made = await addTeam(name)
    if (!made) { this.teamErrorTarget.textContent = 'Not made: the team did not come.'; return }
    this.teamNameTarget.value = ''
    this.teamFormTarget.hidden = true
    this.close()
    go(`${this.teamValue}?team=${encodeURIComponent(made.id)}`)
  }
  // ---- rename a team: the pencil puts a field where the name is; Enter saves (RenameTeam), Escape leaves it ----
  rename({ params: { id } }) {
    const row = this.element.querySelector(`.menu-team-row[data-team="${CSS.escape(id)}"]`), was = row?.querySelector('b')?.textContent ?? ''
    if (!row || row.querySelector('.menu-team-rename')) return
    const form = el('form', 'menu-team-rename'), field = Object.assign(el('input', 'menu-team-field'), { maxLength: 40, autocomplete: 'off', enterKeyHint: 'done', ariaLabel: `New name of the team ${was}` })
    field.value = was
    form.append(field)
    row.classList.add('is-renaming'); row.append(form)
    const done = () => { form.remove(); row.classList.remove('is-renaming') }
    field.addEventListener('keydown', e => { e.stopPropagation(); if (e.key === 'Escape') { e.preventDefault(); done(); row.querySelector('.menu-team-pen')?.focus() } })
    field.addEventListener('blur', () => setTimeout(() => { if (form.isConnected && !form.contains(document.activeElement)) done() }, 0))
    form.addEventListener('submit', e => {
      e.preventDefault()
      const name = field.value.trim().slice(0, 40)
      done()
      if (name && name !== was) T?.act({ do: 'rename_team', team_id: id, name })
    })
    field.focus(); field.select()
  }
  keys() { this.close(); document.dispatchEvent(new CustomEvent('trommi:keys')) }
}
controller('menu', MenuController)

// ---- controller "rail" ----
// The rail: the sidebar folded to the sessions' drawings with their marks (crown, bracket, the drawing
// that fills itself in while a session works, the count). Wide screens only (sidebar.css, [data-rail="folded"]).
// The small "|<" at the right end of the sidebar's foot folds and opens it, and so does the key [ (ui.mjs (keys) presses this button).
// Remembered per browser: the layout's head sets data-rail on <html> before first paint; this controller only flips it.
// The rail shows no names, so a row says its name in a note beside it while the pointer or the keyboard is on it.

const KEY = 'trommi-rail'
const wide = () => matchMedia('(min-width: 861px)').matches
const folded = () => document.documentElement.dataset.rail === 'folded'

// ---- controller "side-resize" ----
// The sidebar's width: its right edge is dragged (a thin handle, a line under the pointer), or moved with the arrow
// keys; under SNAP it becomes the rail, dragged out again the sidebar comes back. Never narrower than the rail: the
// sidebar stays. Remembered per browser ('trommi-side-w', and the rail as before); the page's head sets both before
// the first paint (index.html). The key [ still folds and opens the rail (controller "rail").
const SIDE_W = 'trommi-side-w', SIDE_MIN = 208, SIDE_MAX = 420, SNAP = 150
const sideNow = () => parseInt(document.documentElement.style.getPropertyValue('--side'), 10) || 272
controller('side-resize', class extends Controller {
  down(event) {
    if (!wide() || event.button > 0) return
    event.preventDefault()
    const el = this.element, root = document.documentElement
    el.setPointerCapture(event.pointerId)
    root.classList.add('is-resizing')
    const was = sideNow(), move = e => this.to(e.clientX)
    // (dragged into the rail: the width before stays the one the rail opens to again)
    const up = () => { for (const [n, f] of [['pointermove', move], ['pointerup', up], ['pointercancel', up]]) el.removeEventListener(n, f); root.classList.remove('is-resizing'); if (folded()) root.style.setProperty('--side', `${was}px`); this.keep() }
    for (const [n, f] of [['pointermove', move], ['pointerup', up], ['pointercancel', up]]) el.addEventListener(n, f)
  }
  /** The edge at x: the rail under SNAP, else that width within its bounds. */
  to(x) {
    if (x < SNAP) return this.rail(true)
    this.rail(false)
    document.documentElement.style.setProperty('--side', `${Math.round(Math.min(SIDE_MAX, Math.max(SIDE_MIN, x)))}px`)
  }
  rail(on) {
    if (on === folded()) return
    if (on) document.documentElement.dataset.rail = 'folded'; else delete document.documentElement.dataset.rail
    dispatchEvent(new Event('resize'))
  }
  keep() { try { folded() ? localStorage.setItem(KEY, 'folded') : localStorage.removeItem(KEY); localStorage.setItem(SIDE_W, String(sideNow())) } catch {} }
  key(event) {
    const step = { ArrowLeft: -16, ArrowRight: 16 }[event.key]
    if (!step || !wide()) return
    event.preventDefault()
    this.to(folded() ? (step > 0 ? SIDE_MIN : 0) : sideNow() + step < SIDE_MIN ? 0 : sideNow() + step)
    this.keep()
  }
  flip() { this.rail(!folded()); this.keep() }
})

controller('rail', class extends Controller {
  connect() { this.paint() }
  disconnect() { this.untip() }

  toggle() {
    if (!wide()) return
    const fold = !folded()
    if (fold) document.documentElement.dataset.rail = 'folded'; else delete document.documentElement.dataset.rail
    try { fold ? localStorage.setItem(KEY, 'folded') : localStorage.removeItem(KEY) } catch {}
    this.untip()
    this.paint()
    dispatchEvent(new Event('resize'))   // the brackets of unfolded mains are drawn again for the new width
  }

  paint() {
    const label = `${folded() ? 'Open the sidebar' : 'Fold the sidebar to a rail'} ( [ )`
    this.element.title = `${folded() ? 'Open' : 'Fold'} ( [ )`
    this.element.setAttribute('aria-label', label)
    this.element.setAttribute('aria-pressed', String(folded()))
  }

  tip(event) {
    const row = event.target.closest?.('#agents .agent-row')
    if (row === this.row) return
    this.untip()
    if (!row || !folded() || !wide()) return
    const name = row.querySelector('.agent-text strong')?.textContent.trim()
    if (!name) return
    const r = row.getBoundingClientRect()
    const note = document.createElement('div')
    note.className = 'rail-tip'
    note.setAttribute('aria-hidden', 'true')
    note.textContent = name
    note.style.left = `${Math.round(r.right + 10)}px`
    note.style.top = `${Math.round(r.top + r.height / 2 - 13)}px`
    document.body.append(note)
    this.row = row
    this.note = note
    // The sidebar scrolls under a still pointer: the note would stand beside the wrong row.
    this.off = () => this.untip()
    document.getElementById('agents')?.addEventListener('scroll', this.off, { once: true, passive: true })
  }

  untip() {
    this.note?.remove()
    if (this.off) document.getElementById('agents')?.removeEventListener('scroll', this.off)
    this.note = this.row = this.off = null
  }
})

// ---- the frame's wiring, once ----
export function register(t) {
  T = t
  // Into a team (its stack), back out ("‹ Teams"), home (the bell: the Desk and the first level).
  document.addEventListener('click', e => {
    const el = e.target instanceof Element ? e.target : null
    if (el?.closest('#agents [data-team-in]')) { t.setInTeam(true); return }
    if (el?.closest('#agents [data-team-out], .side-home')) { if (el.closest('[data-team-out]')) e.preventDefault(); t.setInTeam(false) }
  })
  import('./app.mjs').then(m => { newsLatestDay = m.newsLatestDay })
  t.get(/^\/chats$/, ({ req, res }) => t.page(req, res, { title: 'Chats · Trommi', view: 'chats', main: chatsMain(t.vm(), t.BASE) }))
  document.addEventListener('turbo:load', () => { const m = /^\/chat\/([^/?#]+)$/.exec(location.pathname); if (m) chatOpened(decodeURIComponent(m[1])) })
  // The Trommi menu opens and closes (its button, a click beside it, Escape); the theme switch.
  const shut = () => { const doors = $('#brand-doors'); if (doors && !doors.hidden) { doors.hidden = true; $('#brand-menu')?.setAttribute('aria-expanded', 'false'); delete doors.dataset.from } }
  document.addEventListener('click', e => {
    const t = e.target instanceof Element ? e.target : null
    if (!t) return
    // the team switch: the whole Trommi menu (the teams at its top) opens from the chevron beside the team's name,
    // and from the folded rail's tag
    if (t.closest('.team-switch-open, .rail-tag') && !t.closest('#desk-pill')) { const doors = $('#brand-doors'), b = $('#brand-menu'); if (doors && b) { doors.dataset.from = 'head'; doors.hidden = !doors.hidden; b.setAttribute('aria-expanded', String(!doors.hidden)); for (const o of document.querySelectorAll('.team-switch-open, .rail-tag')) o.setAttribute('aria-expanded', String(!doors.hidden)) } return }
    const menu = t.closest('#brand-menu, .rail-tag') && $('#brand-menu'), doors = $('#brand-doors')
    if (menu && doors) { delete doors.dataset.from; doors.hidden = !doors.hidden; menu.setAttribute('aria-expanded', String(!doors.hidden)); return }
    if (doors && !doors.hidden && !t.closest('#brand-doors')) shut()
    // the demo: a switch, never a team; off takes the tab back to its own room for sure (app.mjs ?mock=0)
    if (t.closest('#demo-toggle')) { location.assign(inDemo() ? '/?mock=0' : '/?mock=1'); return }
    if (t.closest('#theme-toggle, .side-theme')) setThemeMode(nextThemeMode())   // Light → Dark → System (the sidebar's foot on a wide window, the menu on a phone)
  })
  // The phone's bar: Note opens the corner note (the sticky itself stands back on a phone with the bar).
  document.addEventListener('click', e => {
    const tab = e.target instanceof Element ? e.target.closest('.tab[data-tab="note"]') : null
    if (!tab) return
    const box = $('#corner-note-box'), head = $('#corner-note-box .corner-note-head')
    if (box?.dataset.tabClosed) { delete box.dataset.tabClosed; tab.setAttribute('aria-expanded', 'false'); return }
    head?.click()
    requestAnimationFrame(() => tab.setAttribute('aria-expanded', String($('#corner-note-box .corner-note-head')?.getAttribute('aria-expanded') === 'true')))
  })
  document.addEventListener('keydown', e => { if (e.key === 'Escape') shut() })
  drawer()
  // The rail is a wide screen's: a narrow window has the drawer, whole (the head's data-rail is taken off there).
  const narrow = matchMedia('(max-width: 860px)')
  const rail = () => { if (narrow.matches) delete document.documentElement.dataset.rail; else { try { if (localStorage.getItem(KEY) === 'folded') document.documentElement.dataset.rail = 'folded' } catch {} } dispatchEvent(new Event('resize')) }
  narrow.addEventListener('change', rail)
  if (narrow.matches) rail()
  // Push on this device (the menu's bell, vm.settings.push): Off → Only knocking → All → Off; the core registers this
  // browser (Push).
  document.addEventListener('click', e => {
    const bell = e.target instanceof Element ? e.target.closest('#push-toggle') : null
    if (!bell || bell.getAttribute('aria-busy') === 'true') return
    e.stopPropagation()
    const now = bell.dataset.level || 'off', level = now === 'off' ? 'knocking' : now === 'knocking' ? 'all' : 'off'
    // (what stands in the way on this device is the core's to say: the platform refuses, the core tells why)
    const why = t.vm().settings?.push ? '' : 'This browser has no push.'
    const note = $('#menu-push-note')
    if (note) { note.textContent = why; note.hidden = !why }
    if (!why) t.act({ do: 'push', level })
  }, true)
}

