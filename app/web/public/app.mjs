// The app: its start (boot), the board (pages, forms: every view registers its own), the router, the frame around a
// page, the service worker's side (new versions). Importing it does nothing; index.html's import starts it (boot, at
// the end).
//
// The room is the app core's (core.mjs: the core runs in its worker and sends the view model, core/bindings/wasm/js/
// views.d.ts). A page is drawn from that model; what the person does goes to the core as an action. When the model
// changes, the page in view is drawn again and the body is morphed in place (only what changed is touched).
import * as desk from './desk.mjs'
import * as sidebar from './sidebar.mjs'
import * as notes from './notes.mjs'
import { DRAWER_VEIL, SIDE_FOOT, cornerNote, markCurrent, phoneBar, sidebarRows, tabBar, topbar } from './sidebar.mjs'
import { Controller, calm, controller, curlHTML, html, keySheet, raw, startUi, toast } from './ui.mjs'
import { movedPath } from './paths.mjs'
import { rowSheet } from './desk.mjs'
import * as core from './core.mjs'
import { passkeysOn, pushable } from './platform.mjs'

// The views a cold start needs (the Desk, its frame, the notes) come with this module; every other view is loaded
// when an address of it is first asked for (LAZY: the addresses it answers), and all of them once the page is idle,
// so a later navigation finds them in memory.
const VIEWS = [desk, sidebar, notes]
/** "MLS proof" (proof.mjs): a page of Settings, and a screen of its own on a device without a room (boot). */
const PROOF_PATH = /^\/settings\/proof$/
const LAZY = {
  auth: { load: () => import('./auth.mjs'), paths: /^\/(?:settings(?:\/(?:devices|account|theme|keys|kit|password|security))?|devices\/|pair|logout|join|login)(?:\/|$)/ },
  agents: { load: () => import('./agents.mjs'), paths: /^\/(?:settings\/(?:sessions|agents)$|sessions\/)/ },
  desks: { load: () => import('./desks.mjs'), paths: /^\/settings\/desks(?:\/|$)/ },
  card: { load: () => import('./card.mjs'), paths: /^(?:\/chat\/[^/]+)?\/card\/|^\/cards\/[0-9a-f]+\// },
  session: { load: () => import('./session.mjs'), paths: /^\/chat\// },
  media: { load: () => import('./media.mjs'), paths: /^\/artifacts(?:\/|$)/ },
  whiteboard: { load: () => import('./whiteboard.mjs'), paths: /^\/scribble-board$/ },
  proof: { load: () => import('./proof.mjs'), paths: PROOF_PATH },
  news: { load: () => import('./news.mjs'), paths: /^\/whats-new$/ },
}
const loaded = {}   // name -> the view's module, once loaded
/** Another view's module, for a view that draws with its parts (desks.mjs: the Scribble Board's canvas and thumbnail). */
export const viewModule = name => view(name)
/** A lazy view's module (loaded once; registered on the board, when there is one, as it arrives). */
function view(name) {
  const v = LAZY[name]
  v.promise ??= v.load().then(m => { loaded[name] = m; v.onLoad?.(m); return m }, err => { v.promise = null; throw err })
  return v.promise
}

// ---- the app's version ----
// The app's version, sent to the hub on every request as Trommi-Client: app/<version> (with Trommi-Protocol: 1, by
// the core). Raise it with every release that changes what the app sends or understands.
const APP_VERSION = '0.2.0'
/** What stands where something of a newer Trommi version would be (codec UPDATE_MESSAGE, in the app's words). */
export const NEEDS_NEWER = 'This needs a newer version of Trommi. Reload to update.'
export const CLIENT = `app/${APP_VERSION}`
// What's new (news.mjs): /whats-new.json, the worker's days from the releases' notes (whats-new.mjs), asked once after
// the start (the menu row's dot needs the newest day). The last answer is kept on this device for when it is offline.
// (In Node's tests there is no such address: no days.)
const NEWS_KEY = 'trommi-news-days'
let newsLatest = null
let newsDays = null
/** The newest day of What's new, or null until it is loaded. */
export const newsLatestDay = () => newsLatest
/** { DAYS }: What's new's days, newest first ([] when neither the network nor this device has them). */
export const changelog = () => (newsDays ??= fetch('/whats-new.json', { headers: { Accept: 'application/json' } })
  .then(r => (r.ok ? r.json() : Promise.reject(new Error(`HTTP ${r.status}`))))
  .then(j => { const days = Array.isArray(j?.days) ? j.days : []; try { localStorage.setItem(NEWS_KEY, JSON.stringify(days)) } catch {} return days })
  .catch(() => { try { const kept = JSON.parse(localStorage.getItem(NEWS_KEY) ?? '[]'); return Array.isArray(kept) ? kept : [] } catch { return [] } })
  .then(days => { newsLatest = days[0]?.day ?? ''; globalThis.document?.dispatchEvent(new CustomEvent('trommi:news')); return { DAYS: days } }))
if (globalThis.location?.protocol?.startsWith('http')) changelog()
// What runs here, for the small line at the top of What's new: the release (gen/release.json, which the release writes
// beside the build: number, commit, time), else the build's commit (gen/build.txt, the dev server's too). Asked once,
// after the page has started; offline it says nothing.
let running = null
/** { release: 'v152' | '', commit, when: 'today, 14:32' | '' }, or null until known. */
export const runningBuild = () => running
/** The same as one line, for the top of What's new: "v152 · 97f72b6 · updated today, 14:32" ('' until known). */
export const runningLine = () => (running ? [running.release, running.commit, running.when && `updated ${running.when}`].filter(Boolean).join(' · ') : '')
setTimeout(() => {
  const when = iso => { const at = iso ? new Date(iso) : null; if (!at || Number.isNaN(+at)) return ''; const day = at.toDateString() === new Date().toDateString() ? 'today' : at.toLocaleDateString('en-GB', { day: 'numeric', month: 'short' }); return `${day}, ${at.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' })}` }
  fetch('/gen/release.json', { cache: 'no-store' }).then(r => (r.ok ? r.json() : null)).catch(() => null)
    .then(rel => (rel?.version ? { release: `v${rel.version}`, commit: String(rel.commit ?? '').slice(0, 7), when: when(rel.released) }
      : fetch('/gen/build.txt', { cache: 'no-store' }).then(r => (r.ok ? r.text() : '')).then(t => { const c = /^commit: (\S+)$/m.exec(t)?.[1]; return c ? { release: '', commit: c.slice(0, 7), when: '' } : null })))
    .then(b => { if (b) { running = b; document.dispatchEvent(new CustomEvent('trommi:news')) } }, () => {})
}, 1500)
// A QR reader for browsers without their own (core/qr-decode.mjs): loaded by the account screens' "Scan a code" only.
export const qrReader = () => import('./gen/vendor/qr-decode.mjs')

// Where the hub is (a tab session may override it: ?hub=, for development).
export const ses = (k, v) => { try { if (v != null) sessionStorage.setItem(k, v); return sessionStorage.getItem(k) } catch { return v ?? null } }
/** The hub: fixed https://hub.trommi.com (local dev: http://127.0.0.1:8890). Hidden developer override, no UI: ?hub=<url> and ?found_code=<code>, read once and kept for the tab session. */
export function hubUrl() {
  const q = new URLSearchParams(location.search)
  const asked = q.get('hub')
  const local = ['localhost', '127.0.0.1', '[::1]'].includes(location.hostname)
  return (ses('trommi-hub', asked ? asked.replace(/\/+$/, '') : null)) || (local ? 'http://127.0.0.1:8890' : 'https://hub.trommi.com')
}

// ---- blocked ----
// When a session is really stopped: the raised red hand. It is about a SESSION, not a card, and it can stand without
// any card (cut off, or disconnected while working). Whether it stands is the client core's rule, decided with the

// ---- board ----
// The board's pages and forms, in the page. The view modules register themselves (register(t) with t.get, t.post);
// a "request" here is a navigation or a form of this page, answered from the view model. A form's handler acts
// (t.act: an action of the core) and answers with a redirect, stream actions, or nothing (204): the page is drawn
// again when the core's next render comes.
//
//   const board = createBoard({ views })
//   await board.request({ method, path, form, json, headers })  -> { kind: 'page' | 'stream' | 'redirect' | 'html' | 'json' | 'none', … }

export const BASE = ''
const STREAM = 'text/vnd.turbo-stream.html'
export const stream = (action, target, content = '') => html`<turbo-stream action="${action}"${target ? html` target="${target}"` : ''}>${action === 'remove' || action === 'refresh' || action === 'visit' ? '' : html`<template>${content}</template>`}</turbo-stream>`

const read = (k, f = null) => { try { return localStorage.getItem(k) ?? f } catch { return f } }
const write = (k, v) => { try { if (v == null) localStorage.removeItem(k); else localStorage.setItem(k, v) } catch {} }
/** The desk in view on this browser: a desk's id, 'all', or null (the first desk). */
const DESK_KEY = 'trommi-desk'
let deskInView = read(DESK_KEY)

function createBoard({ views }) {
  // ---- answers: a response object the router reads ----
  const response = () => ({
    kind: 'none', code: 200, body: '', opts: null, to: null,
    writeHead(code, headers = {}) { this.code = code; if (headers.Location) { this.kind = 'redirect'; this.to = headers.Location } },
    end(body = '') { if (this.kind === 'none' && body) { this.kind = 'html'; this.body = String(body) } },
  })
  const redirect = (res, to) => { res.kind = 'redirect'; res.to = to; res.code = 303 }
  const notFoundMain = what => html`<main id="inbox" aria-label="Not found"><header class="inbox-head"><div class="inbox-title"><h2>${what}</h2><p><a href="${BASE}/" data-nav>Back to the Desk</a></p></div></header></main>`

  // ---- the toolkit the page modules get ----
  const gets = [], posts = []
  const t = {
    BASE, stream, redirect,
    /** The view model now (views.d.ts ViewModel). */
    vm: core.vm,
    /** An action of the core (core/client/src/actions/mod.rs), with the bytes its JSON names by index. */
    act: core.act,
    /** An account step (core/client/src/account/mod.rs AccountAction). */
    account: core.account,
    /** A file's object URL once here (else null; the core is asked for it). */
    fileUrl: core.fileUrl,
    attach: core.attach,
    saveFile: core.saveFile,
    /** The desk in view: its id, 'all', or null for the first. */
    desk: () => deskInView,
    setDesk(d) { deskInView = d; write(DESK_KEY, d); if (d && d !== ALL_DESKS) write('trommi-desk-last', d); sendFocus() },
    toast: opts => stream('prepend', 'says-host', toast(opts)),
    get(pattern, handler) { gets.push({ pattern, handler }) },
    post(pattern, handler) { posts.push({ pattern, handler }) },
    /** A page: { title, view, main, focus (the core's Screen), current (the session in view), sidebar, scope, bodyAttrs }. */
    page(req, res, opts, code = 200) { res.kind = 'page'; res.code = code; res.opts = { ...opts, vm: core.vm() } },
    sendStream: (req, res, body, code = 200) => { res.kind = 'stream'; res.code = code; res.body = String(body) },
    json: (res, value, code = 200) => { res.kind = 'json'; res.code = code; res.body = JSON.stringify(value) },
    done: res => { res.kind = 'none'; res.code = 204 },
    wantsStream: req => String(req.headers.accept ?? '').includes(STREAM),
    redirect, notFound: (req, res, what) => t.page(req, res, { title: 'Not found · Trommi', view: 'missing', main: notFoundMain(what) }, 404),
  }

  // Every view hooks itself in: its pages and forms (and what it wires on the page once).
  for (const view of views) view.register?.(t)
  // A lazy view hooks itself in when it arrives (asked for by an address, or loaded while the page was idle).
  for (const [name, v] of Object.entries(LAZY)) { if (loaded[name]) loaded[name].register?.(t); v.onLoad = m => m.register?.(t) }

  /** One request of this page: a navigation (GET) or a form (POST). */
  async function request({ method = 'GET', path, form = null, json = null, headers = {} }) {
    const url = new URL(path, location.origin)
    const req = { method, url: url.pathname + url.search, headers, form, json }
    const res = response()
    const p = url.pathname
    for (const [name, v] of Object.entries(LAZY)) if (!loaded[name] && v.paths.test(p)) await view(name)
    // (signed out, or an account step under way: the account's screens decide where the page is: auth.mjs gate)
    const vm = core.vm()
    if (method === 'GET' && (vm.phase === 'signed_out' || !['none', 'invite', undefined].includes(vm.account?.step))) {
      const to = (await view('auth')).gate?.(vm, url)
      if (to && to !== url.pathname + url.search) { redirect(res, to); return res }
    }
    const list = method === 'GET' ? gets : posts
    for (const { pattern, handler } of list) {
      const match = pattern.exec(p)
      if (!match) continue
      if ((await handler({ req, res, url, match, form, json })) !== false) return res
    }
    res.kind = 'missing'
    return res
  }
  return { request, t }
}

// ---- focus: what the person looks at, for the core ----
const ALL_DESKS = 'all'
let screenInView = { screen: 'desk' }
let boardInView = null
/** The core hears what is in view: the page's screen, the desk, the Scribble Board beside it, whether the window is seen. */
function sendFocus() {
  core.focus({ screen: screenInView, visible: document.visibilityState === 'visible', ...(deskInView ? { desk: deskInView } : {}), ...(boardInView ? { board: boardInView } : {}) })
}
/** The core's Screen for an address (before the page is drawn: its unit is then in the model). */
export function screenOf(pathname, search = '') {
  let m
  if ((m = /^\/chat\/([^/]+)\/card\/([^/]+)/.exec(pathname)) || (m = /^\/()card\/([^/]+)/.exec(pathname))) return { screen: 'card', card_id: decodeURIComponent(m[2]) }
  if ((m = /^\/chat\/([^/]+)/.exec(pathname))) return { screen: 'session', session_id: decodeURIComponent(m[1]) }
  if (pathname === '/stacks/off') return { screen: 'off', query: new URLSearchParams(search).get('q') ?? '' }
  if (/^\/artifacts(?:\/|$)/.test(pathname)) return { screen: 'artifacts' }
  if (pathname === '/scribble-board') return { screen: 'scribble', board_id: deskInView ?? read('trommi-desk-last') ?? ALL_DESKS }
  if (/^\/(?:settings|devices|pair|sessions)(?:\/|$)/.test(pathname)) return { screen: 'settings' }
  return { screen: 'desk' }
}
/** The page shows its screen's unit once the core sent it: a visit waits a moment for that render. */
function focusFor(url) {
  const s = screenOf(url.pathname, url.search)
  const same = JSON.stringify(s) === JSON.stringify(screenInView)
  screenInView = s
  if (same) return Promise.resolve()
  sendFocus()
  return new Promise(done => { const stop = core.on('render', () => { stop(); clearTimeout(t); done() }); const t = setTimeout(() => { stop(); done() }, 250) })
}

let visitor = null
/** The router registers how a visit is made (path, { action }). */
function setVisitor(fn) { visitor = fn }
function visit(location, options = {}) { return visitor?.(String(location), options) }


/** The elements (boot, once). <turbo-frame> is a plain element with its id; navigation inside it is the router's. */
function defineTurbo() {
  customElements.define('turbo-stream', class extends HTMLElement {
  get action() { return this.getAttribute('action') }
  get target() { return this.getAttribute('target') }
  get templateElement() { return this.querySelector('template') }
  get templateContent() { return this.templateElement?.content.cloneNode(true) ?? document.createDocumentFragment() }
  get targetElements() { const t = this.target ? document.getElementById(this.target) : null; return t ? [t] : [] }
  async connectedCallback() {
    if (this.started) return
    this.started = true
    const render = el => perform(el)
    const event = new CustomEvent('turbo:before-stream-render', { bubbles: true, cancelable: true, detail: { newStream: this, render } })
    if (this.dispatchEvent(event)) { try { await event.detail.render(this) } catch (err) { console.error('turbo-stream', err) } }
    this.remove()
  }
  })
  customElements.define('turbo-frame', class extends HTMLElement {})
  window.Turbo = { visit, renderStreamMessage }
}

/** The Trommi menu (#brand-doors) stays as it stood when a live update or the page drawn again replaces it: a person
 *  who just opened it does not see it shut under the pointer. Returns a function that puts it back. */
export function keepMenu() {
  const doors = document.getElementById('brand-doors')
  if (!doors || doors.hidden) return () => {}
  const from = doors.dataset.from ?? null
  // (and the line for a new desk's name, open, with what is typed in it and the focus in it)
  const form = document.getElementById('desk-new'), field = form?.querySelector('.menu-desk-field')
  const typing = form && !form.hidden ? { value: field?.value ?? '', focused: document.activeElement === field, at: field?.selectionStart ?? null } : null
  return () => {
    const now = document.getElementById('brand-doors')
    // (only a menu that was replaced: one still in the page was shut on purpose)
    if (!now || now === doors || !now.hidden) return
    now.hidden = false
    if (from) now.dataset.from = from
    for (const b of document.querySelectorAll('#brand-menu, .desk-switch-open, .rail-tag')) b.setAttribute('aria-expanded', 'true')
    const again = document.getElementById('desk-new'), line = again?.querySelector('.menu-desk-field')
    if (typing && again && line) {
      again.hidden = false
      line.value = typing.value
      if (typing.focused) { line.focus({ preventScroll: true }); if (typing.at !== null) line.setSelectionRange(typing.at, typing.at) }
    }
  }
}
async function perform(stream) {
  const back = keepMenu()
  try { return await performNow(stream) } finally { back() }
}
async function performNow(stream) {
  const action = stream.action, target = stream.targetElements[0]
  if (action === 'refresh') return refresher?.()
  if (action === 'visit') return visit(stream.getAttribute('target') || '/', { action: 'replace' })   // (a form's answer that leads on: the page it names)
  if (!target) return
  const content = () => stream.templateContent
  switch (action) {
    case 'remove': {
      // A Desk row that leaves goes in one continuous motion, without a jump: it slides
      // and fades while its place closes, so the rows below glide up at the same time; several leave staggered.
      if (target.matches('.inbox-row')) { flipOut([target]); break }
      target.remove(); break
    }
    case 'replace': target.replaceWith(content()); break
    case 'update': target.replaceChildren(content()); break
    case 'append': { const c = content(); dedupe(target, c); target.append(c); break }
    case 'prepend': { const c = content(); dedupe(target, c); target.prepend(c); break }
    case 'before': target.before(content()); break
    case 'after': target.after(content()); break
  }
}
let leaving = 0
/** Desk rows leave without a jump and without work per frame: measured once, each row
 *  is taken out of the layout at once (its section with it when it was the last), a fixed copy slides and fades where
 *  it stood, and what stood below is moved back by the gap and glides up: only transform and opacity animate.
 *  ghost: false when the caller flies its own copies (the pull-down of Later). Reduced motion: gone at once. */
export function flipOut(rows, { ghost = true } = {}) {
  rows = rows.filter(r => r?.isConnected)
  if (!rows.length) return []
  const boxes = [...new Set(rows.map(boxOf))]
  // (where each stood, so a row whose answer failed can come back: putBack)
  const places = boxes.map(box => ({ box, parent: box.parentElement, next: box.nextSibling }))
  if (calm()) { for (const b of boxes) b.remove(); return places }
  const list = rows[0].closest('#desk-list') ?? rows[0].parentElement
  // What may move: the rows after a leaving row in its own section, then the list's own children after it, in order;
  // only what stands in sight (and a little below) is measured: a row far below has nothing to show, and measuring it
  // would make the browser lay out and render it (rows out of sight are content-visibility: auto).
  const moving = []
  const seen = new Set(boxes)
  for (const box of boxes) {
    const after = []
    if (box.matches('.inbox-row')) for (let n = box.nextElementSibling; n; n = n.nextElementSibling) after.push(n)
    const top = box.matches('.inbox-row') ? box.parentElement : box
    if (top?.parentElement === list) for (let n = top.nextElementSibling; n; n = n.nextElementSibling) after.push(n)
    for (const el of after) if (!seen.has(el)) { seen.add(el); moving.push(el) }
  }
  const limit = (document.documentElement.clientHeight || innerHeight) + 120
  // (one read pass, no second one: an element below moves up by the room of every leaving box above it, a box's
  //  height and the gap after it; reading positions again after the removal would lay the page out once more)
  const gapOf = el => parseFloat(getComputedStyle(el.parentElement ?? el).rowGap) || 0
  const room = boxes.map(b => ({ b, h: b.getBoundingClientRect().height + gapOf(b) }))
  const before = new Map()
  for (const el of moving) {
    const t = el.getBoundingClientRect().top
    if (t > limit) break
    const up = room.reduce((sum, { b, h }) => sum + (b.compareDocumentPosition(el) & Node.DOCUMENT_POSITION_FOLLOWING && !b.contains(el) ? h : 0), 0)
    before.set(el, up)
  }
  const n0 = leaving
  leaving += rows.length
  setTimeout(() => { leaving = Math.max(0, leaving - rows.length) }, 450)
  if (ghost) rows.forEach((row, i) => {
    const r = row.getBoundingClientRect(), copy = row.cloneNode(true)
    copy.removeAttribute('id'); copy.classList.add('is-ghost'); copy.inert = true
    Object.assign(copy.style, { position: 'fixed', left: `${r.left}px`, top: `${r.top}px`, width: `${r.width}px`, height: `${r.height}px`, margin: 0, zIndex: 40, pointerEvents: 'none', contain: 'layout paint style' })
    document.body.append(copy)
    copy.animate([{ opacity: 1, transform: 'none' }, { opacity: 0, transform: 'translateX(28px)' }], { duration: 220, delay: (n0 + i) * 45, easing: 'cubic-bezier(.2, .7, .3, 1)', fill: 'forwards' }).finished.then(() => copy.remove(), () => copy.remove())
  })
  // (two frames: in this one the copy flies and the row is hidden; in the next the row leaves the layout and what stood
  //  below glides up, so the list's new layout is not paid for in the frame of the tap)
  for (const b of boxes) b.style.visibility = 'hidden'
  requestAnimationFrame(() => requestAnimationFrame(() => {
    for (const b of boxes) { b.remove(); b.style.visibility = '' }
    for (const [el, d] of before) {
      if (Math.abs(d) < 1 || !el.isConnected) continue
      el.style.willChange = 'transform'
      el.animate([{ transform: `translateY(${d}px)` }, { transform: 'none' }], { duration: 250, delay: n0 * 45, easing: 'cubic-bezier(.3, .6, .3, 1)' }).finished.finally(() => { el.style.willChange = '' })
    }
  }))
  return places
}
/** Rows whose answer did not go through stand where they stood again. */
function putBack(places) {
  for (const { box, parent, next } of places) if (!box.isConnected && parent?.isConnected) parent.insertBefore(box, next?.parentNode === parent ? next : null)
}
/** A row alone in its section takes the section with it. */
function boxOf(row) {
  const sec = row.parentElement
  return sec?.matches('.inbox-group') && ![...sec.children].some(c => c !== row && c.matches('.inbox-row')) ? sec : row
}
// As Turbo does: an appended element whose id stands in the target already replaces it.
function dedupe(target, fragment) {
  for (const el of fragment.children) if (el.id) target.querySelector(`#${CSS.escape(el.id)}`)?.remove()
}


let refresher = null
function setRefresher(fn) { refresher = fn }

/** Applies stream actions given as markup (<turbo-stream action target><template>…</template></turbo-stream>…). */
export function renderStreamMessage(text) {
  if (!text) return
  const t = document.createElement('template')
  t.innerHTML = String(text)
  const host = document.getElementById('stream-host') ?? document.body
  for (const el of [...t.content.querySelectorAll('turbo-stream')]) host.append(document.importNode(el, true))
}


// ---- layout ----
// The frame around every view: the floating Desk with the Trommi menu, the sidebar, the place for toasts, the key
// sheet, the notes. Only the body is made here: the router keeps the head and patches the body.

// The pages whose sheet has the turned corner: the Desk, a session's page and a card's own page. padFrom: the one the board was turned from (its corner turns back there).
const FRONT = new Set(['desk', 'session', 'card'])
let padFrom = '/'
let padKept = false   // the Scribble Board stays mounted under the Desk (set when the corner is first touched, or coming from the board)
const padCanvas = html => /data-whiteboard-canvas-value="([^"]*)"/.exec(html)?.[1] ?? null
/** The body's parts for a page: [{ key, html }] in order (the router keeps a part whose markup did not change). */
function bodyParts({ view, vm, base = '', main, sidebar = true, current = null, says = '', title = '' }) {
  const model = vm
  const parts = []
  if (sidebar) {
    if (model) parts.push({ key: 'phonebar', html: String(phoneBar(model, base, { view, current, title })) })
    parts.push({ key: 'topbar', html: String(topbar(model, base, view === 'desk', view === 'session' ? current : null)) })
    // (the same rows on every page: the session in view is marked after the paint, sidebar.mjs markCurrent)
    parts.push({ key: 'agents', html: String(html`<nav id="agents" aria-label="Sessions" data-controller="folds order">${sidebarRows(model, base)}</nav>`) })
    parts.push({ key: 'foot', html: String(SIDE_FOOT) })
    parts.push({ key: 'veil', html: String(DRAWER_VEIL) })
    if (model) parts.push({ key: 'note', html: String(cornerNote(model, base)) })
    if (model) parts.push({ key: 'tabbar', html: String(tabBar(model, base, view)) })
  }
  // The Desk and the Scribble Board are two sides of one sheet (ui.mjs, controller "curl"): the board is a part of
  // its own ("pad"), which the Desk keeps under its sheet once its corner was touched (padKept), so turning the
  // page mounts nothing anew; the corner itself is the part "curl".
  if (view === 'whiteboard') parts.push({ key: 'pad', html: String(main) })
  else parts.push({ key: 'main', html: String(main) })
  if (FRONT.has(view) && padKept && model && loaded.whiteboard) parts.push({ key: 'pad', html: String(loaded.whiteboard.whiteboardMain(model)) })
  boardInView = (FRONT.has(view) && padKept) || view === 'whiteboard' ? deskInView ?? read('trommi-desk-last') ?? ALL_DESKS : null
  if (FRONT.has(view) || view === 'whiteboard') parts.push({ key: 'curl', html: String(curlHTML(FRONT.has(view) ? 'desk' : 'pad', FRONT.has(view) ? `${base}/scribble-board` : `${base}${padFrom}`)) })
  if (demo) parts.push({ key: 'demo', html: '<span class="demo-band">Demo · <a href="/screens?mock=1" target="_blank" rel="noopener" title="All screens">All screens</a> · <a href="/?mock=0" data-turbo="false" title="Leave demo">leave</a></span>' })
  parts.push({ key: 'says', html: `<div class="says-host says-page" id="says-host" data-turbo-permanent>${says}</div>` })
  parts.push({ key: 'sheets', html: String(html`${keySheet()}${view === 'desk' ? rowSheet(base) : ''}`) })
  return parts
}

// ---- router ----
// Navigation, forms, frames and live updates of the app (what Turbo Drive, Frames and Streams do for server-rendered
// pages, done in the page):
//   - a link of the app (same origin) renders its page from the local model: no request leaves the device;
//   - the page's body is patched by parts (topbar, sidebar, main, …): a part whose markup did not change stays;
//   - a form is answered by the board's handlers (createBoard above): stream actions, a redirect, or a page;
//   - after every change of the core only the elements that changed are replaced (board.live → <turbo-stream>);
//   - fetch() calls of the controllers to the app's own JSON routes (/note, /desk, a card's draft) are answered here.

const STREAM_ACCEPT = 'text/vnd.turbo-stream.html, text/html, application/xhtml+xml'
const fire = (target, name, detail = {}, cancelable = false) => { const e = new CustomEvent(name, { bubbles: true, cancelable, detail }); target.dispatchEvent(e); return e }
const isAppPath = p => !/\.(?:css|js|mjs|json|png|svg|jpe?g|webp|gif|woff2?|webmanifest|html|txt|csv|log|ico)$/i.test(p) && !p.startsWith('/demo/') && !p.startsWith('/gen/') && !p.startsWith('/att/')

function createRouter({ board, onPage = () => {}, beforeVisit = () => {} }) {
  let page = null          // { path, client: { view, params }, opts }
  const parts = new Map()  // key -> { html, nodes: [Node] }

  // ---- painting a page ----
  // Each part stands between two comments (<!--p:key--> … <!--/p:key-->), so what a stream put in its place
  // still belongs to it.
  const between = part => { const out = []; for (let n = part.start; n; n = n.nextSibling) { out.push(n); if (n === part.end) break } return out }
  function paintBody(list, { morph = false } = {}) {
    const body = document.body
    // (every new part is parsed off the page first, in one go, each with its two marks: the page is then changed in
    //  one stretch of insertions and removals, nothing read in between)
    const made = new Map()
    for (const { key, html } of list) {
      const part = parts.get(key)
      if (part && (key === 'says' || part.html === html || (key === 'pad' && part.html !== html && padCanvas(part.html0) === padCanvas(html)))) continue
      const t = document.createElement('template')
      t.innerHTML = html
      const start = document.createComment(`p:${key}`), end = document.createComment(`/p:${key}`)
      t.content.prepend(start); t.content.append(end)
      made.set(key, { frag: t.content, start, end })
    }
    const keep = new Set(list.map(p => p.key))
    for (const [key, part] of parts) if (!keep.has(key)) { for (const n of between(part)) n.remove(); parts.delete(key) }
    let anchor = body.firstChild
    for (const { key, html } of list) {
      let part = parts.get(key)
      if (part && key === 'says') {
        // Toasts stay across pages (data-turbo-permanent); one the new page brings (?said=…) joins them on top.
        const t = document.createElement('template')
        t.innerHTML = html
        const fresh = t.content.firstElementChild, host = document.getElementById('says-host')
        if (fresh?.childNodes.length && host) host.prepend(...fresh.childNodes)
      }
      if (part && key === 'pad' && part.html !== html && padCanvas(part.html0) === padCanvas(html)) {
        // The same drawing: the mounted pad stays; only its list of sessions and the boards beside it are brought up to date.
        const t = document.createElement('template')
        t.innerHTML = html
        for (const id of ['whiteboard-sessions', 'whiteboard-desks']) { const fresh = t.content.getElementById(id); if (fresh) document.getElementById(id)?.replaceWith(fresh) }
        part.html = html
      }
      if (part && (key === 'says' || part.html === html)) {
        // Kept (toasts always stay across pages, like data-turbo-permanent): moved into place if needed.
        if (part.start !== anchor) for (const n of between(part)) body.insertBefore(n, anchor)
        anchor = part.end.nextSibling
        continue
      }
      const { frag, start, end } = made.get(key)
      // A part painted before whose markup changed a little (the session in view marked in the sidebar, a count): the
      // nodes it painted then stay, and only what differs is changed in them (morph). A row keeps its node, its
      // controller and its layout; the browser styles and lays out only what changed.
      const morphs = MORPHED.has(key) || (morph && key !== 'says')
      const pristine = frag.cloneNode(true)
      if (part?.pristine && part.html != null && morphs && morphPart(part, frag)) {
        part.html = html; part.html0 = html; part.pristine = pristine
        anchor = part.end.nextSibling
        continue
      }
      if (part) { anchor = part.end.nextSibling; for (const n of between(part)) n.remove() }
      if (anchor && !anchor.isConnected) anchor = null
      body.insertBefore(frag, anchor)
      parts.set(key, { html, html0: html, start, end, pristine })
      anchor = end.nextSibling
    }
  }
  // ---- morphing a part: keep the painted nodes, change what the new markup changes ----
  // The frame's parts that stay from page to page (the sidebar, the bars); a page's <main> is painted anew (its
  // controllers start from the top, its scroll too).
  const MORPHED = new Set(['agents', 'topbar', 'phonebar', 'tabbar', 'sheets', 'note'])
  // old: the live nodes between the part's marks; was: the markup they were painted from (parsed, untouched since);
  // now: the new markup, parsed. Where now equals was, the live node stays as it is (whatever controllers did to it).
  // Where only attributes differ, those attributes change and the children are compared the same way. Anything else
  // (another element, a node that controllers added or took away) is replaced by the new node. false: not morphed
  // (the part is painted anew).
  function morphNode(live, was, now) {
    if (now.isEqualNode(was)) return true
    if (live.nodeType !== now.nodeType || was.nodeType !== now.nodeType) return false
    if (now.nodeType === Node.TEXT_NODE || now.nodeType === Node.COMMENT_NODE) { if (live.data === was.data) live.data = now.data; else live.replaceWith(now); return true }
    if (now.nodeType !== Node.ELEMENT_NODE || live.tagName !== now.tagName || was.tagName !== now.tagName) return false
    // (a template's content, a form field's value, a frame: replaced whole)
    if (now.tagName === 'TEMPLATE' || now.tagName === 'TEXTAREA' || now.tagName === 'INPUT' || now.tagName === 'SELECT' || now.tagName.includes('-')) { live.replaceWith(now); return true }
    const lk = live.childNodes, wk = was.childNodes, nk = now.childNodes
    const same = lk.length === wk.length && wk.length === nk.length
    // (a list whose rows came or went: matched by their ids, where every row has one; else the element is replaced)
    if (!same && !(lk.length === wk.length && keyed(was) && keyed(now))) { live.replaceWith(now); return true }
    for (const a of new Set([...was.getAttributeNames(), ...now.getAttributeNames()])) {
      const w = was.getAttribute(a), n = now.getAttribute(a)
      if (w === n) continue
      if (n == null) live.removeAttribute(a); else live.setAttribute(a, n)
    }
    if (!same) return morphKeyed(live, was, now)
    const kids = [...lk], was2 = [...wk], now2 = [...nk]
    for (let i = 0; i < now2.length; i++) if (!morphNode(kids[i], was2[i], now2[i])) kids[i].replaceWith(now2[i])
    return true
  }
  /** Every child an element with an id (blank text between them aside). */
  const keyed = el => [...el.childNodes].every(n => (n.nodeType === Node.ELEMENT_NODE && n.id) || (n.nodeType === Node.TEXT_NODE && !n.data.trim()))
  function morphKeyed(live, was, now) {
    const pairs = new Map()
    const lk = [...live.childNodes], wk = [...was.childNodes]
    for (let i = 0; i < wk.length; i++) if (wk[i].nodeType === Node.ELEMENT_NODE) pairs.set(wk[i].id, { live: lk[i], was: wk[i] })
    for (const n of lk) if (n.nodeType === Node.TEXT_NODE) n.remove()
    let at = live.firstChild
    for (const n of [...now.childNodes]) {
      if (n.nodeType !== Node.ELEMENT_NODE) continue
      const p = pairs.get(n.id)
      pairs.delete(n.id)
      let el = n
      if (p) { if (morphNode(p.live, p.was, n)) el = p.live.isConnected ? p.live : n; else { p.live.replaceWith(n) } }
      if (el !== at) live.insertBefore(el, at)
      at = el.nextSibling
    }
    for (const p of pairs.values()) p.live.remove()
    return true
  }
  function morphPart(part, frag) {
    const live = between(part).slice(1, -1), was = [...part.pristine.childNodes].slice(1, -1), now = [...frag.childNodes].slice(1, -1)
    if (live.length !== was.length || was.length !== now.length) return false
    for (let i = 0; i < now.length; i++) if (!morphNode(live[i], was[i], now[i])) live[i].replaceWith(now[i])
    return true
  }
  // A part a stream changed no longer holds the markup it was painted with.
  const forget = streams => {
    for (const [, id] of String(streams).matchAll(/<turbo-stream action="[a-z]+" target="([^"]+)"/g)) {
      const el = document.getElementById(id)
      if (!el) continue
      for (const part of parts.values()) if (part.html != null && between(part).some(n => n === el || n.contains?.(el))) part.html = null
    }
  }
  function paint(path, opts, { scroll = 'top', morph = false } = {}) {
    fire(document, 'turbo:before-cache')
    document.title = opts.title ?? 'Trommi'
    const body = document.body
    body.dataset.view = 'chat'
    body.dataset.scope = opts.scope ?? 'all'
    body.dataset.tView = opts.view
    body.dataset.tBase = ''
    for (const a of [...body.attributes]) if (a.name.startsWith('data-') && !['data-view', 'data-scope', 'data-t-view', 'data-t-base'].includes(a.name)) body.removeAttribute(a.name)
    for (const [, name, value] of String(opts.bodyAttrs ?? '').matchAll(/([\w-]+)="([^"]*)"/g)) body.setAttribute(name, value)
    if (FRONT.has(opts.view)) padFrom = path
    padKept = opts.view === 'whiteboard' || (FRONT.has(opts.view) && padKept)
    paintBody(bodyParts({ ...opts, base: '' }), { morph })
    if (opts.sidebar !== false) markCurrent(opts.current ?? null)
    page = { path, client: { view: opts.view }, opts }
    if (morph) { fire(document, 'turbo:render'); fire(document, 'trommi:morph'); onPage(page); return }
    if (scroll === 'top') window.scrollTo(0, 0)
    else if (typeof scroll === 'number') requestAnimationFrame(() => window.scrollTo(0, scroll))
    // As Turbo does: the first [autofocus] element of the new page gets the keyboard.
    const auto = document.querySelector('main [autofocus], [autofocus]')
    if (auto && !auto.closest('[hidden], dialog:not([open])')) auto.focus({ preventScroll: true })
    fire(document, 'turbo:render')
    fire(document, 'turbo:load', { url: location.href })
    onPage(page)
  }

  // ---- visiting ----
  let visiting = 0
  async function visit(path, { action = 'advance', scroll } = {}) {
    const url = new URL(path, location.href)
    if (url.origin !== location.origin) { location.href = url.href; return }
    url.pathname = movedPath(url.pathname) ?? url.pathname   // (an address of before: paths.mjs)
    const to = url.pathname + url.search + url.hash
    if (action !== 'restore' && fire(document, 'turbo:before-visit', { url: url.href }, true).defaultPrevented) return
    const mine = ++visiting
    beforeVisit(url)
    await focusFor(url)   // (the core makes the screen's unit first: a session, a card)
    if (mine !== visiting) return
    const res = await board.request({ method: 'GET', path: url.pathname + url.search, headers: { accept: 'text/html' } })
    if (mine !== visiting) return
    if (res.kind === 'redirect') return visit(res.to, { action: action === 'restore' ? 'replace' : action === 'advance' ? 'replace-after' : action })
    if (action === 'advance') { saveScroll(); history.pushState({ trommi: true, scroll: 0 }, '', to) }
    else if (action === 'replace' || action === 'replace-after') { if (action === 'replace-after') saveScroll(); history[action === 'replace-after' ? 'pushState' : 'replaceState']({ trommi: true, scroll: 0 }, '', to) }
    if (res.kind === 'page') paint(to, res.opts, { scroll: scroll ?? (action === 'restore' ? history.state?.scroll ?? 0 : 'top') })
    else paint(to, { title: 'Not found · Trommi', view: 'missing', main: '<main id="inbox" aria-label="Not found"><header class="inbox-head"><div class="inbox-title"><h2>Not here.</h2><p><a href="/" data-nav>Back to the Desk</a></p></div></header></main>', vm: core.vm() })
    if (url.hash) document.getElementById(decodeURIComponent(url.hash.slice(1)))?.scrollIntoView({ block: 'center' })
  }
  const saveScroll = () => { try { history.replaceState({ ...(history.state ?? {}), trommi: true, scroll: window.scrollY }, '') } catch {} }
  /** The page in view, drawn again from the view model and morphed into the body (after the core's render). */
  async function refresh() {
    if (!page) return
    const res = await board.request({ method: 'GET', path: page.path, headers: { accept: 'text/html' } })
    if (res.kind === 'page') { const back = keepMenu(); paint(page.path, res.opts, { morph: true }); back() }
    else if (res.kind === 'redirect') visit(res.to, { action: 'replace' })
  }
  setVisitor((path, opts) => visit(path, { action: opts.action === 'replace' ? 'replace' : 'advance' }))
  setRefresher(refresh)
  addEventListener('popstate', () => visit(location.pathname + location.search + location.hash, { action: 'restore' }))
  if ('scrollRestoration' in history) history.scrollRestoration = 'manual'

  // ---- frames ----
  async function frameVisit(frame, path) {
    await focusFor(new URL(path, location.href))
    const res = await board.request({ method: 'GET', path, headers: { accept: 'text/html', 'turbo-frame': frame.id } })
    let markup = ''
    if (res.kind === 'html') markup = res.body
    else if (res.kind === 'page') markup = bodyParts({ ...res.opts, base: '' }).map(p => p.html).join('')
    else if (res.kind === 'redirect') return visit(res.to)
    const t = document.createElement('template')
    t.innerHTML = markup
    const fresh = t.content.querySelector(`turbo-frame#${globalThis.CSS.escape(frame.id)}`)
    if (!fresh) return visit(path)
    frame.replaceChildren(...fresh.childNodes)
    fire(frame, 'turbo:frame-load')
  }
  const frameOf = el => {
    const name = el.getAttribute('data-turbo-frame') ?? el.closest('form')?.getAttribute('data-turbo-frame')
    if (name === '_top') return null
    if (name) return document.getElementById(name)
    const frame = el.closest('turbo-frame')
    if (!frame || frame.getAttribute('target') === '_top') return null
    return frame
  }

  // ---- links ----
  document.addEventListener('click', e => {
    if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return
    const a = e.target instanceof Element ? e.target.closest('a[href]') : null
    if (!a || a.target && a.target !== '_self' || a.hasAttribute('download') || a.getAttribute('data-turbo') === 'false' || a.closest('[data-turbo="false"]')) return
    const url = new URL(a.href, location.href)
    if (url.origin !== location.origin || !isAppPath(url.pathname)) return
    if (url.pathname === location.pathname && url.search === location.search && url.hash) return   // a jump within the page
    e.preventDefault()
    const frame = frameOf(a)
    if (frame) {
      // A frame link with data-turbo-action also moves the address (a picture switched in place: ?pic=n survives a reload).
      const promote = a.getAttribute('data-turbo-action')
      return frameVisit(frame, url.pathname + url.search).then(() => { if (promote) { saveScroll(); history[promote === 'advance' ? 'pushState' : 'replaceState']({ trommi: true, scroll: window.scrollY }, '', url.pathname + url.search); if (page) page.path = url.pathname + url.search } })
    }
    visit(url.pathname + url.search + url.hash, { action: a.getAttribute('data-turbo-action') === 'replace' ? 'replace' : 'advance' })
  })

  // ---- tips ----
  // One tip for the whole app, in its own look (app.css ".tip"), never the browser's: what an element says in
  // data-tip, and what it says in title (taken over when the pointer first comes: the words move to data-tip, and to
  // aria-label where the element had no name). It comes after a short rest under the element, near the pointer on a
  // wide one, and goes with the pointer, a press, a key or a scroll. A mouse or a pen only; the keyboard's focus shows
  // it too. Not where a view draws its own from data-tip in CSS (the Scribble Board, the corner note).
  {
    const OWN = '.pad, .corner-note-head'
    let tip = null, of = null, timer = 0, px = 0
    const hide = () => { clearTimeout(timer); timer = 0; of = null; tip?.classList.remove('is-on') }
    const take = el => {
      const words = el.getAttribute('title')
      if (!words) return
      el.removeAttribute('title'); el.setAttribute('data-tip', words)
      if (!el.hasAttribute('aria-label') && !el.textContent.trim()) el.setAttribute('aria-label', words)
    }
    const show = () => {
      timer = 0
      const words = of?.isConnected ? of.getAttribute('data-tip') : ''
      if (!words) return hide()
      tip ??= document.body.appendChild(Object.assign(document.createElement('div'), { className: 'tip' }))
      tip.setAttribute('aria-hidden', 'true'); tip.textContent = words
      const r = of.getBoundingClientRect(), w = tip.offsetWidth, h = tip.offsetHeight, vw = document.documentElement.clientWidth, vh = document.documentElement.clientHeight
      const cx = r.width > 220 && px ? px : r.left + r.width / 2
      const below = r.bottom + 6 + h <= vh - 4 || r.top - 6 - h < 4
      tip.style.left = `${Math.round(Math.max(6, Math.min(vw - w - 6, cx - w / 2)))}px`
      tip.style.top = `${Math.round(below ? r.bottom + 6 : r.top - 6 - h)}px`
      tip.classList.add('is-on')
    }
    const aim = (el, wait) => {
      if (el === of) return
      hide()
      if (!el || el.closest(OWN)) return
      take(el)
      if (!el.getAttribute('data-tip')) return
      of = el; timer = setTimeout(show, wait)
    }
    const holder = t => t instanceof Element ? t.closest('[data-tip], [title]') : null
    document.addEventListener('pointerover', e => { if (e.pointerType === 'touch') return; px = e.clientX; aim(holder(e.target), 450) })
    document.addEventListener('pointermove', e => { px = e.clientX }, { passive: true })
    document.addEventListener('pointerdown', hide, true)
    document.documentElement.addEventListener('pointerleave', hide)
    document.addEventListener('keydown', hide, true)
    document.addEventListener('scroll', hide, { capture: true, passive: true })
    document.addEventListener('focusin', e => { const el = holder(e.target); if (el && e.target.matches?.(':focus-visible')) aim(el, 250) })
    document.addEventListener('focusout', hide)
    addEventListener('blur', hide)
  }

  // ---- forms ----
  async function submitForm(form, submitter) {
    const method = (submitter?.getAttribute('formmethod') ?? form.getAttribute('method') ?? 'get').toLowerCase()
    const action = new URL(submitter?.getAttribute('formaction') ?? form.getAttribute('action') ?? location.pathname, location.href)
    const data = new FormData(form, submitter ?? undefined)
    if (method === 'get') {
      const q = new URLSearchParams()
      for (const [k, v] of data) if (typeof v === 'string') q.append(k, v)
      const path = `${action.pathname}?${q}`
      const frame = frameOf(form)
      return frame ? frameVisit(frame, path) : visit(path)
    }
    const formSubmission = { formElement: form, submitter, method, location: action }
    fire(form, 'turbo:submit-start', { formSubmission })
    // A Desk row that is answered (or put off, thrown away, asked about) moves at once: the motion starts in this frame,
    // the answer's work (core, re-render) comes when the motion is done. If it
    // does not go through, the row comes back with what went wrong.
    const row = form.closest?.('#desk-list .inbox-row[data-id]')
    const leaves = row && form.hasAttribute('data-turbo-frame') === false && /\/(decide|trust|close|what|shred|snooze|revise)$/.test(action.pathname)
    const chosenRows = form.id === 'sel-bar' && data.get('way') !== 'later' ? String(data.get('ids') ?? '').split(',').filter(Boolean).map(id => document.getElementById(`row-${id}`)).filter(Boolean) : []
    const gone = leaves ? flipOut([row]) : chosenRows.length ? flipOut(chosenRows) : null
    if (gone?.length) await new Promise(r => setTimeout(r, calm() ? 0 : 260 + (chosenRows.length > 1 ? (chosenRows.length - 1) * 45 : 0)))
    // (a page visited while the form was on its way is where he is now: a late answer that leads elsewhere, as a
    // removal that is done only when the room's next change came, does not take him away from it)
    const seen = visiting
    let res, error = null
    try { res = await board.request({ method: 'POST', path: action.pathname + action.search, form: data, headers: { accept: STREAM_ACCEPT, referer: location.href } }) }
    catch (err) { error = err; console.error('form', err) }
    const success = !error && res && res.code < 400
    if (gone?.length && (!success || (res?.kind === 'stream' && [row, ...chosenRows].some(r => r?.id && res.body.includes(`action="replace" target="${r.id}"`))))) putBack(gone)
    if (res?.kind === 'stream') { forget(res.body); renderStreamMessage(res.body) }
    else if (visiting !== seen) {}
    else if (res?.kind === 'redirect') await visit(res.to)
    else if (res?.kind === 'page') { history.replaceState({ trommi: true }, '', location.href); paint(location.pathname + location.search, res.opts, { scroll: window.scrollY }) }
    fire(form.isConnected ? form : document, 'turbo:submit-end', { formSubmission, success, fetchResponse: res ? { response: { status: res.code } } : null, error })
  }
  document.addEventListener('submit', e => {
    if (e.defaultPrevented) return
    const form = e.target
    if (!(form instanceof HTMLFormElement) || (form.method === 'dialog') || form.getAttribute('data-turbo') === 'false') return
    const action = new URL(form.getAttribute('action') ?? location.pathname, location.href)
    if (action.origin !== location.origin) return
    e.preventDefault()
    submitForm(form, e.submitter)
  })

  // ---- the controllers' fetch() to the app's own routes ----
  const realFetch = window.fetch.bind(window)
  window.fetch = async (input, init = {}) => {
    const url = new URL(typeof input === 'string' || input instanceof URL ? String(input) : input.url, location.href)
    const method = (init.method ?? (input instanceof Request ? input.method : 'GET')).toUpperCase()
    if (url.origin !== location.origin || method !== 'POST' || !isAppPath(url.pathname)) return realFetch(input, init)
    if (String(new Headers(init.headers ?? {}).get('content-type') ?? '').includes('json')) {
      const res = await board.request({ method: 'POST', path: url.pathname + url.search, json: JSON.parse(String(init.body ?? '{}')), headers: { accept: 'application/json' } })
      if (res.kind === 'missing') return new Response('not found', { status: 404 })
      return new Response(res.code === 204 ? null : res.body || '{}', { status: res.code, headers: { 'Content-Type': 'application/json' } })
    }
    let form
    if (init.body instanceof FormData) form = init.body
    else { form = new FormData(); for (const [k, v] of new URLSearchParams(typeof init.body === 'string' || init.body instanceof URLSearchParams ? init.body : '')) form.append(k, v) }
    const res = await board.request({ method: 'POST', path: url.pathname + url.search, form, headers: { accept: String(new Headers(init.headers ?? {}).get('accept') ?? STREAM_ACCEPT) } })
    if (res.kind === 'missing') return new Response('not found', { status: 404 })
    return new Response(res.code === 204 ? null : res.body ?? '', { status: res.code, headers: { 'Content-Type': res.kind === 'stream' ? 'text/vnd.turbo-stream.html' : 'text/html' } })
  }

  /** The Scribble Board under the Desk's sheet, from now on (the corner was touched): mounted once, kept. */
  async function keepPad() {
    if (padKept || !FRONT.has(page?.opts.view)) return
    await view('whiteboard')
    if (padKept || !FRONT.has(page?.opts.view)) return
    padKept = true
    paintBody(bodyParts({ ...page.opts, base: '' }))
  }
  /** A page's main markup, rendered from the model without going there (the Desk under the Scribble Board's corner). */
  async function peek(path) {
    const res = await board.request({ method: 'GET', path, headers: { accept: 'text/html' } })
    return res.kind === 'page' ? String(res.opts.main) : ''
  }
  // (forget: every part is painted anew on the next page, as on a first load; the demo's /screens switches a frame's state so)
  const forgetAll = () => { for (const part of parts.values()) for (const n of between(part)) n.remove(); parts.clear() }
  return { visit, refresh, get page() { return page }, paint, keepPad, peek, forget: forgetAll }
}

// ---- push ----
// "Push on this device": the menu's bell (#push-toggle). On: asks for the permission (only a click may), subscribes
// with the hub's VAPID key and hands the subscription to the hub through the core. The hub pushes only
// { room_id, envelope_number, urgency }; the service worker shows a short line (public/sw.js).

// ---- pwa ----

function watchPwa() {
const meta = document.querySelector('meta[name="theme-color"]')
function paintBar() {
  if (!meta) return
  const bar = document.querySelector('.topbar') ?? document.body
  const colour = bar && getComputedStyle(bar).backgroundColor
  if (colour && colour !== 'rgba(0, 0, 0, 0)') meta.setAttribute('content', colour)
}
new MutationObserver(() => requestAnimationFrame(paintBar)).observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme', 'data-ready'] })
requestAnimationFrame(paintBar)

const sw = navigator.serviceWorker
if (sw) {
  const CHECK_MS = 10 * 60e3
  let checked = Date.now(), stale = false
  const hadController = Boolean(sw.controller)   // the first install also "takes over": that is no new version
  const unsent = () => [...document.querySelectorAll('textarea, input[type="text"], input:not([type])')].some(f => f.value.trim() && !f.closest('[hidden]'))
  function offer() {
    if (document.querySelector('.app-update')) return
    const box = document.createElement('div')
    box.className = 'app-update'
    box.setAttribute('role', 'status')
    const go = document.createElement('button')
    go.type = 'button'
    go.textContent = 'Reload'
    go.addEventListener('click', () => location.reload())
    box.append('A new version is ready.', go)
    document.body.append(box)
  }
  // A new version took over: a page in view is never reloaded under the person; a quiet line offers it, and it
  // reloads by itself once the app is in the background with nothing unsent. A page in the background reloads now.
  sw.addEventListener('controllerchange', () => { if (!hadController) return; stale = true; if (document.visibilityState === 'hidden' && !unsent()) location.reload(); else offer() })
  const check = async () => {
    if (Date.now() - checked < CHECK_MS) return
    checked = Date.now()
    try { await (await sw.getRegistration())?.update() } catch {}
  }
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'hidden') { if (stale && !unsent()) location.reload(); return }
    check()
  })
  // (a desktop tab that stays in view all day never turns hidden: it looks for a new version on the same clock)
  setInterval(() => { if (document.visibilityState === 'visible') check() }, CHECK_MS)
}
}

// ---- the core's toasts ----
// What the core says in passing (a refusal, "Note sent" with its Undo) stands with the page's own toasts in
// #says-host: one per toast of the view model, gone when the model drops it. Its button posts /toast/<id> (the action
// it offers), its time runs out into Dismiss.
const shown = new Map()   // toast id -> its element
const toastPath = s => s?.screen === 'session' ? `/chat/${encodeURIComponent(s.session_id)}` : s?.screen === 'card' ? `/card/${encodeURIComponent(s.card_id)}` : s?.screen === 'settings' ? '/settings' : s?.screen === 'artifacts' ? '/artifacts' : s?.screen === 'off' ? '/stacks/off' : '/'
function syncToasts(vm) {
  const now = new Map((vm.toasts ?? []).map(x => [x.id, x]))
  for (const [id, el] of shown) if (!now.has(id)) { el.remove(); shown.delete(id) }
  const host = document.getElementById('says-host')
  if (!host) return
  for (const [id, x] of now) {
    if (shown.has(id)) continue
    const ms = x.ms ?? (x.action ? 8000 : 5000)
    host.insertAdjacentHTML('afterbegin', String(toast({
      head: x.text.en, line: x.line?.en ?? '', role: x.alert ? 'alert' : 'status', ms,
      undo: x.action ? { action: `/toast/${encodeURIComponent(id)}`, label: x.label?.en ?? 'Undo' } : null,
      link: x.go ? { href: toastPath(x.go.screen), label: x.go.label.en } : null,
    })))
    const el = host.firstElementChild
    shown.set(id, el)
    setTimeout(() => core.act({ do: 'dismiss', toast_id: id }), ms + 1000)
  }
}
/** The app's own routes: a toast's action, the desk switch. */
const appView = { register(t) {
  t.post(/^\/toast\/(.+)$/, ({ res, match }) => {
    const id = decodeURIComponent(match[1]), x = core.vm().toasts?.find(y => y.id === id)
    if (x?.action) t.act(x.action)
    t.act({ do: 'dismiss', toast_id: id })
    t.done(res)
  })
  // The menu's "switch desk" (/?desk=<id>) and /desk/<id>: the desk is this browser's; the address is the Desk's again.
  t.get(/^\/$/, ({ res, url }) => { const d = url.searchParams.get('desk'); if (d == null) return false; t.setDesk(d); t.redirect(res, '/') })
  t.get(/^\/desk\/([\w-]+)$/, ({ res, match }) => { t.setDesk(match[1]); t.redirect(res, '/') })
  t.get(/^\/scribble-board$/, ({ res, url }) => { const d = url.searchParams.get('desk'); if (d == null) return false; t.setDesk(d); t.redirect(res, '/scribble-board') })
} }

// ---- the line on the connection ----
const PHASE_WORDS = { live: ['online', 'Connected'], opening: ['connecting', 'Connecting'], starting: ['connecting', 'Connecting'], offline: ['offline', 'No connection'], stopped: ['offline', 'Stopped'], signed_out: ['offline', 'Signed out'] }
function conn(vm) {
  const el = document.getElementById('conn'), text = document.getElementById('conn-text')
  const [state, words] = PHASE_WORDS[vm.phase] ?? ['connecting', 'Connecting']
  if (el) el.dataset.state = state
  if (text) text.textContent = vm.status?.en ?? words
}

// ---- one tab ----
// One tab holds this browser's device (the core's worker owns its state). Another tab says so and offers to take it
// over; the tab that gave it up says that it did.
function elsewhere(head, line) {
  document.title = 'Trommi'
  document.body.replaceChildren()
  document.body.insertAdjacentHTML('beforeend', String(html`<main id="room" class="room room-elsewhere" aria-label="${head}"><div class="room-card"><h1 class="room-title">${head}</h1><p class="room-lead">${line}</p><button type="button" class="room-go" id="use-here">Use it here</button></div></main>`))
  document.getElementById('use-here').addEventListener('click', () => location.reload(), { once: true })
}

// ---- boot ----
// The app's start: the core worker opens this browser's device (or the demo room with ?mock=1); the page in view is
// drawn from the first render the core sends, then drawn again (morphed) with every render after it.
let T0 = 0, demo = null

/** What the core needs to know at its start (core/client/src/app/event.rs Config). */
const configOf = fixture => ({
  platform: 'web', version: APP_VERSION, hub: hubUrl(), seed: Math.floor(Math.random() * 2 ** 52),
  utc_offset_min: -new Date().getTimezoneOffset(), locale: navigator.language ?? 'en',
  can: { notify: false, secrets: false, snapshots: false, passkeys: passkeysOn(), push: pushable() ? 'web_push' : '' },
  ...(fixture ? { demo: fixture } : {}),
})

async function start({ take = false } = {}) {
  // (the demo only: the review page of all screens, and ?state= clicks: demo/demo.mjs)
  const demoPages = demo ? await import('./demo/demo.mjs') : null
  if (demoPages) { demoPages.screensController({ Controller, controller }); const state = new URLSearchParams(location.search).get('state'); if (state) document.addEventListener('turbo:load', () => demoPages.demoState(state), { once: true }) }
  const b = createBoard({ views: [appView, ...(demoPages ? [demoPages.screensView] : []), ...VIEWS] })
  const router = createRouter({ board: b })
  window.trommi = { board: b, router, vm: core.vm, act: core.act, demo: Boolean(demo), view, ...(demoPages ? { demoState: demoPages.demoState } : {}) }
  let frame = 0, painted = false
  const repaint = () => { frame ||= requestAnimationFrame(() => { frame = 0; if (painted) router.refresh(); conn(core.vm()) }) }
  core.on('render', vm => { syncToasts(vm); repaint() })
  core.on('file', repaint)
  core.on('busy', () => elsewhere('Trommi is open in another tab', 'One tab holds this device at a time. Use it here, and the other tab steps back.'))
  core.on('taken', () => elsewhere('Trommi moved to another tab', 'Another tab took this device over. Use it here to take it back.'))
  core.on('failed', err => { console.error('core', err); notice('Trommi could not start here.', String(err?.message ?? err), false, 'failed') })
  let fixture = null
  if (demo) fixture = await fetch('/demo/fixture.json').then(r => r.json()).catch(() => null)
  screenInView = screenOf(location.pathname, location.search)
  core.startCore(configOf(fixture), { take })
  sendFocus()
  // The first page waits for the core to say where it stands (stored, signed out, the demo), a moment at most.
  await new Promise(done => { const t = setTimeout(done, 3000); const stop = core.on('render', vm => { if (vm.phase && vm.phase !== 'starting') { clearTimeout(t); stop(); done() } }) })
  window.trommi.firstRenderMs = performance.now() - T0
  await router.visit(location.pathname + location.search + location.hash, { action: 'replace' })
  painted = true
  window.trommi.firstPaintMs = performance.now() - T0
  window.trommi.readyAt = performance.now()
  document.documentElement.dataset.ready = ''
  fontsAfterPaint()
  // The other views, once the page is idle: the next navigation (a card, a session, the board) finds them in memory.
  const idle = globalThis.requestIdleCallback ?? (f => setTimeout(f, 300))
  idle(() => { for (const name of Object.keys(LAZY)) view(name).catch(err => console.warn('view', name, err)) }, { timeout: 3000 })
  conn(core.vm())
  return router
}

// ---- fonts ----
// The web fonts come after the first paint (a cold start on a slow line spends its bandwidth on the app first): the page
// paints in the metric-matched fallbacks of fonts/fallback.css, then fonts/fonts.css is added (its fonts swap in,
// font-display: swap; the service worker keeps them for later starts). At the latest FONTS_MS after boot.
const FONTS_MS = 3000
let fontsLoaded = false
function loadFonts() {
  if (fontsLoaded) return
  fontsLoaded = true
  const link = document.createElement('link')
  link.rel = 'stylesheet'
  link.href = '/fonts/fonts.css'
  document.head.append(link)
}
const fontsAfterPaint = () => requestAnimationFrame(() => setTimeout(loadFonts, 0))

/** The page starts here (index.html loads this module; importing it elsewhere, as tests in Node do, does nothing). */
async function boot() {
  T0 = performance.now()
  // An address of before (/s/…, /a/…; paths.mjs): the address bar shows its new form from the start.
  const moved = movedPath(location.pathname)
  if (moved) history.replaceState(history.state, '', `${moved}${location.search}${location.hash}`)
  setTimeout(loadFonts, FONTS_MS)
  // The service worker: the app shell offline, push (public/sw.js). Not on the dev server (dev/serve.mjs, the
  // preview: html[data-build="dev"]): there a worker installed before is taken away.
  if (document.documentElement.dataset.build === 'dev') navigator.serviceWorker?.getRegistrations().then(list => list.forEach(r => r.unregister())).catch(() => {})
  else if ('serviceWorker' in navigator) navigator.serviceWorker.register('/sw.js').catch(err => console.warn('service worker', err.message))
  startUi()
  defineTurbo()
  watchPwa()
  // The demo room: ?mock=1 (the fixture); remembered for this tab; ?mock=0 ends it.
  const params = new URLSearchParams(location.search)
  if (params.has('mock')) { if (params.get('mock') === '0') sessionStorage.removeItem('trommi-mock'); else sessionStorage.setItem('trommi-mock', params.get('mock') || '1') }
  demo = sessionStorage.getItem('trommi-mock')
  if (params.has('mock')) { params.delete('mock'); history.replaceState(history.state, '', `${location.pathname}${params.size ? `?${params}` : ''}${location.hash}`) }
  document.documentElement.classList.toggle('is-demo', Boolean(demo))
  // A link for someone outside the room (/artifact/<share_id>#…): its own small page, no room needed.
  if (SHARE_PAGE.test(location.pathname)) return (await view('media')).showShare()
  // (the demo's account screens, /screens: ?mock=1&onboard=<state> draws one of them; no account, nothing sent)
  const onboard = demo && params.get('onboard')
  if (onboard) { fontsAfterPaint(); return (await view('auth')).demoScreen(onboard) }
  keepStorage()
  await start()
}
/** A share page: `/artifact/<share id>` (22 characters of base64url; 32 hex in the links of before). */
const SHARE_PAGE = /^\/artifact\/(?:[0-9a-f]{32}|[A-Za-z0-9_-]{22})$/
if (typeof window !== 'undefined') boot()

/** Ask the browser to keep this origin's storage (no eviction under storage pressure; Safari weighs it too). */
function keepStorage() { try { navigator.storage?.persist?.().catch(() => {}) } catch {} }

/** A calm full-width line at the foot (styled by auth.css), with "Reload". update: fetch the new build first. */
export function notice(text, detail, update, why = '') {
  if (document.querySelector('.room-notice')) return
  const box = document.createElement('div')
  box.className = 'room-notice'
  if (why) box.dataset.why = why
  box.setAttribute('role', 'status')
  const words = document.createElement('span')
  words.textContent = text
  if (detail) words.title = detail
  const go = document.createElement('button')
  go.type = 'button'
  go.className = 'room-notice-go'
  go.textContent = 'Reload'
  go.addEventListener('click', async () => {
    go.disabled = true
    if (update) { try { const reg = await navigator.serviceWorker?.getRegistration(); await reg?.update() } catch {} }
    location.reload()
  })
  box.append(words, go)
  document.body.append(box)
}
