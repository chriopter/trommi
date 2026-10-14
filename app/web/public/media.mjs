// ---- artifacts ----
// What the agents published (Artifacts), drawn from the app core's view model:
//   GET  /artifacts                  the gallery (vm.artifacts, the newest first; ?kind=media | pages: a filter of this page)
//   GET  /artifacts/<id>             one Artifact in the app's own viewer (?from=<talk row>: the way back into its session)
//   POST /artifacts/share/<file>     "Copy link": the file's Share link (Action::Share; vm.files[id].shares), as JSON
//   POST /artifacts/unshare/<file>   "Stop sharing" (Action::RevokeShare)
// The share page /artifact/<share id> for people outside the room (showShare) is drawn here too.
import { Controller, agoSpan, assetGlyph, controller, copyLater, html, mediaPreview, raw, showToast, sk, smallMark, whoOf } from './ui.mjs'

/** How a file is drawn, by its media type: 'image', 'html' (a page), 'video', 'audio' or 'file'. */
export const typeOf = file => {
  const mt = String(file?.media_type ?? '')
  return mt.startsWith('image/') ? 'image' : mt === 'text/html' ? 'html' : mt.startsWith('video/') ? 'video' : mt.startsWith('audio/') ? 'audio' : 'file'
}
const LABEL = { html: 'Page', image: 'Picture', video: 'Video', audio: 'Audio', file: 'File' }
export const sizeText = n => (n >= 1e6 ? `${(n / 1e6).toFixed(1)} MB` : n >= 1e3 ? `${Math.round(n / 1e3)} kB` : `${n} B`)

/** Waits until the view model holds what `test` finds (a link the core made, a file uploaded), at most `ms`; null then. */
export async function until(vm, test, ms = 15000) {
  for (const end = Date.now() + ms; Date.now() < end; await new Promise(r => setTimeout(r, 100))) { const x = test(vm()); if (x) return x }
  return null
}

// ---- the share control: a link for people outside the room (Action::Share; how long it holds is the core's) ----
const daysLeft = sh => Math.max(1, Math.ceil((sh.expires_ms - Date.now()) / 86400_000))
const days = sh => `${daysLeft(sh)} ${daysLeft(sh) === 1 ? 'day' : 'days'}`
/** The button (and Stop sharing) for the file `id` (FileView `file`); tile: the round icons on an Artifacts tile. */
export function shareControl(id, file, title, { tile = false, cls = '' } = {}) {
  if (!id || !file) return ''
  const sh = file.shares?.[0] ?? null, on = sh ? ' is-on' : '', off = sh ? '' : raw(' hidden')
  const head = html`data-controller="sharelink" data-sharelink-file-value="${id}"`
  if (tile) return html`<span class="shr is-tile${on}" ${head}><button type="button" class="art-act shr-copy" data-action="sharelink#copy" title="${sh ? `${days(sh)} left` : 'Copy link'}" aria-label="Copy link to ${title}" aria-pressed="${sh ? 'true' : 'false'}">${sk('link')}</button><details class="art-share t-pick" data-controller="pops"${off}><summary class="art-act" title="More" aria-label="More for ${title}">${sk('more')}</summary><div class="art-share-body"><span class="shr-word">${sh ? `Shared · ${days(sh)} left` : ''}</span><button type="button" class="shr-stop" data-action="sharelink#stop">Stop sharing</button></div></details></span>`
  return html`<span class="shr${on}${cls ? ` ${cls}` : ''}" ${head}><button type="button" class="asset-copy shr-copy" data-action="sharelink#copy" title="${sh ? `${days(sh)} left` : 'Copy link'}" aria-pressed="${sh ? 'true' : 'false'}"><span class="shr-word">${sh ? `Shared · ${days(sh)}` : 'Copy link'}</span></button><button type="button" class="shr-stop" data-action="sharelink#stop"${off}>Stop sharing</button></span>`
}
const post = path => fetch(path, { method: 'POST', headers: { 'Content-Type': 'application/json', Accept: 'application/json' }, body: '{}' })
controller('sharelink', class extends Controller {
  static values = { file: String }
  async copy() {
    if (this.busy) return
    this.busy = true
    // (the clipboard is asked at once, inside the press, and given the link when the core made it: Safari wants it so)
    let said = null
    const link = post(`/artifacts/share/${encodeURIComponent(this.fileValue)}`).then(async r => { said = await r.json(); if (!r.ok) throw new Error(said.error); return said.link })
    let ok = false
    try { ok = await copyLater(link) } catch (err) { this.busy = false; return showToast({ head: 'Not shared', line: err.message, role: 'alert' }) }
    this.busy = false
    if (!ok) return showToast({ head: 'Not copied', line: 'It is shared, but the browser kept the clipboard closed. Press again', role: 'alert' })
    showToast({ head: 'Link copied', line: said?.expires_ms ? `Valid for ${days(said)}` : '' })
  }
  stop() { post(`/artifacts/unshare/${encodeURIComponent(this.fileValue)}`) }
})

// ---- an Artifact as the talk announces it, and in its viewer ----
const thumbAttrs = (type, url) => (type === 'html' && url ? html` data-controller="assetthumb" data-assetthumb-src-value="${url}"` : '')
/** The card of something a session published, in its talk: what it is, Open (the viewer) and Copy link. The talk row
 *  names only the Artifact's id and its words (no title, no file: a core gap), so the card says those. */
export function artifactCard({ id, title = '', note = '', fileId = null, file = null, url = null, from = '' }, base) {
  const type = typeOf(file), view = `${base}/artifacts/${id}${from ? `?from=${encodeURIComponent(from)}` : ''}`
  const kind = ['Artifact', file ? LABEL[type] : '', file?.size ? sizeText(file.size) : ''].filter(Boolean).join(' · ')
  return html`<div class="asset-slot"><div class="asset-card has-preview"><a class="asset-preview${type === 'image' && url ? ' is-shown' : ''}" data-kind="${type}" data-nav href="${view}" tabindex="-1" aria-hidden="true"${thumbAttrs(type, url)}>${type === 'image' && url ? html`<img src="${url}" alt="" loading="lazy" decoding="async">` : assetGlyph(type)}${type === 'html' ? raw('<span class="asset-page-label">Page</span>') : ''}</a><div class="asset-text"><span class="caps">${kind}</span><strong>${title || note || 'Untitled'}</strong>${title && note ? html`<span class="asset-note">${note}</span>` : ''}</div><div class="asset-actions"><a class="asset-open" data-nav href="${view}">Open</a>${shareControl(fileId, file, title || 'Untitled')}</div></div></div>`
}

/** The viewer: the published thing at its own address, as large as the page lets it be. */
function viewer(a, vm, t, from) {
  const fileId = a?.files?.[0] ?? null, file = fileId ? vm.files?.[fileId] : null, url = fileId ? t.fileUrl(fileId) : null
  const type = typeOf(file), title = a?.title || 'Untitled'
  const back = a?.session_id ? `${t.BASE}/chat/${encodeURIComponent(a.session_id)}${from ? `#msg-${from}` : ''}` : `${t.BASE}/artifacts`
  const arrow = raw('<svg viewBox="0 0 24 24" class="focus-icon" aria-hidden="true"><path d="M19 12H5M11 6l-6 6 6 6"/></svg>')
  const stage = !a || !file ? html`<p class="as-problem">This is no longer available.</p>`
    : !url ? html`<div class="as-frame-box"><p class="as-wait">Opening…</p></div>`
      : type === 'html' ? html`<div class="as-frame-box" data-controller="assetthumb" data-assetthumb-src-value="${url}" data-assetthumb-full-value="true"><p class="as-wait">Opening the page…</p></div>`
        : type === 'image' ? html`<img class="as-media" src="${url}" alt="${title}" decoding="async">`
          : type === 'video' ? html`<video class="as-media" src="${url}#t=0.001" controls playsinline preload="metadata"></video>`
            : type === 'audio' ? html`<audio class="as-media" src="${url}" controls preload="metadata"></audio>`
              : html`<div class="as-file"><span class="as-file-name">${file.name}</span>${file.size ? html`<span class="caps">${sizeText(file.size)}</span>` : ''}<a class="as-btn" href="${url}" download="${file.name}">Download</a></div>`
  return html`<div class="t-picture as-view">
<header class="t-picture-bar"><a class="focus-back-desk t-picture-back" data-nav href="${back}" aria-label="Back">${arrow}<span>${a?.session || 'Artifacts'}</span></a><span class="t-picture-where"><b>${LABEL[type]}</b> ${title}</span>${shareControl(fileId, file, title, { cls: 'as-copy' })}</header>
<div class="as-stage" data-type="${type}">${stage}</div>
</div>`
}

// ---- the gallery: a tile per Artifact (the picture in a soft frame, one line under it: the session's small drawing,
// the title, when; Open and Copy link as small round icons on it) ----
// (the session's drawing: its row in the sidebar; ArtifactView names no mark)
const whoBy = (vm, a) => whoOf(vm.sidebar?.sessions?.find(s => s.id === a.session_id) ?? { id: a.session_id ?? a.id, mark: '', name: a.session })
function tile(a, vm, t) {
  const fileId = a.files?.[0] ?? null, file = fileId ? vm.files?.[fileId] : null, type = typeOf(file)
  const href = `${t.BASE}/artifacts/${a.id}`, title = a.title || 'Untitled'
  const go = cls => html`data-nav href="${href}" class="${cls}"`
  return html`<li class="art-item art-card" id="art-${a.id}" data-kind="${type === 'html' ? 'pages' : 'media'}"><a ${go('art-shot')} title="${title}">${mediaPreview({ type, url: fileId ? t.fileUrl(fileId) : null, name: file?.name ?? '' })}</a>
<div class="art-line"><span class="gal-who" title="${a.session}">${smallMark(whoBy(vm, a))}</span><a ${go('art-t')}>${title}</a>${agoSpan(a.sent_at, 'ago art-ago')}</div><div class="art-acts"><a ${go('art-act')} title="Open" aria-label="Open ${title}">${sk('go')}</a>${shareControl(fileId, file, title, { tile: true })}</div></li>`
}
const KINDS = [['', 'All'], ['media', 'Media'], ['pages', 'Pages']]
function gallery(vm, t, kind) {
  const all = vm.artifacts ?? [], list = kind ? all.filter(a => (typeOf(vm.files?.[a.files?.[0]]) === 'html') === (kind === 'pages')) : all
  return html`<main id="gallery" class="gal-page lk-page" aria-label="Artifacts"><div class="gal-column">
<header class="gal-head page-head"><h2>Artifacts</h2><p>${all.length ? 'Every picture, video, file and page your agents made, the newest first.' : 'Nothing yet: pictures, videos, pages and files your agents send show up here.'}</p></header>${all.length ? html`<nav class="art-kinds" aria-label="Kind">${KINDS.map(([k, word]) => html`<a data-nav href="${t.BASE}/artifacts${k ? `?kind=${k}` : ''}"${kind === k ? raw(' aria-current="true"') : ''}>${word}</a>`)}</nav>
<ol class="art-grid" id="artifacts-list">${list.length ? list.map(a => tile(a, vm, t)) : html`<li class="gal-none" id="art-none">${kind === 'pages' ? 'No pages yet.' : 'No pictures, videos or files yet.'}</li>`}</ol>` : ''}
</div></main>`
}

export function register(t) {
  t.get(/^\/artifacts$/, ({ req, res, url }) => {
    const k = url.searchParams.get('kind'), kind = k === 'media' || k === 'pages' ? k : ''
    t.page(req, res, { title: 'Artifacts · Trommi', view: 'artifacts', bodyAttrs: ' data-page="gallery"', main: gallery(t.vm(), t, kind) })
  })
  t.get(/^\/artifacts\/([0-9a-f]{8,64})$/, ({ req, res, url, match }) => {
    const vm = t.vm(), a = (vm.artifacts ?? []).find(x => x.id === match[1]) ?? null
    t.page(req, res, { title: `${a?.title || 'Artifact'} · Trommi`, view: 'picture', sidebar: false, bodyAttrs: ' data-focus-page="card"', main: viewer(a, vm, t, url.searchParams.get('from') ?? '') })
  })
  t.post(/^\/artifacts\/share\/([\w-]+)$/, async ({ res, match }) => {
    const id = match[1], file = t.vm().files?.[id]
    if (!file) return t.json(res, { error: 'this file is not here' }, 422)
    if (!file.shares?.length) t.act({ do: 'share', attachment_id: file.attachment_id ?? id })
    const sh = await until(t.vm, v => v.files?.[id]?.shares?.[0])
    if (!sh) return t.json(res, { error: 'the board did not answer' }, 422)
    t.json(res, sh)
  })
  t.post(/^\/artifacts\/unshare\/([\w-]+)$/, ({ res, match }) => {
    for (const sh of t.vm().files?.[match[1]]?.shares ?? []) t.act({ do: 'revoke_share', share_id: sh.share_id })
    t.done(res)
  })
}

// ---- share view ----
// The page for people outside the room: https://app.trommi.com/artifact/<share_id>#<share_secret>.<file_key>.<sha256>
// (the repository's README, "links for people outside the room"). Everything after # stays in this browser. Opening
// the link is the core's (Action::OpenShared: the file comes as FileReady under the id "shared", or a toast says why
// not). `core`: the page side of a core started without a device ({ act, on, fileUrl }, core.mjs); app.mjs does not
// start one for this page yet, and then the page says it cannot open the link.
const el = (tag, props = {}, ...kids) => { const n = Object.assign(document.createElement(tag), props); n.append(...kids); return n }
function frame(text) {
  const f = el('iframe', { className: 'share-frame', src: '/frame', title: 'Shared page' })
  f.setAttribute('sandbox', 'allow-scripts')
  addEventListener('message', function ready(e) { if (e.source === f.contentWindow && e.data === 'ready') { removeEventListener('message', ready); f.contentWindow.postMessage({ html: text }, '*') } })
  return f
}
export function showShare(core = null) {
  document.title = 'Shared · Trommi'
  const main = el('main', { id: 'share', className: 'room share' })
  document.body.replaceChildren(main)
  const say = (text, cls) => main.replaceChildren(el('p', { className: cls, textContent: text }))
  if (!core) return say('Shared links cannot be opened in this version of Trommi yet.', 'room-error')
  say('Opening…', 'room-wait')
  core.on('render', vm => { const x = vm.toasts?.find(y => y.alert); if (x) say(x.text.en, 'room-error') })
  core.on('file', async id => {
    if (id !== 'shared') return
    const url = core.fileUrl(id), blob = await (await fetch(url)).blob()
    if (blob.type === 'text/html') return main.replaceChildren(frame(await blob.text()))
    main.replaceChildren(blob.type.startsWith('image/') ? el('img', { className: 'share-picture', src: url, alt: 'Shared picture' }) : el('p', { className: 'room-lead' }, el('a', { href: url, download: 'trommi-file', textContent: 'Download the file' })))
  })
  core.act({ do: 'open_shared', link: location.origin + location.pathname + location.hash })
}
