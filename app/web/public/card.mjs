// A card's own page: inside the board's frame, the sidebar beside it. The card is one calm
// block in two columns: at the left the title, one quiet line (who, when, Nr.), the short text and a
// picture; at the right the answers (or what was said), and under the card the comments and the field to write in.
// Everything it shows is the app core's CardView (views.d.ts: vm.cards[id]); this module only draws it and turns what
// is pressed into the core's actions (register below). The comments hold what the card does not: the whole text,
// the links of options, versions and what happened to the card, the talk.
// Above the card stands one row of round drawn buttons, at the card's edges: the way back, a cross, at the left; at
// the right the question before and the next, Focus (the page alone in the window, ?focus=1; a picture opens large
// by a click on it) and More, three dots: Wake up, Copy, Shred. Where it stands ("3 of 9") is in the card's own line.
// Later is a pull-tag tied under the card's bottom-right corner: pulled, the card is put off and the next one follows.
//
// One form (#card-form-<id>) holds the field and the notes on single options; every way to answer is a button of that
// form with its own address (formaction), so what was written goes along. The controller "card" adds the pencil for
// a note on one option, the draft kept while typing, Enter that sends, files that are pasted or dropped, and the pen's
// arrow from the picture to its option. Styles: card.css.
import { Controller, PLUS, avatar, fileTile, isTyping, letterKeysOn, LATER_TAG, SETTLED, WORDS, agoSpan, arrowStrokes, controller, doodleSvg, el, fileSrc, finalSign, html, linkNote, raw, revokeTiles, rich, sideWays, sk, sketch, useFiles, whoOf } from './ui.mjs'
const icon = d => raw(`<svg viewBox="0 0 24 24" class="tc-icon" aria-hidden="true"><path d="${d}"/></svg>`)
const ARROW_L = 'M19 12H5M11 6l-6 6 6 6', ARROW_R = 'M5 12h14M13 6l6 6-6 6', TICK = 'M5 12.5l4.5 4.5L19 7.5', PLAY = 'M9 6.5v11l9-5.5z', GROW = 'M14 5h5v5M19 5l-6 6M10 19H5v-5M5 19l6-6'
// Drawn with the pen, for the round buttons above a card: an arrow to the left and one to the right, a cross, three
// dots, four corners pulled apart (full screen).
const pen = paths => raw(`<svg viewBox="0 0 24 24" class="sketch" aria-hidden="true">${paths}</svg>`)
const BACK = pen('<path d="M19.4 12.3 Q12.2 11.5 5 12.1"/><path d="M10.9 6 Q7.7 9.3 4.7 12.1 Q8 14.8 11.2 18.2"/>')
const FORTH = pen('<path d="M4.6 12.3 Q11.8 11.5 19 12.1"/><path d="M13.1 6 Q16.3 9.3 19.3 12.1 Q16 14.8 12.8 18.2"/>')
const CROSS = pen('<path d="M6.3 6.6 Q12.2 12.1 17.8 17.7"/><path d="M17.6 6.2 Q12 12.2 6.2 17.9"/>')
const DOTS = pen('<path d="M5.4 12 Q5.9 11.5 6.4 12 Q5.9 12.6 5.4 12 M11.5 12 Q12 11.5 12.5 12 Q12 12.6 11.5 12 M17.6 12 Q18.1 11.5 18.6 12 Q18.1 12.6 17.6 12" stroke-width="2.6"/>')
const FULL = pen('<path d="M4.4 9.3 Q4.1 6.6 4.5 4.4 Q6.9 4.1 9.4 4.4"/><path d="M14.7 4.2 Q17.3 4.5 19.6 4.3 Q19.9 6.7 19.6 9.2"/><path d="M19.8 14.8 Q19.5 17.4 19.7 19.7 Q17.2 19.9 14.8 19.6"/><path d="M9.3 19.8 Q6.7 19.5 4.3 19.7 Q4.1 17.2 4.4 14.9"/>')
// Leave focus: the four corners turned inwards
const LESS = pen('<path d="M4.3 9.2 Q6.8 9.5 9.3 9.2 Q9.6 6.7 9.3 4.3"/><path d="M14.8 4.4 Q14.5 6.9 14.8 9.3 Q17.3 9.6 19.7 9.3"/><path d="M19.6 14.8 Q17.2 14.5 14.7 14.8 Q14.4 17.3 14.7 19.7"/><path d="M9.2 19.6 Q9.5 17.2 9.2 14.7 Q6.7 14.4 4.3 14.7"/>')
/** The words of the focus button, off and on (the controller "card" switches them in place). */
const FOCUS_WORDS = { false: 'Focus', true: 'Leave focus · Esc' }
/** An address with ?focus=1 on it or off it (the card page's focus mode lives in the address). */
const focusAt = (href, on) => { const u = new URL(href, globalThis.location?.origin ?? 'http://x'); if (on) u.searchParams.set('focus', '1'); else u.searchParams.delete('focus'); return u.pathname + u.search + u.hash }
const FOCUS_FIELD = '<input type="hidden" name="focus" value="1">'
const DRAW = pen('<path d="M4.6 19.6 Q5 17.6 5.6 15.9 Q10.6 10.8 16 5.2 Q17.6 4 18.9 5.3 Q20 6.6 18.7 8 Q13.3 13.4 8.2 18.5 Q6.5 19.2 4.6 19.6"/><path d="M14.4 6.9 Q15.8 8.1 17.1 9.6"/>')
const COPY_ICON = raw('<svg viewBox="0 0 24 24" class="sketch cardclip-ico" aria-hidden="true"><path d="M9.2 8.6C12.6 8.3 16 8.4 19.3 8.7C19.7 12.2 19.6 15.8 19.4 19.4C16 19.8 12.6 19.7 9.1 19.5C8.7 16 8.8 12.4 9 9"/><path d="M15 5.6C14.8 4.9 14.3 4.5 13.6 4.5C10.9 4.3 8.2 4.4 5.4 4.6C4.8 4.7 4.5 5.1 4.5 5.7C4.3 8.4 4.3 11.2 4.6 14C4.7 14.6 5.1 14.9 5.8 15"/></svg>')
// The pen's circle round an answer in the talk, as wide as its tile.
const RING = raw('<svg class="tc-deed-ring" viewBox="0 0 100 40" preserveAspectRatio="none" aria-hidden="true"><path d="M9 23 C5 9 28 3.4 54 4 C80 4.6 96.4 10 94.6 21 C92.6 33 68 37 45 36.2 C21 35.4 4.6 31 8.4 17 C10 11.6 16 8 25 6.2" vector-effect="non-scaling-stroke"/></svg>')
const BACK_ARROW = raw('<svg viewBox="0 0 24 24" class="sketch tc-undo-ico" aria-hidden="true"><path d="M9.8 5.6 Q6.8 8.4 4.3 11.2 Q7.1 13.8 10 16.6"/><path d="M4.9 11.1 Q10.4 10.4 14.9 11.2 Q19.8 12.5 19.5 16.4 Q19 19.5 15 19.7"/></svg>')
// The pen line under the strip of work: a faint stroke drawn by hand, and a short darker one that runs along it.
const PEN_RUN = raw('<svg class="tc-work-run" viewBox="0 0 200 8" preserveAspectRatio="none" aria-hidden="true"><path class="run-base" d="M2 4.6 Q30 2.4 60 4.4 T120 3.8 T198 4.2"/><path class="run-pen" d="M2 4.6 Q30 2.4 60 4.4 T120 3.8 T198 4.2" pathLength="100"/></svg>')
const URGENCY = ['low', 'normal', 'high', 'critical']

/** A card's address (CardView, CardRefView, or an id) under base: by its number where known, else its id. */
export const cardPath = (card, base = '') => `${base}/card/${encodeURIComponent(typeof card === 'string' ? card : card.number || card.id)}`
/** A form's address for one way of answering: POST /cards/<id>/<what> (register below). */
const act = (card, what) => `/cards/${card.id}/${what}`
/** The card in view by its id or number (the address's), or null. */
export const cardIn = (vm, ref) => vm.cards?.[ref] ?? Object.values(vm.cards ?? {}).find(c => String(c.number) === String(ref)) ?? null

// ---- files ----
const fileOf = (vm, id) => vm.files?.[id] ?? { name: '', media_type: '', size: 0 }
const sizeWord = n => (n >= 1e6 ? `${(n / 1e6).toFixed(1)} MB` : n >= 1e3 ? `${Math.round(n / 1e3)} kB` : `${n} B`)
/** The src attribute of a file's picture or video, once its bytes are here (the page is drawn again when they come). */
const srcAt = (id, suffix = '') => { const url = fileSrc(id); return url ? raw(` src="${url}${suffix}"`) : '' }
/** A card's (or a version's) pictures, videos and other files: { pictures, videos, others } (ids, PictureViews). */
function mediaOf(vm, { pictures = [], files = [] }) {
  const pics = new Set(pictures.map(p => p.file_id)), kind = id => fileOf(vm, id).media_type.split('/')[0]
  const rest = files.filter(id => !pics.has(id))
  return { pictures, videos: rest.filter(id => kind(id) === 'video'), others: rest.filter(id => kind(id) !== 'video' && kind(id) !== 'image') }
}
/** One attached file, to open: the clip, its name, its size. The same on the card and in the talk. */
const fileChip = (vm, id) => { const f = fileOf(vm, id), url = fileSrc(id); return html`<a class="tc-file"${url ? html` href="${url}" download="${f.name}"` : ''} target="_blank" rel="noopener" title="Open ${f.name}">${sk('clip')}<b>${f.name}</b>${f.size > 0 ? html`<i>${sizeWord(f.size)}</i>` : ''}</a>` }

// ---- the picture stage ----
/** Where one stands among the pictures and videos: "2 / 6", at the end of the bar of small ones. */
const count = (i, n) => (n > 1 ? html`<b class="tc-count" data-card-target="where" aria-label="${i} of ${n}">${i} / ${n}</b>` : '')
/** The small pictures to pick from: the one that stands is marked; the pointer on one shows it (controller "card").
 *  Each lies on a ground with a drawn picture, so one that has not come yet is not a hole. */
const strip = (pictures, i, to) => html`<span class="tc-strip" data-action="pointerover->card#peek focusin->card#peek pointerleave->card#unpreview focusout->card#unpreview">${pictures.map((p, n) => html`<a class="tc-thumb" data-nav href="${to(n + 1)}" data-turbo-action="replace" data-at="${n + 1}" title="${p.title ?? p.name}" aria-label="Show picture ${n + 1}: ${p.name}" aria-pressed="${String(n + 1 === i)}">${sk('picture')}<img${srcAt(p.file_id)} alt="" loading="lazy" decoding="async" draggable="false" width="48" height="34"></a>`)}</span>`
/** The videos' small tiles, after the pictures: the same tile with a play mark. */
const clips = (vm, videos, i, to, from) => videos.map((id, n) => html`<a class="tc-thumb tc-thumb-video" data-nav href="${to(from + n + 1)}" data-turbo-action="replace" title="${fileOf(vm, id).name}" aria-label="Show video ${n + 1}: ${fileOf(vm, id).name}" aria-pressed="${String(from + n + 1 === i)}">${icon(PLAY)}</a>`)
/** The files that are neither picture nor video, as tiles of the same bar after them: a click opens the file. */
const attTile = (vm, id) => { const f = fileOf(vm, id), url = fileSrc(id); return html`<a class="tc-thumb tc-thumb-file"${url ? html` href="${url}" download="${f.name}"` : ''} target="_blank" rel="noopener" title="${f.name}${f.size > 0 ? ` · ${sizeWord(f.size)}` : ''}" aria-label="Open ${f.name} in a new tab">${sk('clip')}<b>${f.name}</b></a>` }
/** A picture's shape on the stage, from its size: one more than a quarter taller than wide (TALL) is not fitted into
 *  the stage but stands at its width and is scrolled; a phone's screenshot (SLIM) is kept narrow. */
const TALL = 1.25, SLIM = 1.8
const shapeOf = (w, h) => (w > 0 && h > 0 && h / w > TALL ? (h / w >= SLIM ? ['is-tall', 'is-slim'] : ['is-tall']) : [])
/** The picture's shown width: its own, never wider than its place (a phone's screenshot is not blown up). */
const ownWidth = p => (p.width > 0 ? raw(` style="width:${Number(p.width)}px"${p.height > 0 ? ` width="${Number(p.width)}" height="${Number(p.height)}"` : ''}`) : '')
/** The page behind a picture (the HTML it was rendered from), as one thing to press; it stands hidden where the
 *  picture has none (the controller shows it for one that has). */
const pageChip = (vm, page, view) => html`<a class="page-chip" data-card-target="page" data-nav href="${page ? view : '#'}" title="Open page"${page ? '' : raw(' hidden')}>${sk('page')}<b>${page ? fileOf(vm, page).name : ''}</b><i>open</i></a>`

/** What the controller "card" needs to put another picture on the stage (the pointer on an option or on a small
 *  picture): per picture its address, its size, the option it belongs to, its name and caption, its page. */
const picturesOf = (pictures, vm, here) => pictures.map((p, i) => ({ at: i + 1, src: fileSrc(p.file_id) ?? '', width: p.width ?? null, height: p.height ?? null, name: p.name, title: p.title ?? '', page: p.page ? { view: `${here}/picture/${i + 1}/page`, name: fileOf(vm, p.page).name } : null, marks: p.marks ?? [], key: p.key ?? null, keys: p.keys ?? (p.key != null ? [p.key] : []), href: `${here}/picture/${i + 1}` }))

/** The picture of the card: one at a time on a stage of ONE fixed size, the same for every picture and video of the
 *  card, so no hover, pick or late picture moves anything. A tall one (shapeOf) stands at its width and is scrolled
 *  inside the stage. A click on it opens it large on its own page. On the stage's lower edge: its caption and the page
 *  behind it. Under the stage ONE bar: the pictures and videos as small ones to pick (?pic= counts on through the
 *  videos), the other files as tiles, and which one stands ("2 / 6"). A frame of its own, so picking another loads
 *  only this. A video stands on the stage as a player (never autoplays): the way to the large view is a small button
 *  in the stage's corner. */
function cardMedia(id, media, vm, here, at = 1, query = '') {
  const { pictures, videos, others } = media, all = pictures.length + videos.length
  if (!all) return others.length ? html`<div class="tc-media"><div class="tc-thumbs" aria-label="Attached files"><span class="tc-roll">${others.map(f => attTile(vm, f))}</span></div></div>` : ''
  const i = Math.min(Math.max(1, at), all), video = i > pictures.length
  const to = n => `${here}?pic=${n}${query}`
  const step = (n, cls, label, d) => (all > 1 ? html`<a class="tc-step ${cls}" data-nav href="${to(n)}" data-turbo-action="replace" aria-label="${label}">${icon(d)}</a>` : '')
  let shown
  if (video) {
    const v = videos[i - pictures.length - 1], name = fileOf(vm, v).name
    shown = html`<figure class="tc-video"><video${srcAt(v, '#t=0.001')} controls playsinline preload="metadata" aria-label="Video ${i} of ${all}: ${name}"></video></figure><a class="tc-enlarge" data-nav href="${here}/picture/${i}" data-turbo-frame="_top" title="Enlarge" aria-label="Enlarge video ${i - pictures.length}: ${name}">${icon(GROW)}</a>`
  } else {
    const p = pictures[i - 1]
    shown = html`<div class="tc-scroll"><a class="tc-figure is-wait ${shapeOf(p.width, p.height).join(' ')}" draggable="false" data-nav href="${here}/picture/${i}" data-turbo-frame="_top" data-card-target="figure" data-at="${i}" title="Open it large" aria-label="Picture ${i} of ${pictures.length}: ${p.name}. Open it large"${p.key != null ? html` data-key="${p.key}"` : ''} data-controller="circles" data-circles-marks-value="${JSON.stringify(p.marks ?? [])}"><img alt=""${srcAt(p.file_id)}${ownWidth(p)} decoding="async" draggable="false"></a></div>
<span class="tc-said" data-card-target="said"${p.title ? '' : raw(' hidden')}>${p.title ?? ''}</span>${pageChip(vm, p.page, `${here}/picture/${i}/page`)}`
  }
  return html`<turbo-frame id="card-media-${id}" class="tc-media">
<div class="tc-stage${video ? ' is-video' : ''}" data-at="${i}">${shown}${step(i > 1 ? i - 1 : all, 'is-prev', 'The one before', ARROW_L)}${step(i < all ? i + 1 : 1, 'is-next', 'The next one', ARROW_R)}</div>
${all > 1 || others.length ? html`<div class="tc-thumbs"><span class="tc-roll">${all > 1 ? html`${strip(pictures, i, to)}${clips(vm, videos, i, to, pictures.length)}` : ''}${others.map(f => attTile(vm, f))}</span>${count(i, all)}</div>` : ''}
</turbo-frame>`
}

// ---- the left column: who, title, one quiet line, the text, the picture, the options explained ----
/** Who a card comes from, above its title: the session's drawing and where it stands, desk › main session › itself,
 *  its name in bold; a link to the session. */
function fromLine(from, base) {
  const who = whoOf(from), path = from.path?.length ? from.path : [from.name], name = path.at(-1)
  return html`<a class="tc-fromline" data-nav href="${base}/chat/${encodeURIComponent(from.session_id)}" style="--hue:${who.hue}" aria-label="From ${path.join(', ')}">${raw(doodleSvg(from.mark))}${path.slice(0, -1).map(p => html`<span>${p}</span><i aria-hidden="true">›</i>`)}<b>${name}</b></a>`
}
/** The strip above the title while the card's session works on it (CardWorkView): its drawing redrawing itself,
 *  "<session> is working on it", its status line and how long ago that was said, the pen line under it. */
const workStrip = w => (w ? html`<div class="tc-work" role="status">${avatar(whoOf(w), { crown: false, working: true })}<span class="tc-work-words"><b>${w.says.en}</b><span>${w.words.en}${w.at_ms ? html` · ${agoSpan(w.at_ms, 'ago')}` : ''}</span></span>${PEN_RUN}</div>` : '')
/** The words of the left column (#card-lead-<id>). old: a version before (VersionView), read only. */
function cardLead(card, base, old = null) {
  const notes = [...(card.notes ?? []).map(n => n.en), old ? `version ${old.n}, as it was` : ''].filter(Boolean)
  const text = old ? old.text ?? '' : card.text
  return html`<div class="tc-lead focus-lead" id="card-lead-${card.id}">
${card.knock ? html`<span class="inbox-tab tc-knock">${sk('knock')}${card.knock.en}</span>` : ''}
${old ? '' : workStrip(card.work)}${card.from ? fromLine(card.from, base) : ''}<h1 class="tc-title" id="card-title-${card.id}">${old?.title || card.title}</h1>
<p class="tc-meta">${agoSpan(card.asked_ms, 'tc-ago')}<span aria-hidden="true">·</span><span>${card.nr.en}</span>${card.place && !old ? html`<span aria-hidden="true">·</span><span class="tc-count" title="Its place">${card.place.says.en}</span>` : ''}${notes.length ? html`<span aria-hidden="true">·</span><a href="#card-thread-${card.id}">${notes.join(' · ')}</a>` : ''}</p>
${text ? html`<div class="tc-text">${rich(text)}</div>` : ''}${card.more && !old ? html`<a class="tc-more-text" href="#card-whole-${card.id}">More in the comments ↓</a>` : ''}
</div>`
}
/** The options' own paragraphs (SectionView: a card written as sections), at the end of the left column after the
 *  pictures (#card-say-<id>), one block per option in the order of the tiles. The pointer on a tile (or the keyboard)
 *  marks its block and draws the pen's arrow from the tile to it (controller "card", say). */
export function optionsSaid(card) {
  const secs = card.sections ?? []
  if (!secs.length) return html`<div id="card-say-${card.id}" hidden></div>`
  return html`<section class="tc-say" id="card-say-${card.id}" aria-label="The options explained">${secs.map(s => html`<div class="tc-say-one" data-key="${s.key}"><h3 class="tc-say-head"><span class="tc-say-label">${s.label}</span>${s.recommended ? html` <span class="tc-advised-word">recommended</span>` : ''}</h3>${s.text || s.html ? html`<div class="tc-say-text">${rich(s.text, { extra: s.html ?? '', hand: false })}</div>` : ''}</div>`)}</section>`
}

// ---- the right column: the answers, or what was said ----
// The reverse card: one press hands the card back at once, "put this before me again" (HandBack rework). A button of
// the card's form: what stands unsent in the field and what is drawn on the card go along.
const reviseTile = card => html`<button class="tc-tile tc-reverse" type="submit" form="card-form-${card.id}" formaction="${act(card, 'revise')}" name="next" value="1" title="${WORDS.revise} (B)" aria-label="${WORDS.revise}">${sk('reverse')}</button>`
const whatTile = (card, cls = 'tc-tile tc-wtf is-what') => html`<button class="${cls}" type="submit" form="card-form-${card.id}" formaction="${act(card, 'what')}" title="${WORDS.what} (E)" aria-label="What?? Explain this to me: the session explains it, and it comes back explained">${sk('what')}</button>`
const still = (label, detail = '', cls = '') => html`<div class="tc-opt is-still ${cls}"><span class="tc-opt-words"><span class="tc-opt-label">${label}</span>${detail ? html`<span class="tc-opt-detail">${detail}</span>` : ''}</span></div>`
/** The way back from what was said (AnswerView.back): a quiet link with the pen's return arrow. */
const backWay = (card, way) => (way ? html`<button class="tc-undo" type="submit" form="card-form-${card.id}" formaction="${act(card, way)}">${BACK_ARROW}<span>${WORDS.takeBack}</span></button>` : '')
/** The line under what was said: who did it and when, and the way back at its right end. */
const saidFoot = (card, a) => (a.did ? html`<p class="tc-ans-foot"><span>${a.did.en}</span>${a.at_ms ? html`<i aria-hidden="true">·</i>${agoSpan(a.at_ms, 'ago')}` : ''}${backWay(card, a.back)}</p>` : '')
/** The answer that stands: the tile in the accent, the pen's tick at its left. */
const stood = (label, detail = '') => html`<div class="tc-opt is-still is-picked">${sk('tick')}<span class="tc-opt-words"><span class="tc-opt-label">${label}</span>${detail ? html`<span class="tc-opt-detail">${detail}</span>` : ''}</span></div>`
/** Just answered on its own page (?decided=1): the card stays, answered, and offers the next open question
 *  (CardView.after), or says there is none, and a way out. */
function nextStep(after, { self, home, focus }) {
  return html`<nav class="tc-next" aria-label="What next">${after.next ? html`<a class="tc-next-go" data-nav href="${focusAt(cardPath(after.next, self), focus)}" title="${after.next.title}"><span>${after.says.en}</span><small>${after.next.title}</small>${FORTH}</a>` : html`<p class="tc-next-none">${sk('tick')}<span>${after.says.en}</span></p>`}<a class="tc-next-close" data-nav href="${home}">Close</a></nav>`
}
/** The right column's answers (#card-answer-<id>), by AnswerView.mode. old: a version before (read only). */
function cardAnswer(card, { old = null, pic = 1, after = null, self = '' } = {}) {
  const form = `card-form-${card.id}`, a = card.answer
  const box = inner => html`<div class="tc-answer" id="card-answer-${card.id}"><p class="tc-error" role="alert" hidden></p>${inner}${old ? '' : linkNote(card.note)}</div>`
  if (old) return box(html`<div class="tc-opts">${(old.options ?? []).map(o => still(o.label, o.detail ?? ''))}</div><p class="tc-quiet">Version ${old.n} cannot be answered. <a data-nav href="${cardPath(card, self)}">The question as it stands now</a></p>`)
  const words = x => x?.en ?? ''
  switch (a.mode) {
    case 'said': return box(html`<div class="tc-opts">${stood(words(a.label), words(a.detail))}${saidFoot(card, a)}${(a.option_notes ?? []).map(n => still(n.label, n.note.en))}${a.settled ? still(html`${sk('tick')}${SETTLED}`, a.settled.en, 'is-settled') : ''}${a.summary ? still('Done by the agent', a.summary) : ''}</div>${after ? nextStep(card.after ?? { says: { en: '' } }, after) : ''}${a.finished ? html`<button class="tc-tile is-what tc-done-what" type="submit" form="${form}" formaction="${act(card, 'what')}" title="${WORDS.what}">${sk('what')}</button>` : ''}`)
    case 'revising': return box(html`<div class="tc-opts">${still(words(a.label), words(a.detail))}${saidFoot(card, a)}</div>`)
    case 'working': return box(html`<div class="tc-opts">${still(words(a.label), words(a.detail))}</div>`)
    // An info: Got it is its answer; at the column's foot What?? and the reverse card, as on a decision.
    case 'read': return box(html`<div class="tc-or tc-info-ways"><button class="tc-tile is-ack" type="submit" form="${form}" formaction="${act(card, 'close')}" title="${WORDS.ack}">${sk('tick')}<span>${WORDS.ack}</span></button><div class="tc-or-pair">${whatTile(card, 'tc-tile tc-wtf is-what')}${reviseTile(card)}</div></div>`)
    case 'options': case 'request': break
    default: return box(html`<p class="tc-quiet">${words(a.says ?? a.detail ?? a.label)}</p>`)   // catching, loading, newer
  }
  const decision = card.kind === 'decision', shownKey = card.pictures?.[pic - 1]?.key
  const way = a.mode === 'request' ? 'verdict' : 'decide'
  const option = o => {
    // (A link written into an option is not on its tile: the tile says it in plain words, the link stands in the comments.)
    const words = html`<span class="tc-opt-words"><span class="tc-opt-label"${o.recommended ? raw(' data-controller="advice"') : ''}>${o.label}</span>${o.detail ? html`<span class="tc-opt-detail">${o.detail}</span>` : ''}${o.recommended ? raw('<span class="tc-sr">, recommended by the agent</span>') : ''}</span>${o.settles ? finalSign(true) : ''}`
    const pencil = decision ? html`<span class="tc-opt-pen" data-key="${o.key}" data-action="click->card#note" title="Note">${sk('pen')}</span>` : ''
    const marks = html`${o.tip ? html` title="${o.tip.en}"` : ''}${shownKey === o.key ? raw(' data-match') : ''}`
    const tile = card.multiple
      ? html`<label class="tc-opt${o.recommended ? ' is-advised' : ''}" data-key="${o.key}"${marks}><input type="checkbox" id="tick-${card.id}-${o.key}" name="keys" value="${o.key}" form="${form}" data-action="change->card#keep"><span class="tc-tick" aria-hidden="true">${icon(TICK)}</span>${words}${pencil}</label>`
      : html`<button class="tc-opt${o.recommended ? ' is-advised' : ''}" type="submit" form="${form}" formaction="${act(card, way)}" name="key" value="${o.key}" data-key="${o.key}"${marks}>${words}${pencil}</button>`
    return decision ? html`${tile}<label class="tc-opt-note" data-note="${o.key}" hidden>${sk('pen')}<input type="text" name="note-${o.key}" form="${form}" maxlength="2000" placeholder="A note on ${o.label}" aria-label="A note on ${o.label}" autocomplete="off" data-action="input->card#keep blur->card#noteLeft keydown->card#noteKey"></label>` : tile
  }
  const trustTip = 'Duck it (R)'
  return box(html`<div class="tc-opts" data-action="pointerover->card#preview focusin->card#preview pointerleave->card#unpreview focusout->card#unpreview" role="group" aria-label="${words(a.says)}">${card.options.map(option)}${card.multiple ? html`<button class="tc-opt tc-send-many" type="submit" form="${form}" formaction="${act(card, way)}"><span class="tc-opt-words"><span class="tc-opt-label">Send the answer</span></span></button>` : ''}</div>
${decision ? html`<div class="tc-or"><i>or</i><button class="tc-opt tc-whatever" type="submit" form="${form}" formaction="${act(card, 'trust')}" title="${trustTip}" aria-label="${trustTip}">${sk('duck')}<span class="tc-opt-words"><span class="tc-opt-label">${WORDS.trust}</span></span></button><div class="tc-or-pair">${whatTile(card)}${reviseTile(card)}</div></div>` : ''}`)
}

// ---- the comments under the card (#card-thread-<id>) ----
// Sober, as a conversation is: no drawings here. First what the card does not hold (CardView.lead: the whole text,
// the links of the options, that he put it off), then the talk and what happened to the card (CardView.talk, the
// window of the newest rows; "Load earlier" asks the core for a page more). What?? and the words that answer it stand
// together as one block; each act is the thing itself at its moment: the session's sheet (asked), a version's sheet
// with its number (and the version before it to open), the reverse card (handed back), the answer's tile in the
// pen's circle, the duck, the tick (read), the shredder, Later's tag, the sheet with a tick (closed). Every child of
// the feed has an id, so a new comment is added where it belongs and nothing else is drawn anew.
export function cardThread(card, vm, base = '') {
  const from = card.from, who = from ? whoOf(from) : null, name = from?.name ?? ''
  const pics = files => (files ?? []).filter(id => fileOf(vm, id).media_type.startsWith('image/'))
  const vids = files => (files ?? []).filter(id => fileOf(vm, id).media_type.startsWith('video/'))
  const rest = files => (files ?? []).filter(id => !/^(image|video)\//.test(fileOf(vm, id).media_type))
  const shots = files => { const p = pics(files); return p.length ? html`<div class="shots">${p.map(id => { const url = fileSrc(id), n = fileOf(vm, id).name; return html`<a class="tc-shot"${url ? html` href="${url}"` : ''} target="_blank" rel="noopener" title="Open full size" aria-label="Open ${n} full size"><img${srcAt(id)} alt="${n}" loading="lazy" decoding="async" width="220" height="165"></a>` })}</div>` : '' }
  // (a video in the talk, the clip an agent makes after What??, plays where it is said, never by itself)
  const clipsOf = files => { const v = vids(files); return v.length ? html`<div class="tc-vids">${v.map(id => html`<figure class="tc-vid"><video${srcAt(id, '#t=0.001')} controls playsinline preload="metadata" aria-label="Video: ${fileOf(vm, id).name}"></video></figure>`)}</div>` : '' }
  const chips = files => { const r = rest(files); return r.length ? html`<p class="tc-files">${r.map(id => fileChip(vm, id))}</p>` : '' }
  const along = files => html`${shots(files)}${clipsOf(files)}${chips(files)}`
  const did = (text, ts, id = '') => html`<p class="tc-did"${id ? html` id="${id}"` : ''}><span>${text}</span>${ts ? agoSpan(ts, 'msg-time') : ''}</p>`
  // An act: where a message has its who lies the thing the app draws for it, beside it one quiet line (what, when),
  // under it what was written along. tiles: an answer's options, before the line.
  const deed = (id, thing, cap, ts, { tiles = '', said = [], more = '', cls = '' } = {}) => html`<div class="tc-deed ${cls}"${id ? html` id="${id}"` : ''}><span class="tc-deed-thing" aria-hidden="true">${thing}</span><div class="tc-deed-in">${tiles}<p class="tc-deed-cap"><span>${cap}</span>${ts ? agoSpan(ts, 'msg-time') : ''}</p>${said.filter(Boolean).map(t => html`<p class="tc-deed-said">${t}</p>`)}${more}</div></div>`
  const uno = html`<span class="tc-uno">${sk('reverse')}</span>`
  const sheet = (inner, cls = '') => html`<span class="tc-sheet ${cls}">${inner}</span>`
  const read = html`<span class="tc-read">${sk('tick')}</span>`
  const tiles = (labels, back = false) => html`<span class="tc-picked-row">${labels.map(l => html`<span class="tc-picked"><b>${l}</b>${back ? '' : RING}</span>`)}</span>`
  const mark = who ? html`<span class="tc-c-who" style="--hue:${who.hue}" aria-hidden="true">${raw(doodleSvg(who.mark))}</span>` : html`<span class="tc-c-who" aria-hidden="true"></span>`
  const you = html`<span class="tc-c-who is-you" aria-hidden="true">${sk('pen')}</span>`
  const head = (title, ts, by = name) => html`<header class="msg-head"><span class="msg-name">${by}</span>${title ? html`<b class="tc-said">${title}</b>` : ''}${ts ? agoSpan(ts, 'msg-time') : ''}</header>`
  const agentMsg = (r, cont = false) => html`<article class="msg msg-agent tc-c${cont ? ' cont' : ''}" id="msg-${r.id}">${cont ? '' : mark}<div class="tc-c-in">${cont ? '' : head('', r.time_ms)}${rich(r.text, { extra: r.html ?? '', hand: false })}${along(r.files)}${r.details ? html`<details class="msg-details"><summary>Details</summary>${rich(r.details, { hand: false })}</details>` : ''}</div></article>`
  const userMsg = r => html`<article class="msg msg-user tc-c" id="msg-${r.id}">${you}<div class="tc-c-in">${head('', r.time_ms, 'You')}${r.text ? html`<div class="bubble"><p>${r.text}</p></div>` : ''}${along(r.files)}</div></article>`
  // The version before a revision, readable in place (title, text, options, pictures), and to open as it was.
  const before = n => {
    const v = card.versions?.find(x => x.n === n)
    if (!v) return ''
    return html`<details class="tc-turn-before"><summary>See version ${v.n}</summary><div class="tc-was"><b class="tc-was-title">${v.title || card.title}</b>${v.text ? rich(v.text, { hand: false }) : ''}${v.options?.length ? html`<div class="tc-opts">${v.options.map(o => still(o.label, o.detail ?? ''))}</div>` : ''}${shots((v.pictures ?? []).map(p => p.file_id))}${chips(v.files)}<a class="tc-was-open" data-nav href="${cardPath(card, base)}?v=${v.n}">Open version ${v.n} as it was</a></div></details>`
  }
  /** One act on the card (EventView), drawn as the thing itself. r: its row (id, time, files). */
  const act1 = (e, r) => {
    const id = `ev-${r.id}`, ts = r.time_ms, said = (e.said ?? []).map(t => t.en), label = e.label.en
    switch (e.kind) {
      case 'asked': case 'info': return deed(id, sheet(who ? raw(doodleSvg(who.mark)) : ''), label, ts, { cls: 'is-asked' })
      case 'revised': return html`<div class="tc-turn" id="${id}" data-version="${e.version ?? ''}">${deed('', sheet(html`<b>${e.version ?? ''}</b>`, 'is-again'), html`<b>${label}${e.asked ? ', as you asked' : ''}</b>`, ts, { said: [e.text], more: e.version > 1 ? before(e.version - 1) : '' })}</div>`
      case 'decided': return deed(id, sk('tick'), `${label}${e.version ? ` in version ${e.version}` : ''}`, ts, { tiles: e.text ? tiles([e.text]) : '', said, more: along(e.files), cls: 'is-chosen' })
      case 'trusted': return deed(id, sk('duck'), html`${label}${e.text ? html`: <b>${e.text}</b>` : ''}`, ts, { said, more: along(e.files), cls: 'is-duck' })
      case 'read': return deed(id, read, label, ts)
      case 'shredded': return deed(id, sk('shred'), label, ts, { said, more: along(e.files), cls: 'is-shred' })
      case 'reopened': return deed(id, sk('back'), label, ts, { cls: 'is-undone' })
      case 'done': return deed(id, sheet(sk('tick'), 'is-done'), html`<b>${label}</b>`, ts, { said: [e.text] })
      case 'snoozed': return deed(id, LATER_TAG, label, ts, { cls: 'is-later' })
      case 'took_back': return deed(id, uno, label, ts, { cls: 'is-undone' })
      case 'handed': case 'handed_back': return deed(id, uno, label, ts, { said: [r.text], more: along(r.files) })
      case 'explain': return deed(id, sk('what'), label, ts, { cls: 'is-what' })
      default: return did(label, ts, id)
    }
  }
  // ---- what the card does not hold ----
  const lead = (card.lead ?? []).map((l, i) => {
    if (l.kind === 'whole' || l.kind === 'with') return html`<article class="msg msg-agent tc-c tc-whole" id="card-whole-${card.id}">${mark}<div class="tc-c-in">${head(l.label.en)}${rich(l.text ?? '', { extra: l.html ?? '', hand: false })}</div></article>`
    if (l.kind === 'links') return html`<div class="tc-did t-opt-links" id="card-links-${card.id}"><span>${l.label.en}</span><ul>${(l.links ?? []).map(x => html`<li><b>${x.label}</b> <a href="${x.url}" target="_blank" rel="noopener noreferrer">${x.short}</a></li>`)}</ul></div>`
    if (l.kind === 'later') return deed(`card-later-${card.id}`, LATER_TAG, l.label.en, l.at_ms, { cls: 'is-later' })
    return did(l.label.en, 0, `card-lead-${i}-${card.id}`)
  })
  // ---- the talk ----
  const rows = card.talk ?? []
  const answers = new Map()   // What?? row id -> the agent's rows that answer it
  for (const r of rows) if (r.answers) answers.set(r.answers, [...(answers.get(r.answers) ?? []), r])
  const items = []
  for (const r of rows) {
    if (r.answers && rows.some(x => x.id === r.answers)) continue
    const e = r.event ?? r.deed
    if (e?.kind === 'explain') {
      const got = answers.get(r.id) ?? []
      items.push(html`<section class="tc-explained" id="explained-${r.id}">${act1(e, r)}${r.deed && r.text ? html`<p class="tc-deed-said">${r.text}</p>` : ''}${got.length ? got.map((x, n) => agentMsg(x, n > 0)) : r.waits ? did(r.waits.en) : ''}</section>`)
    } else if (r.kind === 'event' || r.deed) items.push(e ? act1(e, r) : did(r.text, r.time_ms, `ev-${r.id}`))
    else if (r.kind === 'user' || r.kind === 'note') items.push(userMsg(r))
    else if (r.kind === 'agent') items.push(agentMsg(r, r.cont))
    else if (r.text) items.push(did(r.text, r.time_ms, `ev-${r.id}`))
  }
  const earlier = card.earlier ? html`<form class="tc-earlier" id="card-earlier-${card.id}" method="post" action="${act(card, 'more')}"><button type="submit" class="tc-load-earlier">${card.earlier.says.en}</button></form>` : ''
  // (the session's sheet that opens the comments stands only above something: a card nobody wrote under shows none)
  const any = lead.length || card.earlier || rows.some(r => r.event?.kind !== 'asked' && r.event?.kind !== 'info')
  return html`<section class="tc-feed" id="card-thread-${card.id}" aria-label="Comments"${any ? '' : raw(' hidden')}>${earlier}${lead}${items}</section>`
}

/** The star beside the way back: his own mark on a card, to find it again in Off your mind. */
export const starButton = card => html`<form class="tc-star-form" id="card-star-${card.id}" method="post" action="${act(card, card.starred ? 'unstar' : 'star')}"><input type="hidden" name="stay" value="1"><button class="tc-rail tc-star${card.starred ? ' is-on' : ''}" type="submit" aria-pressed="${String(card.starred)}" title="${card.starred ? 'Unstar' : card.star_says.en}" aria-label="${card.star_says.en}">${sk('star')}</button></form>`
/** The small button that copies a card as one line, to paste into another agent (controller "clip"). */
function copyButton(card) {
  const picked = card.answer.mode === 'said' ? card.options.filter(o => o.chosen).map(o => o.label).join(', ') : ''
  const text = `${card.nr.en} · ${card.title}${picked ? ` → ${picked}` : ''}`
  return html`<button class="cardclip-copy" type="button" data-controller="clip" data-action="clip#copy" data-clip-text-value="${text}" data-clip-card-value="${JSON.stringify({ id: card.id, number: card.number, title: card.title, choice_label: picked })}" title="Copy" aria-label="Copy">${COPY_ICON}</button>`
}

/** The whole <main> of a card's page. pic: which picture stands. walk: a step of Blitz. version: a version before, as
 *  it was. from: the session it was opened from (/chat/<slug>/card/<n>: { slug, name }): the way back and the links
 *  lead there. full: a picture opened large. focus: the page alone in the window. decided: just answered here. */
export function cardPage(card, vm, { pic = 1, walk = false, version = null, from = null, full = false, focus = false, decided = false } = {}) {
  const old = version ? card.versions?.find(v => v.n === version) ?? null : null
  const mode = card.answer.mode, answering = !old && (mode === 'options' || mode === 'read')
  const open = card.state === 'open' && !old
  // The draft and the pen stay while the card is with its session too: what is drawn or written meanwhile is kept.
  const drafting = open && card.kind === 'decision'
  const canDraw = drafting || (answering && card.kind === 'info')
  const home = from ? `/chat/${encodeURIComponent(from.slug)}` : '/'
  const self = from ? home : ''
  const query = `${walk ? '&walk=1' : ''}${old ? `&v=${old.n}` : ''}`
  const media = mediaOf(vm, old ?? card), shown = media.pictures.length + media.videos.length
  const large = Boolean(full && shown)
  const keep = focus ? raw(FOCUS_FIELD) : ''
  const shownPic = Math.min(Math.max(1, pic), Math.max(1, shown))
  const here = cardPath(card, self)
  const step = (to, cls, label, art) => (to ? html`<a class="tc-rail ${cls}" data-nav href="${cardPath(to, self)}${walk ? '?walk=1' : ''}" aria-label="${label}">${art}</a>` : '')
  const form = `card-form-${card.id}`
  const corner = cls => raw(`<svg class="tc-corner ${cls}" viewBox="0 0 64 64" aria-hidden="true"><path d="M1.5 1.5 Q30 2.4 61 1.8 Q32 31 2.2 61 Q1 30 1.5 1.5 Z"/><path d="M8 40 L40 8 M8 26 L26 8 M8 13 L13 8" class="hatch"/></svg>`)
  // The slim strip under the top edge once the card has scrolled out of view: the title, the answers as chips (the
  // card's own buttons again: the same form, the same addresses), the duck, What??, the reverse card, Later's tag.
  // (Reverse goes on to the next card, as the card's own reverse tile does: name="next")
  const mini = (cls, way, tip, art) => html`<button class="tc-mini ${cls}" type="submit" form="${form}" formaction="${act(card, way)}"${way === 'revise' ? raw(' name="next" value="1"') : ''} title="${tip}" aria-label="${tip}">${art}</button>`
  const few = answering && card.kind === 'decision' && !card.multiple && card.options.length <= 4
  const laterTag = cls => (open && card.kind !== 'request' && !card.snoozed_until ? html`<form class="${cls}"${cls === 'tc-later' ? raw(' data-action="pointerdown->card#pullStart click->card#pullClick" aria-label="Put this question off"') : ''} method="post" action="${act(card, 'snooze')}"><input type="hidden" name="page" value="1">${keep}${from ? html`<input type="hidden" name="back" value="${home}">` : ''}${sideWays({ duck: false, shred: false, word: false })}</form>` : '')
  const bar = html`<div class="tc-bar" aria-label="This question, in short"><div class="tc-bar-in"><b class="tc-bar-title">${card.title}</b>
${few ? html`<span class="tc-bar-chips">${card.options.map(o => html`<button class="tc-chip" type="submit" form="${form}" formaction="${act(card, 'decide')}" name="key" value="${o.key}" title="${o.label}">${o.label}${o.settles ? finalSign() : ''}</button>`)}</span>` : ''}<button class="tc-chip is-jump" type="button" data-action="card#toAnswers">To the answers ↑</button>
${answering && card.kind === 'decision' ? html`<i class="tc-bar-sep"></i>${mini('is-duck', 'trust', 'Duck it', sk('duck'))}${mini('is-what', 'what', 'What??', sk('what'))}${mini('is-reverse', 'revise', WORDS.revise, sk('reverse'))}` : answering && card.kind === 'info' ? html`<i class="tc-bar-sep"></i>${mini('is-what', 'what', 'What??', sk('what'))}${mini('is-reverse', 'revise', WORDS.revise, sk('reverse'))}${mini('is-ack', 'close', WORDS.ack, sk('tick'))}` : ''}
${laterTag('tc-bar-later')}
</div></div>`
  // The tray: card, field and talk lie on one pad in the session's tone, with a pen outline and hatched corners; the
  // card is white with an ink outline and a drop shadow, an object on it; the talk in time order, the field at its foot.
  const tone = card.from ? whoOf(card.from).hue : null
  const more = (cls, drawing, word, tip, way) => html`<button class="tc-more-item ${cls}" type="submit" form="${form}" formaction="${act(card, way)}" title="${tip}">${sk(drawing)}<span>${word}</span></button>`
  // What stands unsent (the draft: the field's words, the ticks, the notes on options) is given to the controller,
  // which puts it into the fields once: the fields' markup never changes while he types.
  const kept = drafting ? { note: card.draft ?? '', keys: card.multiple ? card.options.filter(o => o.chosen).map(o => o.key) : [], notes: Object.fromEntries(card.options.filter(o => o.note).map(o => [o.key, o.note])) } : { note: card.draft ?? '' }
  const after = decided && !old && card.state !== 'open' ? { self, home, focus } : null
  return html`<main id="cardpage" class="tc-page${large ? ' is-full' : focus ? ' is-focus' : ''}" aria-label="Question ${card.number}" data-id="${card.id}"${focus ? raw(' data-focus') : ''} data-controller="card" data-card-draft-value="${drafting ? act(card, 'draft') : ''}" data-card-kept-value="${JSON.stringify(kept)}" data-card-pictures-value="${JSON.stringify(picturesOf(media.pictures, vm, here))}" data-action="turbo:frame-load->card#framed circles:drawn->card#link turbo:submit-start->card#sent turbo:submit-end->card#done dragover->card#over drop->card#drop">
<div class="tc-pad"${tone != null ? html` style="--hue:${tone}"` : ''}>${corner('tl')}${corner('tr')}${corner('bl')}${corner('br')}${bar}<div class="tc-frame">
<nav class="tc-rails" aria-label="Around this question">
<a class="tc-rail tc-back" data-nav href="${home}" aria-keyshortcuts="Escape" title="Back (Esc)" aria-label="Back to ${from ? from.name : WORDS.desk}">${CROSS}</a>
${old ? '' : starButton(card)}
${card.place && !old ? html`${step(card.place.prev, 'is-prev', 'The question before', BACK)}${step(card.place.next, 'is-next', 'The next question', FORTH)}` : ''}
${large ? html`<a class="tc-rail tc-full is-leave" data-nav href="${here}?pic=${shownPic}${focus ? '&focus=1' : ''}" data-card-target="gallery" data-back data-turbo-action="replace" title="Back (Esc)" aria-label="Back to the card">${CROSS}</a>`
    : html`<a class="tc-rail tc-full${focus ? ' is-leave' : ''}" data-nav href="${focusAt(`${here}?${shownPic > 1 ? `pic=${shownPic}` : ''}${query}`, !focus)}" data-turbo-action="replace" data-action="card#focus" role="button" aria-pressed="${String(focus)}" title="${FOCUS_WORDS[focus]}" aria-label="${FOCUS_WORDS[focus]}">${FULL}${LESS}</a>`}
${canDraw ? html`<button type="button" class="tc-rail tc-draw" data-action="card#trace" aria-pressed="false" title="Draw" aria-label="Draw on the card">${DRAW}</button>` : ''}
<details class="tc-more" data-controller="pops"><summary class="tc-rail tc-more-open" title="More" aria-label="More for this question">${DOTS}</summary><div class="tc-more-list" role="menu">
${open && card.kind !== 'request' && card.snoozed_until ? more('', 'wake', WORDS.wake, `${WORDS.wake}: back on the Desk now`, 'wake') : ''}
${copyButton(card)}
${answering ? more('is-shred', 'bin', WORDS.shred, WORDS.shred, 'shred') : ''}
</div></details>
</nav>
<article class="tc-card" id="card-${card.id}" style="--hue:162" data-id="${card.id}" data-kind="${card.kind}" data-urgency="${URGENCY[card.urgency] ?? 'normal'}" aria-labelledby="card-title-${card.id}"${shown ? raw(' data-pictures') : ''}${card.state === 'open' && mode !== 'revising' ? '' : raw(' data-settled')}>
<div class="tc-left">${cardLead(card, self, old)}
${cardMedia(card.id, media, vm, here, shownPic, query)}
${old ? '' : optionsSaid(card)}
</div>
<div class="tc-right">
${cardAnswer(card, { old, pic: shownPic, after, self })}
</div>
<div class="tc-ink" style="display:contents"></div>
</article>
${laterTag('tc-later')}
</div>
<div class="tc-notes">${cardThread(card, vm, self)}
<form class="tc-chat" id="${form}" aria-label="Write to the agent" method="post" action="${act(card, 'message')}" enctype="multipart/form-data" data-card-target="form">
<input type="hidden" name="page" value="1">${walk ? raw('<input type="hidden" name="walk" value="1">') : ''}${keep}${from ? html`<input type="hidden" name="back" value="${home}">` : ''}
${canDraw ? html`<input type="hidden" name="marks" value="[]" data-card-target="marks">` : ''}
<span class="tc-c-who is-you" aria-hidden="true">${sk('pen')}</span>
<div class="tc-ask"><div class="tc-chips" data-card-target="chips" hidden></div>
${plusMenu(canDraw)}<textarea class="tc-field" id="card-field-${card.id}" data-card-target="field" data-action="input->card#typed keydown->card#keys paste->card#paste" name="note" rows="1" placeholder="${card.ask.en}" autocomplete="off" enterkeyhint="send" aria-label="Write to the agent about this question. Send adds it to the talk; an answer takes it along as a note."></textarea>
<div class="tc-ask-row"><span class="tc-saved" role="status" data-card-target="saved" hidden></span>${sendButton(answering && (card.kind === 'decision' || card.kind === 'info'))}</div></div>
</form>
</div>
</div>
</main>`
}
// Send and reverse (the words go back with the card, for rework): while Shift is held, Send is the reverse card, and
// a click sends so; Shift+Enter (and Ctrl/⌘+Enter) from the field does the same. Alt+Enter is a new line (ui.mjs
// watchShift sets <html data-shift>, card.css turns Send; controller "card": sendClick, keys).
function sendButton(canReverse) {
  return html`<button class="tc-send" type="submit" name="stay" value="1" title="Send (Enter)" aria-label="Send to the agent" data-action="click->card#sendClick">${sk('send')}${canReverse ? html`<span class="tc-send-uno" aria-hidden="true">${sk('reverse')}</span>` : ''}</button>`
}
/** The "+" at the field's left: a small menu with Attach (files or pictures; also paste or drop them on the card) and,
 *  where the card can be drawn on, Draw on the card. */
function plusMenu(canDraw) {
  return html`<details class="tc-plus" data-controller="pops"><summary class="tc-plus-open" title="Attach or draw" aria-label="Attach or draw">${PLUS}</summary><div class="tc-plus-pop" role="menu">
<label class="tc-plus-item tc-clip" role="menuitem" title="Attach">${sk('clip')}<span>Attach files or pictures</span><input type="file" name="files" multiple hidden data-card-target="files" data-action="change->card#files"></label>${canDraw ? html`<button type="button" role="menuitem" class="tc-plus-item tc-draw tc-draw-pen" data-action="card#trace" data-pop-close aria-pressed="false" title="Draw">${sk('pen')}<span>Draw on the card</span></button>` : ''}</div></details>`
}

// ---- focus marks ----
// Writing and drawing on a question card. A pencil in a paragraph's margin begins a note that stays with that
// paragraph (or option); the tracing sheet (traceSheet below) is laid over the whole card to draw on. Both are "marks":
//   { id, anchor: { kind: 'text' | 'option', key?, quote? }, text }                          a written note
//   { id: 'pen-card' | 'pen-pic-<n>', anchor: { kind: 'card' } | { kind: 'picture', index },
//     strokes: [{ tool: 'pen' | 'hl', color, pts: [x0, y0, …] }], words: [{ x, y, text, color }], text?, scroll? }
//                                                                                             a tracing sheet
// A note on a piece of the text carries how that piece begins (quote), to find it again after a rewording; a
// paragraph that is an option counts as that option. A sheet's points are fractions of the card's WIDTH (y too), so
// a drawing scales with the card; its words are its text too (what the agent reads), and scroll is where the question
// column stood while drawing (put back when the sheet is laid on again). One sheet over the card, one per picture in
// Full screen. They go with whatever is sent from the card, the sheet as a picture beside them (controller "card").
//
//   const marks = cardMarks({ scroll, blocks(), labelOf(key), onChange() })
//   marks.get() / set(list)   the marks, plain data (for the card's draft)
//   marks.sheet(id, anchor, make) / touch(id)   a tracing sheet's mark; it changed

const NS = 'http://www.w3.org/2000/svg'
const newId = () => Math.random().toString(36).slice(2, 10)

function cardMarks({ scroll, blocks, labelOf, onChange }) {
  let list = []
  let pen = false

  // (While the tracing sheet lies on the card, no pencil is offered in the margin.)
  function setPen(on) { pen = on; quill.hidden = true }

  // ── written notes ──
  const anchorOf = target => {
    const all = blocks()
    const block = all.find(b => b.contains(target))
    if (!block) return null
    if (block.dataset.key) return { kind: 'option', key: block.dataset.key }
    return { kind: 'text', quote: block.textContent.trim().replace(/\s+/g, ' ').slice(0, 48) }
  }
  /** Where a note of that anchor stands: after its block, under its option, or at the end of the text. */
  function placeOf(anchor) {
    if (anchor.kind === 'text') return blocks().find(b => !b.classList.contains('focus-mark') && b.textContent.trim().replace(/\s+/g, ' ').startsWith(anchor.quote ?? '\u0000')) ?? null
    return null
  }
  const noteNodes = new Map()   // mark id -> node
  function paintNotes() {
    for (const [id, node] of noteNodes) if (!list.some(m => m.id === id && m.text != null)) { node.remove(); noteNodes.delete(id) }
    for (const mark of list) {
      if (mark.text == null) continue
      let node = noteNodes.get(mark.id)
      if (!node) {
        node = el('label', 'focus-mark')
        const field = el('textarea')
        field.rows = 1
        field.placeholder = 'Write here'
        field.setAttribute('aria-label', mark.anchor.kind === 'option' ? `Note on ${labelOf(mark.anchor.key)}` : 'Note on this place')
        const grow = () => { field.style.height = 'auto'; field.style.height = `${field.scrollHeight}px` }
        field.addEventListener('input', () => { mark.text = field.value; grow(); onChange() })
        // Enter finishes the note, Shift+Enter breaks the line, Escape leaves; an empty note is gone.
        field.addEventListener('keydown', e => {
          if (e.key === 'Escape' || (e.key === 'Enter' && !e.shiftKey && !e.isComposing)) { e.preventDefault(); e.stopPropagation(); field.blur() }
        })
        field.addEventListener('blur', () => { if (!field.value.trim()) { list = list.filter(m => m !== mark); changed() } })
        node.append(sketch('pen'), field)
        node.grow = grow
        noteNodes.set(mark.id, node)
      }
      const field = node.querySelector('textarea')
      if (document.activeElement !== field && field.value !== mark.text) field.value = mark.text
      const place = placeOf(mark.anchor)
      node.dataset.kind = mark.anchor.kind
      const home = place ?? scroll.querySelector('.focus-lead')
      if (node.previousElementSibling !== home && !(node.previousElementSibling?.classList.contains('focus-mark') && node.isConnected)) home?.after(node)
      node.grow()
    }
  }
  function note(anchor) {
    if (!anchor || anchor.kind === 'card') return   // words about the whole card go into the field at its foot
    // one note per place: a second click there goes on writing the first
    let mark = list.find(m => m.text != null && m.anchor.kind === anchor.kind && m.anchor.key === anchor.key && (anchor.kind !== 'text' || m.anchor.quote === anchor.quote))
    if (!mark) { mark = { id: newId(), anchor, text: '' }; list.push(mark) }
    paintNotes()
    noteNodes.get(mark.id)?.querySelector('textarea').focus({ preventScroll: false })
  }
  // A note never begins by a plain click on the text (that happens by accident while reading and selecting). Under
  // the pointer a paragraph shows a small pencil in its margin; the pencil begins the note. Selecting text in a
  // paragraph shows the same pencil.
  const quill = el('button', 'focus-mark-quill')
  quill.type = 'button'
  quill.title = 'Note'
  quill.setAttribute('aria-label', 'Write a note on this paragraph')
  quill.append(sketch('pen'))
  quill.hidden = true
  scroll.append(quill)
  let quillAt = null
  const offer = block => {
    if (pen || !block || block.dataset.key) { if (!quill.matches(':hover')) quill.hidden = true; return }
    quillAt = block
    const box = scroll.getBoundingClientRect(), r = block.getBoundingClientRect()
    quill.style.left = `${Math.max(2, r.left - box.left + scroll.scrollLeft - 28)}px`
    quill.style.top = `${r.top - box.top + scroll.scrollTop}px`
    quill.hidden = false
  }
  scroll.addEventListener('pointerover', e => { if (e.target === quill || quill.contains(e.target)) return; offer(blocks().find(b => b.contains(e.target))) })
  scroll.addEventListener('pointerleave', () => { quill.hidden = true })
  // (The page is swapped without a reload: once the card is gone, this listener goes too, not one more per card opened.)
  document.addEventListener('selectionchange', function selected() {
    if (!scroll.isConnected) return document.removeEventListener('selectionchange', selected)
    const sel = getSelection()
    if (!sel || sel.isCollapsed || !scroll.contains(sel.anchorNode)) return
    offer(blocks().find(b => b.contains(sel.anchorNode)))
  })
  quill.addEventListener('click', () => { if (!quillAt) return; quill.hidden = true; note(anchorOf(quillAt)) })

  function changed() {
    list = list.filter(m => m.text != null || m.strokes?.length)
    paintNotes()
    onChange()
  }

  return {
    scroll,
    setPen,
    get: () => list.map(m => ({ ...m, ...(m.strokes ? { strokes: m.strokes.map(s => ({ ...s, pts: [...s.pts] })) } : {}), ...(m.words ? { words: m.words.map(w => ({ ...w })) } : {}) })),
    set(next) {
      list = (Array.isArray(next) ? next : []).filter(m => m && m.id && m.anchor && (String(m.id).startsWith('pen-') ? m.strokes || m.words : m.text != null && m.anchor.kind !== 'card')).map(m => ({ id: m.id, anchor: m.anchor, ...(m.text != null ? { text: String(m.text) } : {}), ...(m.strokes ? { strokes: m.strokes } : {}), ...(m.words ? { words: m.words } : {}), ...(m.scroll ? { scroll: m.scroll } : {}) }))
      paintNotes()
    },
    /** The drawing of one tracing sheet (id 'pen-card' or 'pen-pic-<n>'): its mark, made when asked to. */
    sheet(id, anchor, make) { let m = list.find(x => x.id === id); if (!m && make) { m = { id, anchor, strokes: [], words: [] }; list.push(m) } return m ?? null },
    /** The sheet's drawing changed: an empty one is dropped, its words are the mark's text (what the agent reads). */
    touch(id) {
      const m = list.find(x => x.id === id)
      if (m) { m.strokes ??= []; m.words ??= []; const said = m.words.map(w => w.text.trim()).filter(Boolean).join(' · '); if (said) m.text = said; else delete m.text }
      list = list.filter(x => !String(x.id).startsWith('pen-') || x.strokes?.length || x.words?.length)
      onChange()
    },
  }
}

// ---- the tracing sheet ----
// The pen above the card (and the one in the field's row) lays a sheet of tracing paper, taped at the top, over the
// WHOLE card: the question, its picture and the answers show through; mouse, pen and finger draw on it. The Scribble
// Board's tools stand at the foot: pen, highlighter, colours, eraser, text, undo and redo, and "Send with the card".
// Esc or the cross takes the sheet off; the drawing stays on the card (its draft) until it is sent or cleared.
//
//   const sheet = traceSheet({ card, marks, id, anchor, left, onSend, onClose })   card: the .tc-card element
//   sheet.close()                       take it off (as Esc)
const PEN_INKS = [['ink', 'Ink'], ['#d9480f', 'Rust'], ['#0b7a5c', 'Green'], ['#1971c2', 'Blue']]
const HL_INK = '#ffd43b'
const TOOL_PATHS = {
  pen: ['M4 20l1.2-4.4L16.6 4.2a2 2 0 012.9 0l.3.3a2 2 0 010 2.9L8.4 18.8z', 'M14.5 6.5l3 3'],
  hl: ['M14.5 4l5.5 5.5-8 8H7.5v-4.5z', 'M11.5 7l5.5 5.5', 'M4 21h10'],
  eraser: ['M20 20H9.5l-5-5a2 2 0 010-2.8l8-8a2 2 0 012.8 0l4.9 4.9a2 2 0 010 2.8L12 20', 'M8.7 8.3l7 7'],
  text: ['M5.5 7V5h13v2', 'M12 5v14', 'M9.5 19h5'],
  undo: ['M9 14L4 9l5-5', 'M4 9h10.5a5.5 5.5 0 010 11H11'],
  redo: ['M15 14l5-5-5-5', 'M20 9H9.5a5.5 5.5 0 000 11H13'],
  trash: ['M4 7h16', 'M10 11v6', 'M14 11v6', 'M6 7l1 12a2 2 0 002 2h6a2 2 0 002-2l1-12', 'M9 7V4h6v3'],
  send: ['M12 19V5', 'M5.5 11.5L12 5l6.5 6.5'],
  close: ['M6 6l12 12', 'M18 6L6 18'],
}
const toolIcon = name => { const svg = document.createElementNS(NS, 'svg'); svg.setAttribute('viewBox', '0 0 24 24'); svg.setAttribute('class', 'tc-trace-icon'); svg.setAttribute('aria-hidden', 'true'); for (const d of TOOL_PATHS[name]) { const p = document.createElementNS(NS, 'path'); p.setAttribute('d', d); svg.append(p) } return svg }
const isDark = () => document.documentElement.dataset.theme === 'dark'
const DARK_INKS = { ink: '#e9eeea', '#d9480f': '#ff8a4c', '#0b7a5c': '#45d1a3', '#1971c2': '#6cb2ff' }
const inkOf = color => (isDark() ? DARK_INKS[color] ?? color : color === 'ink' ? '#1b1f23' : color)
const SHEET_TOOL = 'trommi-trace-tool'
/** A sheet's stroke as a pen draws it: a smooth line through its points (each point the control of a quadratic curve
 *  between the midpoints beside it), never straight pieces from point to point. pts: [x0, y0, …] as parts of the
 *  sheet's width w. Returns the steps [['M', x, y], ['Q', cx, cy, x, y], ['L', x, y]]: an SVG path's and a canvas's. */
function strokeSteps(pts, w) {
  const n = pts.length >> 1, X = i => pts[2 * i] * w, Y = i => pts[2 * i + 1] * w
  if (!n) return []
  const out = [['M', X(0), Y(0)]]
  if (n === 1) return [...out, ['L', X(0) + 0.1, Y(0)]]
  for (let i = 1; i < n - 1; i++) out.push(['Q', X(i), Y(i), (X(i) + X(i + 1)) / 2, (Y(i) + Y(i + 1)) / 2])
  out.push(['L', X(n - 1), Y(n - 1)])
  return out
}
const strokePath = (pts, w) => strokeSteps(pts, w).map(([k, ...v]) => k + v.map(x => x.toFixed(1)).join(' ')).join('')
/** Every sample of a pointer move: the browser hands one event per frame, the samples between are in it. Taking
 *  only the event itself leaves a quick stroke with one point per frame: long straight pieces with corners. */
const samplesOf = e => { const list = e.getCoalescedEvents?.() ?? []; return list.length ? list : [e] }

function traceSheet({ card, marks, id, anchor, left, dock, onSend, onClose }) {
  let mark = marks.sheet(id, anchor, false)
  let { tool, color } = (() => { try { return { tool: 'pen', color: 'ink', ...JSON.parse(localStorage.getItem(SHEET_TOOL) || '{}') } } catch { return { tool: 'pen', color: 'ink' } } })()
  if (!['pen', 'hl', 'eraser', 'text'].includes(tool)) tool = 'pen'
  const past = [], next = []
  const snap = () => JSON.stringify({ strokes: mark?.strokes ?? [], words: mark?.words ?? [] })
  // (the question column stands where it stood when this was drawn)
  if (left && mark?.scroll != null) left.scrollTop = mark.scroll

  const sheet = el('div', 'tc-trace')
  sheet.setAttribute('role', 'application')
  sheet.setAttribute('aria-label', 'Tracing sheet over the card: draw with mouse, pen or finger. Esc takes it off.')
  const tape = el('span', 'tc-trace-tape')
  const ink = document.createElementNS(NS, 'svg')
  ink.setAttribute('class', 'tc-trace-ink')
  const words = el('div', 'tc-trace-words')
  sheet.append(ink, words, tape)
  ;(card.querySelector(':scope > .tc-ink') ?? card).append(sheet)
  card.closest('.tc-page')?.classList.add('is-tracing')

  const width = () => sheet.clientWidth || 1
  const pathOf = pts => strokePath(pts, width())
  function paint() {
    ink.replaceChildren(...(mark?.strokes ?? []).map((s, at) => { const p = document.createElementNS(NS, 'path'); p.setAttribute('d', pathOf(s.pts)); p.setAttribute('class', `is-${s.tool ?? 'pen'}`); p.style.stroke = s.tool === 'hl' ? s.color : inkOf(s.color); p.dataset.at = at; return p }))
    words.replaceChildren(...(mark?.words ?? []).map((w, at) => { const t = el('span', 'tc-trace-word', w.text); t.style.left = `${w.x * width()}px`; t.style.top = `${w.y * width()}px`; t.style.color = inkOf(w.color); t.dataset.at = at; return t }))
    paintTools()
  }
  new ResizeObserver(() => paint()).observe(sheet)
  const point = e => { const box = sheet.getBoundingClientRect(); return [Number(((e.clientX - box.left) / width()).toFixed(4)), Number(((e.clientY - box.top) / width()).toFixed(4))] }
  const own = () => (mark ??= marks.sheet(id, anchor, true))
  function changed(before) {
    if (before != null && before !== snap()) { past.push(before); next.length = 0 }
    if (mark) { if (left && !mark.scroll && left.scrollTop) mark.scroll = Math.round(left.scrollTop); marks.touch(id); if (!marks.sheet(id)) mark = null }
    paint()
  }

  // ── drawing: one pointer draws; a second finger stops the line and moves the page instead ──
  let drawing = null, fingers = new Map(), inking = 0
  sheet.addEventListener('pointerdown', e => {
    if (e.button || e.target.closest('.tc-trace-type')) return
    fingers.set(e.pointerId, [e.clientX, e.clientY])
    if (fingers.size > 1) { if (drawing) { mark.strokes.pop(); drawing = null; paint() } return }
    e.preventDefault()
    sheet.setPointerCapture(e.pointerId)
    if (tool === 'eraser') return rub(e)
    if (tool === 'text') return type(point(e))
    const before = snap()
    drawing = { before, stroke: { tool, color: tool === 'hl' ? HL_INK : color, pts: point(e) } }
    own().strokes.push(drawing.stroke)
    paint()
  })
  sheet.addEventListener('pointermove', e => {
    const was = fingers.get(e.pointerId)
    if (was && fingers.size > 1) { window.scrollBy(was[0] - e.clientX, was[1] - e.clientY); if (left) left.scrollTop += 0; fingers.set(e.pointerId, [e.clientX, e.clientY]); return }
    if (tool === 'eraser' && e.buttons & 1) return rub(e)
    if (!drawing) return
    const pts = drawing.stroke.pts, n = pts.length
    for (const sample of samplesOf(e)) {
      const [x, y] = point(sample)
      if (Math.hypot(x - pts.at(-2), y - pts.at(-1)) * width() >= 1.5) pts.push(x, y)
    }
    // (the line is laid once per frame, however many samples came)
    if (pts.length !== n) inking ||= requestAnimationFrame(() => { inking = 0; if (drawing) ink.lastElementChild?.setAttribute('d', pathOf(drawing.stroke.pts)) })
  })
  const lift = e => { fingers.delete(e.pointerId); if (!drawing) return; const { before } = drawing; drawing = null; cancelAnimationFrame(inking); inking = 0; changed(before) }
  sheet.addEventListener('pointerup', lift)
  sheet.addEventListener('pointercancel', lift)
  sheet.addEventListener('wheel', e => e.preventDefault(), { passive: false })   // (the card stays still under the sheet)
  function rub(e) {
    for (const hit of document.elementsFromPoint(e.clientX, e.clientY)) {
      if (!sheet.contains(hit)) continue
      const before = snap()
      if (hit instanceof SVGPathElement) { mark.strokes.splice(Number(hit.dataset.at), 1); return changed(before) }
      if (hit.classList.contains('tc-trace-word')) { mark.words.splice(Number(hit.dataset.at), 1); return changed(before) }
    }
  }
  // ── text: a click puts a field there; Enter or leaving it puts the words on the sheet ──
  function type([x, y], at = null) {
    sheet.querySelector('.tc-trace-type')?.blur()
    const field = el('textarea', 'tc-trace-type')
    field.rows = 1
    field.placeholder = 'Write'
    field.setAttribute('aria-label', 'Words on the sheet')
    field.style.left = `${x * width()}px`
    field.style.top = `${y * width()}px`
    field.style.color = inkOf(color)
    if (at != null) field.value = mark.words[at].text
    sheet.append(field)
    requestAnimationFrame(() => field.focus())
    let done = false
    const put = () => {
      if (done) return; done = true
      const before = snap(), text = field.value.trim()
      field.remove()
      if (at != null) { if (text) mark.words[at].text = text; else mark.words.splice(at, 1) }
      else if (text) own().words.push({ x, y, text, color })
      changed(before)
    }
    field.addEventListener('keydown', e => { e.stopPropagation(); if (e.key === 'Escape' || (e.key === 'Enter' && !e.shiftKey && !e.isComposing)) { e.preventDefault(); field.blur() } })
    field.addEventListener('blur', put)
  }
  words.addEventListener('pointerdown', e => {
    const w = e.target.closest('.tc-trace-word')
    if (!w || tool !== 'text') return
    e.stopPropagation(); e.preventDefault()
    const at = Number(w.dataset.at); type([mark.words[at].x, mark.words[at].y], at)
  })

  // ── the tools ──
  // With the card's field in view (dock: its box, .tc-ask) the tools are the field's own top row: one bar, the words
  // that go with the drawing typed under the tools, and the field's Send sends both. Where there is no field (a
  // picture in Full screen, a card that takes no words) the tools float at the foot with a Send of their own.
  const bar = el('div', 'tc-trace-tools')
  bar.setAttribute('role', 'toolbar')
  bar.setAttribute('aria-label', 'Drawing tools')
  const button = (cls, label, name, act) => { const b = el('button', `tc-trace-btn ${cls}`); b.type = 'button'; b.title = label; b.setAttribute('aria-label', label); if (name) b.append(toolIcon(name)); b.addEventListener('click', act); return b }
  const pick = t => { tool = t; sheet.querySelector('.tc-trace-type')?.blur(); remember(); paintTools() }
  const remember = () => { try { localStorage.setItem(SHEET_TOOL, JSON.stringify({ tool, color })) } catch {} }
  const tools = { pen: button('is-tool', 'Pen · P', 'pen', () => pick('pen')), hl: button('is-tool', 'Highlighter · H', 'hl', () => pick('hl')) }
  const swatches = PEN_INKS.map(([c, name]) => { const b = button('is-swatch', name, null, () => { color = c; if (tool !== 'text') tool = 'pen'; remember(); paintTools() }); b.dataset.color = c; b.append(el('span', 'tc-trace-dot')); return b })
  tools.eraser = button('is-tool', 'Eraser · E', 'eraser', () => pick('eraser'))
  tools.text = button('is-tool', 'Text · T', 'text', () => pick('text'))
  const undo = button('', 'Undo · Ctrl+Z', 'undo', () => step(past, next))
  const redo = button('', 'Redo · Ctrl+Shift+Z', 'redo', () => step(next, past))
  const clear = button('is-clear', 'Clear the sheet', 'trash', () => { if (!mark) return; const before = snap(); mark.strokes = []; mark.words = []; changed(before) })
  const send = el('button', 'tc-trace-send')
  send.type = 'button'
  send.title = 'Send'
  send.append(toolIcon('send'), el('span', '', 'Send with the card'))
  send.addEventListener('click', () => { close(); onSend() })
  const off = button('is-off', 'Take the sheet off · Esc (the drawing stays)', 'close', () => close())
  const group = (...nodes) => { const g = el('span', 'tc-trace-group'); g.append(...nodes); return g }
  bar.append(group(tools.pen, tools.hl, ...swatches), group(tools.eraser, tools.text), group(undo, redo), group(clear))
  const page = card.closest('.tc-page')
  // (the field's own Send says what it sends while the sheet lies there)
  const fieldSend = dock?.querySelector('.tc-send'), sendWas = fieldSend && [fieldSend.title, fieldSend.getAttribute('aria-label')]
  if (dock) {
    bar.classList.add('is-docked')
    bar.append(off)
    dock.prepend(bar)
    page?.classList.add('is-docked')
    if (fieldSend) { fieldSend.title = 'Send (Enter)'; fieldSend.setAttribute('aria-label', 'Send with the card') }
  } else { bar.append(send, off); document.body.append(bar) }
  function step(from, to) {
    if (!from.length) return
    to.push(snap())
    const { strokes, words: w } = JSON.parse(from.pop())
    own().strokes = strokes; mark.words = w
    changed(null)
  }
  function paintTools() {
    for (const [name, b] of Object.entries(tools)) b.setAttribute('aria-pressed', String(tool === name))
    for (const b of swatches) { b.setAttribute('aria-pressed', String(b.dataset.color === color && tool !== 'hl' && tool !== 'eraser')); b.style.setProperty('--c', inkOf(b.dataset.color)) }
    undo.disabled = !past.length
    redo.disabled = !next.length
    clear.disabled = send.disabled = !(mark?.strokes?.length || mark?.words?.length)
    bar.dataset.tool = tool
    sheet.dataset.tool = tool
  }
  // ── keys while the sheet lies there: Esc takes it off, the tools by their letters, undo and redo ──
  const keys = e => {
    if (e.key !== 'Escape' && isTyping(e)) return
    const k = e.key.toLowerCase(), mod = e.ctrlKey || e.metaKey
    if (!mod && k.length === 1 && !letterKeysOn(e)) return
    if (e.key === 'Escape') close()
    else if (mod && k === 'z') step(e.shiftKey ? next : past, e.shiftKey ? past : next)
    else if (mod && k === 'y') step(next, past)
    else if (!mod && !e.altKey && { p: 'pen', h: 'hl', e: 'eraser', t: 'text' }[k]) pick({ p: 'pen', h: 'hl', e: 'eraser', t: 'text' }[k])
    else return
    e.preventDefault(); e.stopImmediatePropagation()
  }
  addEventListener('keydown', keys, true)
  let open = true
  function close() {
    if (!open) return; open = false
    sheet.querySelector('.tc-trace-type')?.blur()
    removeEventListener('keydown', keys, true)
    sheet.remove(); bar.remove()
    page?.classList.remove('is-tracing', 'is-docked')
    if (fieldSend) { fieldSend.title = sendWas[0]; fieldSend.setAttribute('aria-label', sendWas[1]) }
    onClose()
  }
  paint()
  if (!dock) requestAnimationFrame(() => (tools[tool] ?? tools.pen).focus({ preventScroll: true }))
  return { close, get open() { return open } }
}

/** The app's fonts for a picture of the page: fonts.css and fallback.css with every font file written into them as
 *  data (a picture made from markup can fetch nothing, so without this it is set in the system's fallback font, wider,
 *  and its lines break and run into each other). Made once. */
let fontStyles = null
const pageFonts = () => (fontStyles ??= (async () => {
  const dataOf = async url => { const blob = await (await fetch(url)).blob(); return new Promise((resolve, reject) => { const r = new FileReader(); r.onload = () => resolve(r.result); r.onerror = reject; r.readAsDataURL(blob) }) }
  let css = (await Promise.all(['/fonts/fallback.css', '/fonts/fonts.css'].map(async u => (await fetch(u)).text()))).join('\n').replace(/\/\*[\s\S]*?\*\//g, '')
  const files = [...new Set([...css.matchAll(/url\(([^)]+)\)/g)].map(m => m[1]))]
  const data = await Promise.all(files.map(f => dataOf(f.replace(/['"]/g, ''))))
  files.forEach((f, i) => { css = css.replaceAll(`url(${f})`, `url(${data[i]})`) })
  return css
})().catch(err => { fontStyles = null; throw err }))
/** A picture that stands in the page, as data: read from the element itself (its address may be one only the page
 *  can open: an attachment decrypted here). null when it has not loaded or cannot be read. */
function pictureData(img) {
  if (!img.complete || !img.naturalWidth) return null
  try {
    const k = Math.min(1, 2000 / Math.max(img.naturalWidth, img.naturalHeight))
    const cv = document.createElement('canvas')
    cv.width = Math.max(1, Math.round(img.naturalWidth * k)); cv.height = Math.max(1, Math.round(img.naturalHeight * k))
    cv.getContext('2d').drawImage(img, 0, 0, cv.width, cv.height)
    return cv.toDataURL('image/png')
  } catch { return null }
}
/** The card painted from the page as it stands, without markup inside a picture (cardPicture's second way, for a
 *  browser that will not hand out a canvas an SVG <foreignObject> was painted on, or paints nothing of it): every
 *  element's box is measured where it lies and painted in document order: outer shadows, ground, borders, inner
 *  shadows, then what it holds: a picture from its <img> (object-fit kept), a video's frame, a drawing (<svg>, as a
 *  picture of its own with its looks written in; no foreignObject), a field's words, and text word by word where the
 *  page laid each word (Range.getClientRects), in the page's own fonts. What is cut off or scrolled away stays cut
 *  (overflow), opacity is kept. Not painted: gradients and pictures as grounds, ::before and ::after, rotation and
 *  other transforms (a turned box is painted upright), z-index (the order is the document's), text shadows.
 *  The drawings are loaded first, then everything is painted in one go (no waiting while the canvas holds a clip).
 *  c is scaled so that one unit is one CSS pixel of the card, its upper left corner at 0 0. */
async function paintCard(c, card, { pictures = true } = {}) {
  const origin = card.getBoundingClientRect(), px = c.getTransform().a
  const num = v => parseFloat(v) || 0
  const clear = color => !color || color === 'transparent' || /^rgba\(.*,\s*0\)$|\/\s*0\)$/.test(color)
  const boxOf = (r, d = 0) => ({ x: r.left - origin.left + d, y: r.top - origin.top + d, w: Math.max(0, r.width - 2 * d), h: Math.max(0, r.height - 2 * d) })
  const radiiOf = (s, r) => ['TopLeft', 'TopRight', 'BottomRight', 'BottomLeft'].map(k => { const [x, y = x] = s[`border${k}Radius`].split(' ').map((v, i) => (v.endsWith('%') ? parseFloat(v) / 100 * (i ? r.height : r.width) : num(v))); return { x, y } })
  // (a box's outline, d inside it (negative: outside), moved by dx dy; no beginPath, so two of them make a ring)
  const outline = (b, radii, d = 0, dx = 0, dy = 0) => { if (b.w - 2 * d > 0 && b.h - 2 * d > 0) c.roundRect(b.x + d + dx, b.y + d + dy, b.w - 2 * d, b.h - 2 * d, radii.map(q => ({ x: Math.max(0, q.x - d), y: Math.max(0, q.y - d) }))) }
  const shadowsOf = s => (s.boxShadow === 'none' ? [] : s.boxShadow.split(/,(?![^(]*\))/).map(one => { const m = one.trim().match(/^(.*?\)|\S+)\s+(-?[\d.]+)px\s+(-?[\d.]+)px\s+([\d.]+)px\s+(-?[\d.]+)px(\s+inset)?$/); return m ? { color: m[1], x: num(m[2]), y: num(m[3]), blur: num(m[4]), spread: num(m[5]), inset: Boolean(m[6]) } : null }).filter(Boolean).reverse())
  const fontOf = s => `${s.fontStyle} ${s.fontWeight} ${s.fontSize} ${s.fontFamily}`
  const drawings = new Map()
  const LOOKS = ['fill', 'stroke', 'stroke-width', 'stroke-linecap', 'stroke-linejoin', 'stroke-dasharray', 'stroke-dashoffset', 'stroke-miterlimit', 'opacity', 'fill-opacity', 'stroke-opacity', 'fill-rule', 'clip-rule', 'color', 'display', 'visibility', 'transform', 'transform-origin', 'transform-box', 'font-family', 'font-size', 'font-weight', 'font-style', 'text-anchor', 'paint-order']
  const drawingOf = svg => {
    if (svg.querySelector('foreignObject, image, use')) return null
    const copy = svg.cloneNode(true), from = [svg, ...svg.querySelectorAll('*')], to = [copy, ...copy.querySelectorAll('*')], r = svg.getBoundingClientRect()
    from.forEach((a, i) => { const s = getComputedStyle(a); to[i].setAttribute('style', LOOKS.filter(p => i || !p.startsWith('transform')).map(p => `${p}:${s.getPropertyValue(p)}`).join(';')); to[i].removeAttribute('class') })
    copy.setAttribute('xmlns', NS); copy.setAttribute('width', r.width); copy.setAttribute('height', r.height)
    if (!copy.hasAttribute('viewBox')) copy.setAttribute('viewBox', `0 0 ${r.width} ${r.height}`)
    copy.style.display = 'block'; copy.style.overflow = getComputedStyle(svg).overflow
    const text = new XMLSerializer().serializeToString(copy)
    if (!drawings.has(text)) drawings.set(text, new Promise(resolve => { const image = new Image(); image.onload = () => resolve(image); image.onerror = () => resolve(null); image.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(text)}` }))
    return drawings.get(text)
  }
  // (what an element holds in place of children, in its content box: fitted as object-fit says)
  const fitted = (source, sw, sh, b, s) => {
    if (!(sw > 0 && sh > 0 && b.w > 0 && b.h > 0)) return
    const fit = s.objectFit, [ox = .5, oy = .5] = s.objectPosition.split(' ').map(v => (v.endsWith('%') ? parseFloat(v) / 100 : null) ?? .5)
    const k = fit === 'contain' ? Math.min(b.w / sw, b.h / sh) : fit === 'cover' ? Math.max(b.w / sw, b.h / sh) : fit === 'none' ? 1 : fit === 'scale-down' ? Math.min(1, b.w / sw, b.h / sh) : null
    const dw = k == null ? b.w : sw * k, dh = k == null ? b.h : sh * k
    c.save(); c.beginPath(); c.rect(b.x, b.y, b.w, b.h); c.clip()
    c.drawImage(source, b.x + (b.w - dw) * ox, b.y + (b.h - dh) * oy, dw, dh)
    c.restore()
  }
  const range = document.createRange()
  const words = (node, s) => {
    const fill = s.webkitTextFillColor && !clear(s.webkitTextFillColor) ? s.webkitTextFillColor : s.color
    if (!node.data.trim() || clear(fill)) return
    c.font = fontOf(s); c.fillStyle = fill; c.textBaseline = 'alphabetic'
    if ('letterSpacing' in c) c.letterSpacing = s.letterSpacing === 'normal' ? '0px' : s.letterSpacing
    const m = c.measureText('Hg'), up = m.fontBoundingBoxAscent ?? m.actualBoundingBoxAscent, down = m.fontBoundingBoxDescent ?? m.actualBoundingBoxDescent
    const shown = s.textTransform === 'uppercase' ? t => t.toUpperCase() : s.textTransform === 'lowercase' ? t => t.toLowerCase() : t => t
    const lines = []   // (for a line under or through the words: one stretch per line of the page)
    const put = (from, to) => {
      range.setStart(node, from); range.setEnd(node, to)
      const rects = [...range.getClientRects()].filter(r => r.width > 0)
      if (!rects.length) return
      // (a word broken over two lines: letter by letter)
      if (rects.length > 1 && to - from > 1 && rects.some(r => Math.abs(r.top - rects[0].top) > 2)) { for (let i = from; i < to; i++) put(i, i + 1); return }
      const r = rects[0], base = r.top - origin.top + (r.height - up - down) / 2 + up
      if (r.bottom < origin.top || r.top > origin.bottom) return
      c.fillText(shown(node.data.slice(from, to)), r.left - origin.left, base)
      const last = lines.at(-1)
      if (last && Math.abs(last.base - base) < 1) last.right = r.right - origin.left; else lines.push({ left: r.left - origin.left, right: r.right - origin.left, base })
    }
    for (const word of node.data.matchAll(/\S+/g)) put(word.index, word.index + word[0].length)
    const line = s.textDecorationLine ?? ''
    if (line === 'none' || !lines.length) return
    const thick = Math.max(1, num(s.fontSize) / 14)
    c.fillStyle = clear(s.textDecorationColor) ? fill : s.textDecorationColor
    for (const l of lines) {
      if (line.includes('underline')) c.fillRect(l.left, l.base + thick * 1.6, l.right - l.left, thick)
      if (line.includes('line-through')) c.fillRect(l.left, l.base - up * .32, l.right - l.left, thick)
    }
  }
  const paint = el => {
    if (el.classList.contains('tc-trace')) return
    const s = getComputedStyle(el)
    if (s.display === 'none' || el.checkVisibility?.({ contentVisibilityAuto: true }) === false) return
    const seen = s.visibility === 'visible', opacity = num(s.opacity)
    if (s.opacity !== '1' && opacity <= 0) return
    c.save()
    if (opacity < 1) c.globalAlpha *= opacity
    const whole = el.getBoundingClientRect()
    const rects = s.display === 'contents' ? [] : s.display === 'inline' ? [...el.getClientRects()] : [whole]
    const svg = el instanceof SVGElement
    if (seen && !svg) for (const r of rects) {
      if (!(r.width > 0 && r.height > 0)) continue
      const b = boxOf(r), radii = radiiOf(s, r), shadows = shadowsOf(s)
      for (const sh of shadows.filter(x => !x.inset && !clear(x.color))) {
        // (an outer shadow lies around the box only, never under it)
        c.save(); c.beginPath(); c.rect(b.x - 400, b.y - 400, b.w + 800, b.h + 800); outline(b, radii); c.clip('evenodd')
        if (sh.blur > 0) { c.shadowColor = sh.color; c.shadowBlur = sh.blur * px; c.shadowOffsetX = (sh.x + 4000) * px; c.shadowOffsetY = sh.y * px; c.fillStyle = '#000'; c.beginPath(); outline(b, radii, -sh.spread, -4000, 0); c.fill() }
        else { c.fillStyle = sh.color; c.beginPath(); outline(b, radii, -sh.spread, sh.x, sh.y); c.fill() }
        c.restore()
      }
      if (!clear(s.backgroundColor)) { c.fillStyle = s.backgroundColor; c.beginPath(); outline(b, radii); c.fill() }
      const sides = ['Top', 'Right', 'Bottom', 'Left'].map(k => ({ w: s[`border${k}Style`] === 'none' || s[`border${k}Style`] === 'hidden' ? 0 : num(s[`border${k}Width`]), color: s[`border${k}Color`], style: s[`border${k}Style`] }))
      if (sides.every(d => d.w > 0 && d.w === sides[0].w && d.color === sides[0].color)) {
        const d = sides[0]
        if (!clear(d.color)) { c.strokeStyle = d.color; c.lineWidth = d.w; c.setLineDash(d.style === 'dashed' ? [d.w * 3, d.w * 2] : d.style === 'dotted' ? [d.w, d.w] : []); c.beginPath(); outline(b, radii, d.w / 2); c.stroke(); c.setLineDash([]) }
      } else sides.forEach((d, i) => { if (d.w > 0 && !clear(d.color)) { c.fillStyle = d.color; c.fillRect(i === 1 ? b.x + b.w - d.w : b.x, i === 2 ? b.y + b.h - d.w : b.y, i % 2 ? d.w : b.w, i % 2 ? b.h : d.w) } })
      for (const sh of shadows.filter(x => x.inset && !clear(x.color))) {
        c.save(); c.beginPath(); outline(b, radii); c.clip()
        c.fillStyle = sh.color; c.beginPath(); c.rect(b.x - 1, b.y - 1, b.w + 2, b.h + 2); outline(b, radii, sh.spread + sh.blur / 2, sh.x, sh.y); c.fill('evenodd')
        c.restore()
      }
    }
    const inner = { x: whole.left - origin.left + num(s.borderLeftWidth) + num(s.paddingLeft), y: whole.top - origin.top + num(s.borderTopWidth) + num(s.paddingTop), w: whole.width - num(s.borderLeftWidth) - num(s.borderRightWidth) - num(s.paddingLeft) - num(s.paddingRight), h: whole.height - num(s.borderTopWidth) - num(s.borderBottomWidth) - num(s.paddingTop) - num(s.paddingBottom) }
    const rounded = fn => { c.save(); c.beginPath(); outline(boxOf(whole), radiiOf(s, whole)); c.clip(); try { fn() } finally { c.restore() } }
    if (!seen) { /* (its children may be seen) */ }
    else if (svg) { const image = made.get(el); if (image && whole.width > 0 && whole.height > 0) c.drawImage(image, whole.left - origin.left, whole.top - origin.top, whole.width, whole.height) }
    else if (el instanceof HTMLImageElement) { if (pictures && el.complete && el.naturalWidth) rounded(() => fitted(el, el.naturalWidth, el.naturalHeight, inner, s)) }
    else if (el instanceof HTMLVideoElement) { if (pictures && el.readyState >= 2) rounded(() => fitted(el, el.videoWidth, el.videoHeight, inner, s)) }
    else if (el instanceof HTMLCanvasElement) { if (pictures && el.width && el.height) fitted(el, el.width, el.height, inner, { objectFit: 'fill', objectPosition: '50% 50%' }) }
    else if (el instanceof HTMLIFrameElement) { c.fillStyle = '#ddd'; c.fillRect(inner.x, inner.y, inner.w, inner.h) }
    else if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
      const text = el.value || el.placeholder || '', size = num(s.fontSize), high = num(s.lineHeight) || size * 1.3
      if (text && !['checkbox', 'radio', 'hidden', 'file', 'range', 'color'].includes(el.type)) {
        c.save(); c.beginPath(); c.rect(inner.x, inner.y, inner.w, inner.h); c.clip()
        c.font = fontOf(s); c.textBaseline = 'middle'; c.fillStyle = s.color
        if (!el.value) c.globalAlpha *= .55
        text.split('\n').forEach((t, i) => c.fillText(t, inner.x, el instanceof HTMLTextAreaElement ? inner.y + high * (i + .5) - el.scrollTop : inner.y + inner.h / 2))
        c.restore()
      }
    }
    if (svg || el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLImageElement || el instanceof HTMLVideoElement || el instanceof HTMLIFrameElement || el instanceof HTMLCanvasElement) { c.restore(); return }
    // (a list's point or number, left of the entry's first line)
    if (seen && s.display === 'list-item' && s.listStyleType !== 'none') {
      range.selectNodeContents(el)
      const first = [...range.getClientRects()].find(r => r.width > 0 && r.height > 0), size = num(s.fontSize)
      if (first) {
        c.fillStyle = s.color
        const x = inner.x - size * .55, y = first.top - origin.top + first.height / 2
        if (/decimal/.test(s.listStyleType)) { const at = (el.parentElement?.start || 1) + [...el.parentElement.children].filter(k => k.tagName === 'LI').indexOf(el); c.font = fontOf(s); c.textBaseline = 'middle'; c.textAlign = 'right'; c.fillText(`${at}.`, inner.x - size * .3, y); c.textAlign = 'left' }
        else { c.beginPath(); c.arc(x, y, size * .17, 0, Math.PI * 2); if (s.listStyleType === 'circle') { c.strokeStyle = s.color; c.lineWidth = 1; c.stroke() } else c.fill() }
      }
    }
    // (what lies beyond a box that cuts its content, or is scrolled away in it, is not painted)
    if (s.display !== 'inline' && s.display !== 'contents' && (s.overflowX !== 'visible' || s.overflowY !== 'visible')) {
      const bl = num(s.borderLeftWidth), bt = num(s.borderTopWidth), far = 1e5, cutX = s.overflowX !== 'visible', cutY = s.overflowY !== 'visible'
      const b = boxOf(whole)
      c.beginPath()
      if (cutX && cutY) outline({ x: b.x + bl, y: b.y + bt, w: el.clientWidth || b.w, h: el.clientHeight || b.h }, radiiOf(s, whole).map(q => ({ x: Math.max(0, q.x - bl), y: Math.max(0, q.y - bt) })))
      else c.rect(cutX ? b.x + bl : -far, cutY ? b.y + bt : -far, cutX ? el.clientWidth || b.w : 2 * far, cutY ? el.clientHeight || b.h : 2 * far)
      c.clip()
    }
    for (const kid of el.childNodes) {
      if (kid.nodeType === 3) { if (seen) words(kid, s) }
      else if (kid.nodeType === 1) paint(kid)
    }
    c.restore()
  }
  // (the drawings are made into pictures first: the card is then painted in one go, as it stands at that moment)
  const made = new Map()
  await Promise.all([...card.querySelectorAll('svg')].filter(svg => !svg.closest('.tc-trace') && !svg.parentElement.closest('svg')).map(async svg => made.set(svg, await drawingOf(svg))))
  paint(card)
}
/** The card as the human sees it, with a sheet's drawing on it: what the card shows is copied with its looks written
 *  into every element, the app's fonts and its pictures taken in as data, laid into an SVG and painted on a canvas at
 *  twice the size; the drawing is painted over it. A picture that cannot be taken in leaves its place empty. Where a
 *  browser will not hand such a canvas out, cannot load that SVG, or paints nothing of it (Safari), the card is
 *  painted box by box and word by word from the page instead (paintCard); only if that fails too do the drawing and
 *  its words go on plain paper. The canvas is never larger than a phone's browser makes one (16 million pixels).
 *  Promise<Blob | null>. */
async function cardPicture(card, mark) {
  const w = Math.round(card.clientWidth), h = Math.min(Math.round(card.clientHeight), 6000), scale = Math.min(2, Math.sqrt(16e6 / Math.max(1, w * h)))
  let canvas, c
  const paper = () => { canvas = document.createElement('canvas'); canvas.width = w * scale; canvas.height = h * scale; c = canvas.getContext('2d'); c.fillStyle = '#fff'; c.fillRect(0, 0, canvas.width, canvas.height) }
  paper()
  const blob = () => new Promise(resolve => { try { canvas.toBlob(resolve, 'image/png') } catch { resolve(null) } })
  const drawing = () => {
    c.save(); c.scale(scale, scale); c.lineCap = c.lineJoin = 'round'
    for (const s of mark.strokes ?? []) {
      c.globalAlpha = s.tool === 'hl' ? .38 : 1
      c.lineWidth = s.tool === 'hl' ? 16 : 2.8
      c.strokeStyle = s.tool === 'hl' ? s.color : inkOf(s.color)
      c.beginPath()
      for (const [k, ...v] of strokeSteps(s.pts, w)) { if (k === 'M') c.moveTo(...v); else if (k === 'Q') c.quadraticCurveTo(...v); else c.lineTo(...v) }
      c.stroke()
    }
    c.globalAlpha = 1
    c.font = `600 17px ${getComputedStyle(card).fontFamily || 'system-ui, sans-serif'}`
    c.textBaseline = 'top'
    for (const word of mark.words ?? []) { c.fillStyle = inkOf(word.color); word.text.split('\n').forEach((line, i) => c.fillText(line, word.x * w + 2, word.y * w + 2 + i * 22)) }
    c.restore()
  }
  try {
    const [fonts] = await Promise.all([pageFonts(), document.fonts?.ready])
    const copy = card.cloneNode(true)
    copy.querySelector('.tc-trace')?.remove()
    const from = [card, ...card.querySelectorAll('*')].filter(n => !n.closest('.tc-trace')), to = [copy, ...copy.querySelectorAll('*')]
    for (let i = 0; i < from.length; i++) {
      const a = from[i], b = to[i]
      if (!(b instanceof HTMLElement || b instanceof SVGElement)) continue
      const style = getComputedStyle(a)
      let css = ''
      for (const prop of style) css += `${prop}:${style.getPropertyValue(prop)};`
      b.setAttribute('style', css)
      if (b instanceof HTMLTextAreaElement) b.textContent = a.value
      if (b instanceof HTMLIFrameElement || b instanceof HTMLVideoElement) { const box = el('div'); box.setAttribute('style', `${css}background:#ddd;`); b.replaceWith(box) }
      if (b instanceof HTMLImageElement) {
        const data = pictureData(a)
        b.removeAttribute('srcset'); b.removeAttribute('loading')
        if (data) b.src = data; else { b.removeAttribute('src'); b.alt = ''; b.style.visibility = 'hidden' }
      }
      // (what was scrolled inside the card is shown as it stood)
      if (a.scrollTop && b instanceof HTMLElement) for (const kid of b.children) kid.style.translate = `0 ${-a.scrollTop}px`
    }
    Object.assign(copy.style, { position: 'static', margin: '0', width: `${w}px`, height: `${h}px`, translate: 'none', transform: 'none', animation: 'none' })
    copy.setAttribute('xmlns', 'http://www.w3.org/1999/xhtml')
    const svg = `<svg xmlns="${NS}" width="${w}" height="${h}"><foreignObject width="100%" height="100%"><style xmlns="http://www.w3.org/1999/xhtml">${fonts.replace(/[<&]/g, ch => (ch === '<' ? '&lt;' : '&amp;'))}</style>${new XMLSerializer().serializeToString(copy)}</foreignObject></svg>`
    const url = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`
    const load = () => new Promise((resolve, reject) => { const image = new Image(); image.onload = () => resolve(image); image.onerror = reject; image.src = url })
    // (the picture's own fonts are read while it is painted the first time: the second painting has them)
    c.drawImage(await load(), 0, 0, canvas.width, canvas.height)
    await new Promise(r => setTimeout(r, 120))
    paper()
    c.drawImage(await load(), 0, 0, canvas.width, canvas.height)
    // (throws where the canvas is tainted; and a canvas that is still nothing but paper was not painted)
    const small = document.createElement('canvas'); small.width = small.height = 32
    const seen = small.getContext('2d'); seen.drawImage(canvas, 0, 0, 32, 32)
    if (seen.getImageData(0, 0, 32, 32).data.every(v => v === 255)) throw new Error('nothing was painted')
  } catch (err) {
    console.warn('card picture: painted from the page instead', err)
    // (with its pictures; without them if one of them taints the canvas; plain paper if that fails too)
    const painted = async pictures => { paper(); c.save(); c.scale(scale * w / card.getBoundingClientRect().width, scale * w / card.getBoundingClientRect().width); try { await paintCard(c, card, { pictures }); c.restore(); c.getImageData(0, 0, 1, 1); return true } catch (err2) { console.warn('card picture', err2); return false } }
    if (!(await painted(true)) && !(await painted(false))) paper()
  }
  drawing()
  return blob()
}

// ---- controller "card" ----
// A card's page. The page works without this: every answer is a button of one form.
// This adds what needs a script: the pencil that opens the note on one option, the draft kept on the hub while
// typing, Enter that sends, the names of attached files (also pasted or dropped), and the arrow of the pen from
// the picture shown to the option it belongs to.

const clamp = (n, lo, hi) => Math.min(Math.max(n, lo), hi)
let turn = 0
const GROWS = globalThis.CSS?.supports?.('field-sizing', 'content') ?? false

// A picture that is scrolled inside its stage (a tall one on the card; one at its own size, opened large) is also
// moved by dragging it with the mouse: a press and a move of more than a few pixels scrolls the stage, and the click
// that ends such a drag does not open the picture; a plain click still does. A finger and a pen scroll by themselves
// (nothing here), and so do the wheel and the scrollbar. While the mouse is on such a picture the cursor is a hand
// (card.css .can-drag, .is-dragged); nothing is sized or moved by it.
const DRAG_FROM = 5
const rolls = box => box.scrollHeight > box.clientHeight + 1 || box.scrollWidth > box.clientWidth + 1
if (typeof window !== 'undefined') {
  let dragEnded = false
  const scrollOf = e => (e.pointerType === 'mouse' && e.target instanceof Element ? e.target.closest('.tc-stage > .tc-scroll') : null)
  window.addEventListener('pointerover', e => { const box = scrollOf(e); if (box) box.classList.toggle('can-drag', rolls(box)) }, true)
  window.addEventListener('pointerdown', e => {
    const box = e.button === 0 ? scrollOf(e) : null
    if (!box || !rolls(box)) return
    // (a press on the stage's own scrollbar is the browser's)
    const r = box.getBoundingClientRect()
    if (e.clientX >= r.left + box.clientLeft + box.clientWidth || e.clientY >= r.top + box.clientTop + box.clientHeight) return
    box.classList.add('can-drag')
    const from = { x: e.clientX, y: e.clientY, left: box.scrollLeft, top: box.scrollTop }
    let dragged = false
    const move = m => {
      if (!dragged && Math.hypot(m.clientX - from.x, m.clientY - from.y) < DRAG_FROM) return
      if (!dragged) { dragged = true; box.classList.add('is-dragged'); getSelection()?.removeAllRanges() }
      box.scrollLeft = from.left - (m.clientX - from.x); box.scrollTop = from.top - (m.clientY - from.y)
    }
    const end = () => {
      window.removeEventListener('pointermove', move, true); window.removeEventListener('pointerup', end, true); window.removeEventListener('pointercancel', end, true)
      if (!dragged) return
      box.classList.remove('is-dragged')
      // (the click the release makes is the end of the drag, not a click on the picture)
      dragEnded = true
      setTimeout(() => { dragEnded = false }, 0)
    }
    window.addEventListener('pointermove', move, true); window.addEventListener('pointerup', end, true); window.addEventListener('pointercancel', end, true)
  }, true)
  window.addEventListener('click', e => { if (!dragEnded) return; dragEnded = false; e.preventDefault(); e.stopImmediatePropagation() }, true)
  // (no ghost of the picture or of its link under the pointer)
  window.addEventListener('dragstart', e => { if (e.target instanceof Element && e.target.closest('.tc-stage > .tc-scroll')) e.preventDefault() }, true)
}

// A picture opened large (Full screen, /card/<n>/picture/<m>): a click shows it at its own size to scroll, the next fits it again.
if (typeof window !== 'undefined') window.addEventListener('click', e => {
  const fig = e.target instanceof Element ? e.target.closest('.tc-page.is-full .tc-stage:not(.is-video) .tc-figure') : null
  if (!fig || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey) return
  e.preventDefault(); e.stopPropagation()
  const stage = fig.closest('.tc-stage'), on = stage.classList.toggle('is-zoomed')
  fig.title = on ? 'Fit it into the window' : 'Show it at its own size'
  if (!on) stage.querySelector('.tc-scroll')?.scrollTo(0, 0)
}, true)

controller('card', class extends Controller {
  static targets = ['form', 'field', 'files', 'chips', 'saved', 'figure', 'marks', 'where', 'said', 'page', 'gallery']
  static values = { draft: String, kept: Object, pictures: Array }

  connect() {
    this.fill()
    this.grow()
    if (this.hasMarksTarget) this.mountMarks()
    // (the card's pictures are fetched and decoded ahead, so a hover from option to option swaps at once)
    const warm = () => { this.warm = (this.picturesValue ?? []).map(p => { const i = new Image(); i.decoding = 'async'; i.src = p.src; i.decode?.().catch(() => {}); return i }) }
    if ('requestIdleCallback' in window) requestIdleCallback(warm, { timeout: 1500 }); else setTimeout(warm, 300)
    this.link = this.link.bind(this)
    this.placed = this.placed.bind(this)
    addEventListener('resize', this.placed)
    this.element.addEventListener('scroll', this.placed, { passive: true })
    this.element.querySelector('.tc-card > .tc-left')?.addEventListener('scroll', this.placed, { passive: true })
    // (no measuring while the page is built: the ResizeObserver below places it once the layout is there)
    // (the card grows after it was placed: a picture loads, a note's field opens, the talk arrives)
    this.sized = new ResizeObserver(this.placed)
    for (const el of this.element.querySelectorAll('.tc-card, .tc-pad')) this.sized.observe(el)
    addEventListener('resize', this.link)
    // (a finger's swipe across the card: the question before, the next; the round arrows are not there on a phone)
    this.element.addEventListener('touchstart', e => { const t = e.touches[0]; this.swipe = e.touches.length === 1 && t.clientX > 24 && e.target.closest?.('.tc-card') ? { x: t.clientX, y: t.clientY } : null }, { passive: true })   // (from the left edge a finger pulls the drawer: sidebar.mjs)
    this.element.addEventListener('touchend', e => {
      const from = this.swipe, t = e.changedTouches[0]; this.swipe = null
      if (!from || Math.abs(t.clientX - from.x) < 80 || Math.abs(t.clientY - from.y) > Math.abs(t.clientX - from.x) * .5) return
      this.element.querySelector(`.tc-rails a.${t.clientX < from.x ? 'is-next' : 'is-prev'}`)?.click()
    }, { passive: true })
    this.element.addEventListener('scroll', this.link, { capture: true, passive: true })
    this.sayLink = this.sayLink.bind(this)
    this.element.addEventListener('scroll', this.sayLink, { capture: true, passive: true })
    this.element.addEventListener('scroll', e => { if (e.target instanceof Element && e.target.classList.contains('tc-roll')) this.rolled() }, { capture: true, passive: true })
    // (in focus, every way to a card's page from here keeps the mode: the question before and the next, another
    //  picture, a picture opened large and the way back from it)
    this.element.addEventListener('click', e => {
      const a = e.target instanceof Element ? e.target.closest('a[href]') : null
      if (!a || a.matches('[data-action~="card#focus"]')) return
      const url = new URL(a.href, location.href)
      if (url.origin === location.origin && /^\/(?:chat\/[^/]+\/)?card\/[\w-]+(?:\/picture\/\d+)?$/.test(url.pathname)) a.href = focusAt(url.href, this.element.hasAttribute('data-focus'))
    }, true)
    this.element.addEventListener('load', e => { if (e.target.matches?.('.tc-figure img')) this.shape(e.target) }, true)
    // (the page is drawn again from the core's view model and morphed in place: a part that was drawn anew, as the
    //  card's left column, takes the notes on it along)
    this.morphed = () => { if (this.marksUi && !this.marksUi.scroll.isConnected && this.hasMarksTarget) { const list = this.marksUi.get(); this.marksUi = null; this.mountMarks().then(() => this.marksUi?.set(list)) } this.link() }
    document.addEventListener('trommi:morph', this.morphed)
    this.link()
  }
  // What stands unsent (the core's draft: the field's words, the ticks, the notes on options): put into the fields
  // once, as the page opens (cardPage: data-card-kept-value). The fields' markup never holds it, so a draft saved
  // while he types changes nothing under his hands.
  fill() {
    const kept = this.keptValue ?? {}
    if (this.hasFieldTarget && !this.fieldTarget.value && kept.note) this.fieldTarget.value = kept.note
    for (const key of kept.keys ?? []) { const box = this.element.querySelector(`input[name="keys"][value="${CSS.escape(key)}"]`); if (box) box.checked = true }
    for (const [key, said] of Object.entries(kept.notes ?? {})) {
      const row = this.element.querySelector(`.tc-opt-note[data-note="${CSS.escape(key)}"]`), field = row?.querySelector('input')
      if (!field || field.value) continue
      field.value = said; row.hidden = false
      this.element.querySelector(`.tc-opt-pen[data-key="${CSS.escape(key)}"]`)?.toggleAttribute('data-noted', true)
    }
  }
  // The shape of the picture that stands (cardMedia, shapeOf): from its known size, or from the picture once it has
  // come. The stage's size does not depend on it.
  shape(img, w = img.naturalWidth, h = img.naturalHeight) {
    const fig = img.closest('.tc-figure'), is = shapeOf(w, h)
    if (!fig || !(w > 0 && h > 0)) return
    for (const c of ['is-tall', 'is-slim']) fig.classList.toggle(c, is.includes(c))
  }
  disconnect() {
    this.sized?.disconnect()
    removeEventListener('resize', this.link)
    removeEventListener('resize', this.placed)
    this.element.removeEventListener('scroll', this.link, { capture: true })
    this.element.removeEventListener('scroll', this.sayLink, { capture: true })
    clearTimeout(this.timer)
    document.removeEventListener('trommi:morph', this.morphed)
    this.sheet?.close()
  }

  // ---- marks: a note at a paragraph, a drawing on the tracing sheet ----
  // They travel in the form's field "marks" with whatever is pressed, and in the draft while nothing is. A sheet that
  // was drawn on also goes as a picture of the card with the drawing (attached as the form is sent).
  async mountMarks() {
    const left = this.element.querySelector('.tc-left')
    if (!left || !this.element.isConnected || this.marksUi || !this.hasMarksTarget) return
    const labels = new Map([...this.element.querySelectorAll('.tc-opt[data-key]')].map(b => [b.dataset.key, b.querySelector('.tc-opt-label')?.textContent ?? b.dataset.key]))
    this.marksUi = cardMarks({
      scroll: left,
      blocks: () => [...left.querySelectorAll('.tc-title, .tc-text .rich > *, .focus-mark')],
      labelOf: key => labels.get(key) ?? key,
      onChange: () => { if (!this.hasMarksTarget) return; this.marksTarget.value = JSON.stringify(this.marksUi.get()); this.keep(); this.keepHere(); this.inked() },
    })
    // (what is written and drawn on the card is kept on this device; the core's draft takes it along, but its view
    //  model does not give it back: CardView.draft is the field's words only)
    if (this.marksTarget.value === '[]') try { this.marksTarget.value = localStorage.getItem(this.hereKey()) || '[]' } catch {}
    try { this.marksUi.set(JSON.parse(this.marksTarget.value || '[]')) } catch {}
    this.attachDrawing = this.attachDrawing.bind(this)
    this.formTarget.addEventListener('submit', this.attachDrawing)
    this.inked()
    // (a drawing kept from before: its picture is made ready, so an answer takes it along)
    if (this.sheetMark()) requestAnimationFrame(() => this.drawingPicture())
  }
  hereKey() { return `trommi-marks-${this.element.dataset.id}` }
  keepHere() {
    if (!this.hasMarksTarget) return
    try { const v = this.marksTarget.value; if (v && v !== '[]') localStorage.setItem(this.hereKey(), v); else localStorage.removeItem(this.hereKey()) } catch {}
  }
  /** The sheet of this view: over the card, or over the picture standing large in Full screen. */
  sheetId() {
    const at = this.element.classList.contains('is-full') ? Number(this.element.querySelector('.tc-stage[data-at]')?.dataset.at) || 1 : 0
    return at ? { id: `pen-pic-${at}`, anchor: { kind: 'picture', index: at - 1 } } : { id: 'pen-card', anchor: { kind: 'card' } }
  }
  sheetMark() { const m = this.marksUi?.sheet(this.sheetId().id); return m && (m.strokes?.length || m.words?.length) ? m : null }
  // The pen above the card, or in the field's row: lays the sheet on; again: takes it off.
  trace(event) {
    event?.preventDefault()
    if (!this.marksUi) return
    if (this.sheet?.open) return this.sheet.close()
    const { id, anchor } = this.sheetId()
    this.marksUi.setPen(true)
    this.sheet = traceSheet({
      card: this.element.querySelector('.tc-card'), marks: this.marksUi, id, anchor, left: this.element.querySelector('.tc-card > .tc-left'),
      // (the field's box, when it stands under the card: not in Full screen, where the card covers the page)
      dock: this.element.classList.contains('is-full') ? null : this.element.querySelector('.tc-chat > .tc-ask'),
      onClose: () => { this.marksUi.setPen(false); this.inked(); this.drawingPicture() },
      onSend: async () => {
        await this.drawingPicture()
        const send = this.formTarget.querySelector('.tc-send')
        if (send) this.formTarget.requestSubmit(send)
      },
    })
    this.inked()
  }
  /** The pens say whether a drawing waits on the card; the field's chips show it, a tap lays the sheet on again. */
  inked() {
    const on = Boolean(this.sheetMark()), open = Boolean(this.sheet?.open)
    for (const b of this.element.querySelectorAll('.tc-draw')) { b.toggleAttribute('data-inked', on); b.setAttribute('aria-pressed', String(open)) }
    if (!on) this.drawing = null
    this.files()
  }
  async drawingPicture() {
    const mark = this.sheetMark()
    if (!mark) { this.drawing = null; return null }
    const blob = await cardPicture(this.element.querySelector('.tc-card'), mark).catch(() => null)
    this.drawing = blob ? new File([blob], `drawing-on-card-${this.element.getAttribute('aria-label')?.match(/\d+/)?.[0] ?? 'card'}.png`, { type: 'image/png' }) : null
    return this.drawing
  }
  // (sent: the picture of the drawing joins the files, once; the router reads the form after this)
  attachDrawing(event) {
    // Sent while the sheet still lies on the card (the field's Send, Enter, Send and Reverse): the sheet comes off,
    // the picture of the drawing is made, and the same send goes again with it.
    if (this.sheet?.open) {
      event?.preventDefault(); event?.stopImmediatePropagation()
      const by = event?.submitter
      this.sheet.close()
      this.drawingPicture().then(() => this.formTarget.requestSubmit(by?.isConnected ? by : undefined))
      return
    }
    if (!this.drawing || !this.sheetMark() || !this.hasFilesTarget) return
    if ([...this.filesTarget.files].some(f => f.name === this.drawing.name)) return
    const all = new DataTransfer()
    for (const f of [...this.filesTarget.files, this.drawing]) all.items.add(f)
    this.filesTarget.files = all.files
  }

  // ---- the pictures and the options are one thing ----
  // While the pointer or the keyboard is on an option, its picture stands on the stage and its small picture is
  // marked; on a small picture, that picture stands there and its option is marked. Leaving puts back what stood.
  // A click on a small picture makes it the one that stands (its link). A finger has no hover: a tap on a small
  // picture shows it and marks its option; a tap on an option answers.
  preview(event) {
    if (event.pointerType === 'touch') return
    const key = event.target.closest?.('.tc-opt[data-key]')?.dataset.key
    this.say(key)
    const pic = key != null && this.picturesValue.find(p => (p.keys ?? [p.key]).includes(key))
    if (pic) this.stage(pic, key)
  }
  peek(event) {
    if (event.pointerType === 'touch') return
    const pic = this.picturesValue.find(p => p.at === Number(event.target.closest?.('.tc-thumb[data-at]')?.dataset.at))
    if (pic) this.stage(pic)
  }
  unpreview(event) {
    if (event.relatedTarget instanceof Element && event.currentTarget.contains(event.relatedTarget)) return
    if (event.currentTarget.classList?.contains('tc-opts')) this.say(null)
    if (this.stood) { const back = this.stood; this.stood = null; this.stage(back.pic, back.key, true) }
  }
  // key: the option the picture stands for now (a picture several options share points at the one under the pointer).
  stage(pic, key = pic.key, restoring = false) {
    if (!this.hasFigureTarget || !pic) return
    const fig = this.figureTarget
    // (a picture that could not be fetched left a word in its place, app.mjs watchAttachments: the next one gets a picture again)
    let img = fig.querySelector('img')
    if (!img) { img = document.createElement('img'); img.alt = ''; img.decoding = 'async'; img.draggable = false; const said = fig.querySelector('.att-gone'); if (said) said.replaceWith(img); else fig.prepend(img) }
    const now = Number(fig.dataset.at)
    if (!restoring && !this.stood) this.stood = { pic: this.picturesValue.find(p => p.at === now), key: fig.dataset.key ?? null }
    if (key != null) fig.dataset.key = key; else delete fig.dataset.key
    const thumbs = [...this.element.querySelectorAll('.tc-thumb[data-at]')]
    for (const t of thumbs) t.toggleAttribute('data-peek', !restoring && Number(t.dataset.at) === pic.at)
    if (now === pic.at && !this.pending) return this.link()
    // The picture that stands stays until the next one is decoded: no empty frame between the two, no flash.
    // A newer request wins over one still decoding.
    const token = this.pending = {}
    const next = new Image()
    next.src = pic.src
    const swap = () => {
      if (this.pending !== token) return
      this.pending = null
      delete img.dataset.attTried
      img.src = pic.src
      if (pic.width && pic.height) { img.width = pic.width; img.height = pic.height } else { img.removeAttribute('width'); img.removeAttribute('height') }
      img.style.width = pic.width ? `${pic.width}px` : ''
      this.shape(img, pic.width || next.naturalWidth, pic.height || next.naturalHeight)
      fig.closest('.tc-scroll')?.scrollTo(0, 0)
      this.link()
    }
    ;(next.decode ? next.decode() : Promise.resolve()).then(swap, swap)
    fig.dataset.at = pic.at
    fig.href = pic.href
    fig.dataset.circlesMarksValue = JSON.stringify(pic.marks ?? [])
    if (this.hasWhereTarget) this.whereTarget.textContent = `${pic.at} / ${this.element.querySelectorAll('.tc-roll .tc-thumb:not(.tc-thumb-file)').length || this.picturesValue.length}`
    if (this.hasSaidTarget) { this.saidTarget.textContent = pic.title ?? ''; this.saidTarget.hidden = !pic.title }
    if (this.hasPageTarget) {
      this.pageTarget.hidden = !pic.page
      if (pic.page) {
        const inApp = Boolean(pic.page.view), chip = this.pageTarget
        chip.href = pic.page.view ?? pic.page.url
        chip.querySelector('b').textContent = pic.page.name
        if (inApp) { chip.removeAttribute('target'); chip.removeAttribute('rel'); chip.dataset.nav = '' } else { chip.target = '_blank'; chip.rel = 'noopener noreferrer'; delete chip.dataset.nav }
      }
    }
    // (the way to the gallery opens the picture that stands; the gallery's way back returns to it)
    if (this.hasGalleryTarget) this.galleryTarget.href = 'back' in this.galleryTarget.dataset ? pic.href.replace(/\/picture\/(\d+)$/, '?pic=$1') : pic.href
    img.addEventListener('load', () => this.link(), { once: true })
    this.link()
  }
  // ---- Later is a pull: the tag under the card is drawn down (mouse or finger), the card follows the string; let go
  // past the threshold (or a plain press) and the card is pulled off the page, then the next one comes; let go before
  // it and both spring back. Reduced motion: nothing moves, the press does it.
  pullStart(event) {
    const tag = event.target.closest?.('.sel-later')
    if (!tag || event.button > 0 || this.pulling) return
    const card = this.element.querySelector('.tc-card'), y0 = event.clientY, calm = matchMedia('(prefers-reduced-motion: reduce)').matches
    let dy = 0
    const set = v => { dy = v; tag.style.setProperty('--pull', `${v}px`); if (card && !calm) card.style.translate = `0 ${(v * .4).toFixed(1)}px` }
    tag.setPointerCapture(event.pointerId)
    tag.classList.add('is-pulling')
    if (card) card.style.transition = 'none'
    const move = e => set(clamp(e.clientY - y0, 0, 240))
    const up = e => {
      tag.removeEventListener('pointermove', move); tag.removeEventListener('pointerup', up); tag.removeEventListener('pointercancel', up)
      tag.classList.remove('is-pulling')
      if (dy > 6) { this.dragged = true; setTimeout(() => { this.dragged = false }, 0) }   // (the click that follows a drag is not a press)
      if (e.type === 'pointerup' && dy > 72) return this.pullAway(tag)
      if (card) card.style.transition = 'translate 320ms cubic-bezier(.3, 1.7, .5, 1)'
      set(0)
    }
    tag.addEventListener('pointermove', move); tag.addEventListener('pointerup', up); tag.addEventListener('pointercancel', up)
  }
  pullClick(event) {
    const tag = event.target.closest?.('.sel-later')
    if (!tag || this.going) return
    event.preventDefault()
    if (!this.dragged && !this.pulling) this.pullAway(tag)
  }
  pullAway(tag) {
    this.pulling = true
    const go = () => { this.going = true; tag.form.requestSubmit(tag) }
    const card = this.element.querySelector('.tc-card')
    if (!card || matchMedia('(prefers-reduced-motion: reduce)').matches) return go()
    const from = parseFloat(card.style.translate.split(' ')[1]) || 0, far = innerHeight
    const how = { duration: 300, easing: 'cubic-bezier(.55, 0, .9, .45)', fill: 'forwards' }
    tag.animate([{ translate: `0 ${tag.style.getPropertyValue('--pull') || '0px'}` }, { translate: `0 ${far + 120}px` }], how)
    card.animate([{ translate: `0 ${from}px` }, { translate: `0 ${far}px`, rotate: '1.2deg' }], how).finished.then(go, go)
  }
  // Where the card stands against the window: gone (out of view above: the strip comes), and its foot passed (the
  // field to write in may stick to the window's foot without lying on the card).
  placed() {
    this.rolled()
    // (the left side that scrolls inside the card: no fade once its end is in view)
    const left = this.element.querySelector('.tc-card > .tc-left')
    if (left) left.classList.toggle('is-end', left.scrollHeight - left.scrollTop - left.clientHeight < 4)
    const card = this.element.querySelector('.tc-card')
    if (!card) return
    const box = this.element.getBoundingClientRect(), r = card.getBoundingClientRect()
    this.element.classList.toggle('card-gone', r.bottom < box.top + 90)
    this.element.classList.toggle('foot-passed', r.bottom < box.bottom - 96)
  }
  toAnswers() { this.element.querySelector('.tc-card')?.scrollIntoView({ block: 'start', behavior: matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth' }) }
  // ---- focus: the card's page alone in the window (card.css .is-focus), with the talk and the field ----
  // Switched in place, so what is written, drawn or scrolled stays; the address says it (?focus=1), so a reload and
  // back and forward keep it, and the card's forms say it, so the card that comes next is in focus too.
  focus(event) {
    event.preventDefault()
    const on = !this.element.hasAttribute('data-focus'), button = event.currentTarget, to = focusAt(location.href, on)
    this.element.toggleAttribute('data-focus', on)
    this.element.classList.toggle('is-focus', on)
    history.replaceState(history.state, '', to)
    if (window.trommi?.router?.page) window.trommi.router.page.path = to
    button.href = focusAt(location.href, !on)
    button.classList.toggle('is-leave', on)
    button.setAttribute('aria-pressed', String(on))
    button.removeAttribute('title'); button.dataset.tip = FOCUS_WORDS[on]; button.setAttribute('aria-label', FOCUS_WORDS[on])
    for (const form of this.element.querySelectorAll('form[method="post"]')) {
      form.querySelector(':scope > input[name="focus"]')?.remove()
      if (on) form.insertAdjacentHTML('afterbegin', FOCUS_FIELD)
    }
    this.placed()
  }
  // The bar of small ones ends before its counter ("2 / 5" has its own place beside it). Where more of it lies beyond
  // an edge, that edge fades (card.css .is-more-start, .is-more-end); a bar that fits has none.
  rolled() {
    for (const roll of this.element.querySelectorAll('.tc-roll')) {
      const more = roll.scrollWidth - roll.clientWidth
      roll.classList.toggle('is-more-start', more > 1 && roll.scrollLeft > 1)
      roll.classList.toggle('is-more-end', more > 1 && roll.scrollLeft < more - 1)
    }
  }
  // (another picture or a video came into the frame: it is the one that stands now)
  framed() {
    this.stood = null
    // (the one that stands is in view in the bar of small ones)
    const on = this.element.querySelector('.tc-roll .tc-thumb[aria-pressed="true"]'), roll = on?.closest('.tc-roll')
    if (roll && (on.offsetLeft < roll.scrollLeft || on.offsetLeft + on.offsetWidth > roll.scrollLeft + roll.clientWidth)) roll.scrollLeft = on.offsetLeft - (roll.clientWidth - on.offsetWidth) / 2
    const at = this.element.querySelector('.tc-stage[data-at]')?.dataset.at
    if (at && this.hasGalleryTarget) this.galleryTarget.href = this.galleryTarget.getAttribute('href').replace(/\/picture\/\d+$/, `/picture/${at}`)
    this.rolled()
    this.link()
  }

  // ---- the note on one option: the pencil opens its line ----
  note(event) {
    event.preventDefault()
    event.stopPropagation()
    const row = this.element.querySelector(`.tc-opt-note[data-note="${CSS.escape(event.currentTarget.dataset.key)}"]`)
    if (!row) return
    row.hidden = false
    row.querySelector('input').focus()
  }
  // (left empty, the line goes again; Enter in it is no answer)
  noteLeft({ currentTarget }) {
    const row = currentTarget.closest('.tc-opt-note'), said = currentTarget.value.trim()
    if (!said) row.hidden = true
    this.element.querySelector(`.tc-opt-pen[data-key="${CSS.escape(row.dataset.note)}"]`)?.toggleAttribute('data-noted', Boolean(said))
  }
  noteKey(event) { if (event.key === 'Enter') { event.preventDefault(); event.currentTarget.blur() } }

  // ---- the field ----
  typed() { this.grow(); this.keep() }
  // (Where the browser sizes a field to its content itself (field-sizing, card.css), no measuring: it costs a forced layout per page.)
  grow() { if (GROWS) return; const f = this.hasFieldTarget ? this.fieldTarget : null; if (f) { f.style.height = 'auto'; f.style.height = `${Math.min(f.scrollHeight, 220)}px` } }
  // Enter sends. Shift+Enter (and Ctrl/⌘+Enter) on a decision or an info sends and reverses: the words go back with the
  // card, for rework (the reverse card's own way: revise takes the field's words as the hand-back's message); where the
  // card cannot go back it is a new line. Alt+Enter (⌥+Enter) is a new line.
  keys(event) {
    if (event.key !== 'Enter' || event.isComposing) return
    if (event.altKey) {
      event.preventDefault()
      const f = this.fieldTarget
      f.setRangeText('\n', f.selectionStart, f.selectionEnd, 'end')
      f.dispatchEvent(new Event('input', { bubbles: true }))
      return
    }
    const reverse = (event.shiftKey || event.ctrlKey || event.metaKey) && this.reverseTile()
    if (event.shiftKey && !reverse) return
    if (!this.fieldTarget.value.trim() && !this.filesTarget.files.length) return
    event.preventDefault()
    this.formTarget.requestSubmit(reverse || this.formTarget.querySelector('.tc-send'))
  }
  reverseTile() { return this.element.querySelector('.tc-answer .tc-reverse') }
  // A click on Send while it wears the reverse card (Shift held: ui.mjs watchShift, <html data-shift>): send and
  // reverse, as Shift+Enter.
  sendClick(event) {
    const tile = document.documentElement.hasAttribute('data-shift') && this.reverseTile()
    if (!tile) return
    event.preventDefault()
    this.formTarget.requestSubmit(tile)
  }

  // ---- the draft: what is ticked and written is kept on the hub a moment after the last stroke ----
  keep() {
    if (!this.draftValue) return
    clearTimeout(this.timer)
    this.timer = setTimeout(() => this.save(), 700)
  }
  async save() {
    const body = new URLSearchParams()
    for (const [name, value] of new FormData(this.formTarget)) if (typeof value === 'string' && (name === 'note' || name === 'keys' || name === 'marks' || name.startsWith('note-'))) body.append(name, value)
    const say = (state, words) => { if (this.hasSavedTarget) { this.savedTarget.hidden = false; this.savedTarget.dataset.state = state; this.savedTarget.textContent = words } }
    try {
      const res = await fetch(this.draftValue, { method: 'POST', body })
      say(res.ok ? 'saved' : 'failed', res.ok ? 'Saved' : 'Not saved')
    } catch { say('failed', 'Not saved') }
  }
  // (an answer is on its way: a save that is still waiting must not come after it)
  sent() { clearTimeout(this.timer) }
  // A message sent with Send stays on the page (the comment comes in live under the card): the field, the files and
  // the marks that went along are emptied here. (Ticks and notes on options stay: they belong to the answer.)
  done(event) {
    const { formSubmission, success } = event.detail ?? {}
    if (!success || !formSubmission?.submitter?.classList.contains('tc-send')) return
    if (this.hasFieldTarget) { this.fieldTarget.value = ''; this.grow() }
    if (this.hasFilesTarget) { this.filesTarget.value = ''; this.files() }
    if (this.marksUi && this.hasMarksTarget) { this.marksUi.set([]); this.marksTarget.value = '[]'; this.keepHere(); this.drawing = null; this.inked() }
  }

  // ---- files: chosen, pasted or dropped ----
  files() {
    if (!this.hasFilesTarget || !this.hasChipsTarget) return
    const list = [...this.filesTarget.files].filter(f => !f.name.startsWith('drawing-on-card-'))
    const drawn = this.sheetMark() && !this.sheet?.open
    this.chipsTarget.hidden = !list.length && !drawn
    // (each a small preview with its own × : ui.mjs fileTile; the object URLs of the ones before are let go)
    revokeTiles(this.chipsTarget)
    this.chipsTarget.replaceChildren(...list.map(f => {
      const tile = fileTile(f)
      tile.querySelector('.att-off').addEventListener('click', () => { const left = new DataTransfer(); for (const x of this.filesTarget.files) if (x !== f) left.items.add(x); this.filesTarget.files = left.files; this.files() })
      return tile
    }))
    if (drawn) {
      const chip = document.createElement('button')
      chip.type = 'button'; chip.className = 'focus-chip is-drawing'; chip.textContent = 'Drawing on the card'; chip.title = 'Open drawing'
      chip.addEventListener('click', () => this.trace())
      this.chipsTarget.prepend(chip)
    }
  }
  add(list) {
    const all = new DataTransfer()
    for (const f of [...this.filesTarget.files, ...list]) all.items.add(f)
    this.filesTarget.files = all.files
    this.files()
  }
  paste(event) { const got = [...(event.clipboardData?.files ?? [])]; if (got.length) { event.preventDefault(); this.add(got) } }
  over(event) { if (event.dataTransfer?.types.includes('Files')) event.preventDefault() }
  drop(event) { const got = [...(event.dataTransfer?.files ?? [])]; if (got.length) { event.preventDefault(); this.add(got) } }

  // ---- the options explained: the pointer on a tile marks its block at the end of the left column ----
  // and draws the pen's arrow from the tile to the block's heading, where both stand side by side and in sight.
  // Nothing scrolls: the pointer on a tile never moves the left column away from the picture being looked at.
  say(key) {
    const host = this.element.querySelector('.tc-card'), box = host?.querySelector('.tc-say')
    if (!box) return
    const block = key != null ? box.querySelector(`.tc-say-one[data-key="${CSS.escape(key)}"]`) : null
    box.toggleAttribute('data-hover', Boolean(block))
    for (const b of box.querySelectorAll('.tc-say-one')) {
      b.toggleAttribute('data-match', b === block)
    }
    this.sayKey = block ? key : null
    this.sayLink()
  }
  sayLink() {
    const host = this.element.querySelector('.tc-card')
    host?.querySelector(':scope > .tc-ink > .focus-arrow.is-say')?.remove()
    const key = this.sayKey, box = host?.querySelector('.tc-say')
    const block = key != null ? box?.querySelector(`.tc-say-one[data-key="${CSS.escape(key)}"] > .tc-say-head`) : null
    const tile = key != null ? host.querySelector(`.tc-opts .tc-opt[data-key="${CSS.escape(key)}"]`) : null
    const left = host?.querySelector('.tc-left')
    if (!block || !tile || !left) return
    const base = host.getBoundingClientRect()
    const at = n => { const r = n.getBoundingClientRect(); return { x: r.left - base.left, y: r.top - base.top, w: r.width, h: r.height } }
    const T = at(tile), S = at(block), F = at(left)
    if (T.x < S.x + S.w - 1) return   // a narrow window: the answers stand above the words
    const y = S.y + S.h / 2
    if (y < F.y + 8 || y > F.y + F.h - 8) return
    const mid = T.y + T.h / 2, gx = Math.min(T.x - 14, F.x + F.w + 10)
    const end = [...block.children].reduce((r, c) => Math.max(r, c.getBoundingClientRect().right), 0) - base.left
    const to = [Math.min(end + 12, gx - 30), y]
    const points = [[T.x - 4, mid], [gx + 2, mid + (y > mid ? 6 : -6)], [gx - 4, mid + (y - mid) * .5], [gx - 6, y - (y > mid ? 12 : -12)], [gx - 16, y], to]
    const svg = document.createElementNS(NS, 'svg')
    svg.setAttribute('class', 'focus-arrow is-say')
    svg.setAttribute('aria-hidden', 'true')
    for (const d of arrowStrokes(points, `say:${key}:${turn++}`)) { const path = document.createElementNS(NS, 'path'); path.setAttribute('d', d); svg.append(path) }
    ;(host.querySelector(':scope > .tc-ink') ?? host).append(svg)
  }

  // ---- the arrow: one line of the pen from the picture shown to the option it belongs to ----
  // The picture names its option (data-key); the option is marked (data-match) and, where the answers stand beside
  // the picture and both ends are in sight, the arrow is drawn into the option's left edge.
  link() {
    const picture = this.hasFigureTarget ? this.figureTarget : null, key = picture?.dataset.key
    for (const b of this.element.querySelectorAll('.tc-opt[data-key]')) b.toggleAttribute('data-match', key != null && b.dataset.key === key)
    const host = this.element.querySelector('.tc-card')
    if (!host) return   // (the gallery: the option is marked, no arrow)
    const tile = host.querySelector('.tc-opt[data-match]'), answer = host.querySelector('.tc-right'), scroll = host.querySelector('.tc-left')
    const old = host.querySelector(':scope > .tc-ink > .focus-arrow:not(.is-say)')
    const gone = () => { old?.remove(); this.arrowSig = '' }
    if (!picture || !tile || !answer || !scroll) return gone()
    const base = host.getBoundingClientRect()
    const box = n => { const r = n.getBoundingClientRect(); return { x: r.left - base.left, y: r.top - base.top, w: r.width, h: r.height } }
    const P = box(picture.closest('.tc-stage') ?? picture), T = box(tile), A = box(answer), F = box(scroll), L = box(tile.parentElement)
    if (A.x < P.x + P.w - 1) return gone()   // a narrow window: the answers stand under the pictures
    const top = Math.max(P.y, F.y), bottom = Math.min(P.y + P.h, F.y + F.h)
    if (!P.w || bottom - top < 70 || T.y < L.y - 2 || T.y + T.h > L.y + L.h + 2) return gone()
    const ring = picture.querySelector(':scope > .focus-circles > path')?.getBoundingClientRect()
    const sig = [P.x, top, P.w, T.x, T.y, T.h, ring?.right ?? 0, ring?.top ?? 0].map(Math.round).join()
    if (sig === this.arrowSig && old) return
    this.arrowSig = sig
    old?.remove()
    const mid = T.y + T.h / 2
    // (it starts at the first circle where the agent marked a region, else near the picture's edge)
    const from = ring?.width ? [ring.right - base.left - 2, clamp(ring.top - base.top + ring.height / 2, top + 6, bottom - 6)] : [P.x + P.w - 30, clamp(mid - 46, Math.min(top + 62, bottom - 22), bottom - 22)]   // (under the "+" in its corner)
    const gx = Math.max(P.x + P.w + 12, T.x - 26)   // down the gap just before the options, never across the words
    const points = [from, [P.x + P.w - 6, from[1] + 5], [gx - 8, from[1] + 9], [gx + 4, from[1] + (mid - from[1]) * .45], [gx + 6, mid - (mid > from[1] ? 14 : -14)], [gx + 12, mid - (mid > from[1] ? 3 : -3)], [T.x - 2, mid]]
    const svg = document.createElementNS(NS, 'svg')
    svg.setAttribute('class', 'focus-arrow')
    svg.setAttribute('aria-hidden', 'true')
    for (const d of arrowStrokes(points, `${tile.dataset.key}:${turn++}`)) { const path = document.createElementNS(NS, 'path'); path.setAttribute('d', d); svg.append(path) }
    ;(host.querySelector(':scope > .tc-ink') ?? host).append(svg)
  }
})

// ---- controller "circles" ----
// The regions an agent marked on a picture (marks: [{ x, y, w, h, label? }] in fractions of the picture): each is
// circled with the pen over the picture as it is shown, its label beside it. The element holds the <img>.

controller('circles', class extends Controller {
  static values = { marks: Array }
  connect() {
    this.img = this.element.querySelector('img')
    if (!this.img) return
    this.layer = document.createElementNS(NS, 'svg')
    this.layer.setAttribute('class', 'focus-circles')
    this.layer.setAttribute('aria-hidden', 'true')
    if (getComputedStyle(this.element).position === 'static') this.element.style.position = 'relative'
    this.element.append(this.layer)
    this.draw = this.draw.bind(this)
    this.sizes = new ResizeObserver(this.draw)
    this.sizes.observe(this.img)
    this.img.addEventListener('load', this.draw)
    this.draw()
  }
  disconnect() { this.sizes?.disconnect(); this.img?.removeEventListener('load', this.draw); this.layer?.remove() }
  // (The card puts another picture on the stage with its marks: drawn anew.)
  marksValueChanged() { if (this.layer) this.draw() }
  draw() {
    const img = this.img, w = img.offsetWidth, h = img.offsetHeight
    if (!w || !h) return this.layer.replaceChildren()
    Object.assign(this.layer.style, { left: `${img.offsetLeft}px`, top: `${img.offsetTop}px`, width: `${w}px`, height: `${h}px` })
    const nodes = []
    this.marksValue.forEach((given, n) => {
      // (A region's size comes as width/height (the client core, the connector's ref) or as w/h: both are read.)
      const m = { ...given, w: Number(given.w ?? given.width), h: Number(given.h ?? given.height) }
      if (![m.x, m.y, m.w, m.h].every(Number.isFinite)) return
      const cx = (m.x + m.w / 2) * w, cy = (m.y + m.h / 2) * h
      const rx = Math.max(12, m.w * w / 2 * 1.16 + 5), ry = Math.max(12, m.h * h / 2 * 1.16 + 5)
      // one stroke of the pen, a little more than once round, never quite closing where it began
      let d = ''
      const turns = 34, seed = n * 7 + 3
      for (let i = 0; i <= turns; i++) {
        const t = -2.4 + (i / turns) * (Math.PI * 2 + .5)
        const wob = 1 + Math.sin(i * 1.7 + seed) * .035 + (i / turns - .5) * .05
        d += `${i ? 'L' : 'M'}${(cx + Math.cos(t) * rx * wob).toFixed(1)} ${(cy + Math.sin(t) * ry * wob).toFixed(1)}`
      }
      const path = document.createElementNS(NS, 'path')
      path.setAttribute('d', d)
      nodes.push(path)
      if (m.label) {
        const text = document.createElementNS(NS, 'text')
        text.setAttribute('x', Math.min(Math.max(cx - rx, 4), Math.max(4, w - 8 * String(m.label).length)).toFixed(1))
        text.setAttribute('y', (cy - ry < 22 ? cy + ry + 17 : cy - ry - 7).toFixed(1))
        text.textContent = m.label
        nodes.push(text)
      }
    })
    this.layer.replaceChildren(...nodes)
    this.dispatch('drawn')   // (the card draws its arrow from the first circle)
  }
})


// ---- the card's routes: its page, its picture, the forms that answer it ----
// Every way of answering is POST /cards/<id>/<what> (the card's page and the Desk's rows post there): it becomes the
// core's action. Answered from a Desk row (stay=1), the answer is a toast with Undo; from the card's own page (page=1)
// the page goes on: an answer keeps the card in view, answered, with the next question (?decided=1, CardView.after); the
// reverse card and Later go on to the next question (CardView.place.next); What?? goes back where the card was opened.
/** The toasts a way of answering shows, with the way that takes it back (its Undo). */
const SAID = {
  decide: { head: 'Answered', back: 'reopen' }, verdict: { head: 'Answered' }, trust: { head: WORDS.trust, back: 'reopen' }, close: { head: 'Read', back: 'reopen' },
  shred: { head: 'Shredded', back: 'reopen' }, snooze: { head: WORDS.later, back: 'wake' }, revise: { head: 'Handed back', back: 'takeback' }, what: { head: `Asked: ${WORDS.what}`, back: 'takeback' },
}
/** The ways that settle a card: on its own page each is followed by the next question. */
export const ANSWERS = new Set(['decide', 'verdict', 'trust', 'close', 'shred'])
const one = x => (x ? [x] : [])
/** After a message or a hand-back from the card's field, its draft keeps no words and no drawing. */
const forget = card => (card?.draft ? [{ do: 'draft', place: card.id, text: '', marks: [] }] : [])
/** The core's actions for one way of answering (a list; an Error says what is missing). */
const WAYS = {
  decide({ id, card, f, note, notes, marks, attachments }) {
    const keys = f.getAll('keys').map(String), choices = keys.length ? keys : f.has('key') ? [String(f.get('key'))] : []
    if (!choices.length) throw new Error('Pick an option first.')
    if (card?.kind === 'request') return [{ do: 'verdict', request_id: id, allow: choices[0] === 'allow' }]
    return [{ do: 'answer', card_id: id, choices, ...(note ? { note } : {}), attachments, option_notes: notes, marks }]
  },
  verdict: ({ id, f }) => [{ do: 'verdict', request_id: id, allow: f.get('key') === 'allow' }],
  // (the agent still chooses; its recommendation goes as the choices: the card's in view, else the Desk row's tile)
  trust: ({ id, vm, card, note }) => [{ do: 'answer', card_id: id, choices: card ? card.options.filter(o => o.recommended).map(o => o.key) : (vm.desk.rows?.find(r => r.id === id)?.tiles.tiles ?? []).filter(x => x.advised).map(x => x.key), trusted: true, ...(note ? { note } : {}) }],
  close: ({ id }) => [{ do: 'read', card_id: id }],
  snooze: ({ id }) => [{ do: 'snooze', card_id: id }],
  wake: ({ id }) => [{ do: 'unsnooze', card_id: id }],
  star: ({ id }) => [{ do: 'star', card_id: id, on: true }],
  unstar: ({ id }) => [{ do: 'star', card_id: id, on: false }],
  shred: ({ id, note, marks, attachments }) => [{ do: 'shred', card_id: id, ...(note ? { note } : {}), attachments, marks }],
  reopen: ({ id }) => [{ do: 'decide_again', card_id: id }],
  takeback: ({ id }) => [{ do: 'take_back', card_id: id }],
  // (HandBack carries no files: what was attached, the drawing's picture too, goes as a comment just before it)
  revise: ({ id, card, note, marks, attachments }) => [...(attachments.length && card?.session_id ? [{ do: 'write', session_id: card.session_id, card_id: id, text: '', attachments }] : []), { do: 'hand_back', card_id: id, ...(note ? { note } : {}), rework: true, marks }, ...forget(card)],
  what: ({ id, marks }) => [{ do: 'hand_back', card_id: id, rework: false, marks }],
  message({ id, card, note, marks, attachments }) {
    if (!note && !attachments.length && !marks.length) throw new Error('Write something first.')
    if (!card?.session_id) throw new Error('This question is not on the board any more.')
    return [{ do: 'write', session_id: card.session_id, card_id: id, text: note, attachments, marks }, ...forget(card)]
  },
  draft: ({ id, f, notes, marks }) => [{ do: 'draft', place: id, text: String(f.get('note') ?? ''), keys: f.getAll('keys').map(String), notes, ...(f.has('marks') ? { marks } : {}) }],
  more: ({ id }) => [{ do: 'show_more', of: { list: 'comments', card: id } }],
}

export function register(t) {
  const { stream } = t
  // (ui.mjs fileSrc: a picture's object URL, the core asked once; app.mjs does not hand it over yet)
  if (t.fileUrl) useFiles(t.fileUrl)
  const gone = (req, res) => t.notFound(req, res, 'This question is not on the board any more.')
  /** The session a card was opened from (/chat/<slug>/card/<n>): its address, its key, its name. */
  const fromOf = (vm, slug) => { if (!slug) return null; const s = vm.sidebar.sessions.find(x => x.slug === slug || x.id === slug); return { slug, id: s?.id ?? null, name: s?.name.en ?? 'the session' } }
  const show = (req, res, ref, slug, opts) => {
    const vm = t.vm(), card = cardIn(vm, ref)
    if (!card) return gone(req, res)
    const from = fromOf(vm, slug)
    t.page(req, res, { title: `${card.title} · Trommi`, view: 'card', current: from?.id ?? null, main: cardPage(card, vm, { ...opts, from }) })
  }
  const dec = x => (x ? decodeURIComponent(x) : null)
  t.get(/^\/(?:chat\/([^/]+)\/)?card\/([\w-]+)$/, ({ req, res, url, match: [, slug, ref] }) => {
    const q = url.searchParams
    show(req, res, dec(ref), dec(slug), { pic: Number(q.get('pic')) || 1, walk: q.has('walk'), focus: q.has('focus'), version: Number(q.get('v')) || null, decided: q.has('decided') && !q.has('walk') })
  })
  // A picture opened large (the card's page with the stage alone).
  t.get(/^\/(?:chat\/([^/]+)\/)?card\/([\w-]+)\/picture\/(\d+)$/, ({ req, res, url, match: [, slug, ref, at] }) => {
    const vm = t.vm(), card = cardIn(vm, dec(ref)), focus = url.searchParams.has('focus')
    if (!card) return gone(req, res)
    const m = mediaOf(vm, card)
    if (!m.pictures.length && !m.videos.length) return t.redirect(res, focusAt(cardPath(card, slug ? `/chat/${slug}` : ''), focus))
    show(req, res, card.id, dec(slug), { pic: Number(at) || 1, full: true, focus })
  })
  // The page a picture was made from (PictureView.page): shown in the sandboxed frame, never as a page of the app's own
  // origin (controller "assetthumb": its bytes are decrypted here and handed to /frame as one message).
  t.get(/^\/(?:chat\/([^/]+)\/)?card\/([\w-]+)\/picture\/(\d+)\/page$/, ({ req, res, match: [, slug, ref, at] }) => {
    const vm = t.vm(), card = cardIn(vm, dec(ref))
    if (!card) return gone(req, res)
    const p = card.pictures?.[Number(at) - 1], here = cardPath(card, slug ? `/chat/${slug}` : '')
    if (!p?.page) return t.redirect(res, here)
    const url = t.fileUrl(p.page), name = fileOf(vm, p.page).name
    const main = html`<div class="t-picture as-view"><header class="t-picture-bar"><a class="t-picture-back" data-nav href="${here}?pic=${Number(at)}" aria-label="Back to the card">${sk('back')}<h1>${card.title}</h1></a><span class="t-picture-where"><b>Page</b> ${name}</span></header>
<div class="as-stage" data-type="html">${url ? html`<div class="as-frame-box" data-controller="assetthumb" data-assetthumb-src-value="${url}" data-assetthumb-full-value="true"><p class="as-wait">Opening the page…</p></div>` : html`<p class="as-wait">Opening the page…</p>`}</div></div>`
    t.page(req, res, { title: `${name} · Trommi`, view: 'picture', sidebar: false, main, bodyAttrs: ' data-focus-page="card"' })
  })

  /** Files picked on the card, attached: their ids once the core has them uploaded (an answer names only those). */
  async function attachAll(files) {
    const ids = await Promise.all(files.map(f => t.attach(f)))
    for (const until = Date.now() + 120_000; ;) {
      const states = ids.map(id => t.vm().files?.[id]?.state)
      if (states.every(s => s === 'ready')) return ids
      if (states.includes('failed') || Date.now() > until) throw new Error('A file could not be uploaded.')
      await new Promise(r => setTimeout(r, 150))
    }
  }
  t.post(/^\/cards\/([0-9a-f]+)\/([a-z]+)$/, async ({ req, res, match: [, id, what], form }) => {
    if (!Object.hasOwn(WAYS, what)) return false
    const f = form ?? new FormData(), vm = t.vm(), card = vm.cards?.[id] ?? null
    const note = String(f.get('note') ?? '').trim()
    let marks = []
    try { const list = JSON.parse(String(f.get('marks') ?? '[]')); if (Array.isArray(list)) marks = list } catch {}
    const notes = Object.fromEntries([...f.keys()].filter(k => k.startsWith('note-')).map(k => [k.slice(5), String(f.get(k) ?? '').trim()]).filter(([, said]) => said))
    const next = card?.place?.next ?? null   // (the question after it, before the answer moves it)
    try {
      const files = what === 'draft' ? [] : f.getAll('files').filter(v => typeof v === 'object' && v?.size > 0)
      const attachments = files.length ? await attachAll(files) : []
      for (const a of WAYS[what]({ id, vm, card, f, note, notes, marks, attachments })) t.act(a)
    } catch (err) {
      return t.sendStream(req, res, t.toast({ head: what === 'message' ? 'Not sent' : 'Not saved', line: String(err?.message ?? err), role: 'alert' }), 422)
    }
    if (what === 'draft' || what === 'more') return t.done(res)
    // Undone from a toast: back to that card, unless its page is in view already.
    if (f.has('undo')) return /\/card\/[^/]+$/.test(new URL(String(req.headers?.referer ?? '/'), 'http://x').pathname) ? t.done(res) : t.sendStream(req, res, stream('visit', cardPath(card ?? id)))
    const said = !f.has('quiet') && SAID[what]
    const toast = said ? t.toast({ head: said.head, line: card?.title ?? vm.desk.rows?.find(r => r.id === id)?.title ?? '', undo: said.back ? { action: `/cards/${id}/${said.back}` } : null }) : ''
    let to = null
    if (f.has('page')) {
      const back = String(f.get('back') ?? ''), home = /^\/chat\/[^/?#]+$/.test(back) ? back : '/', self = home === '/' ? '' : home
      const walk = f.has('walk'), onward = ANSWERS.has(what) || what === 'snooze' || what === 'revise'
      if (walk && onward) to = next ? `${cardPath(next, self)}?walk=1` : '/'
      else if (ANSWERS.has(what)) to = `${cardPath(card ?? id, self)}?decided=1`
      else if (what === 'snooze' || (what === 'revise' && f.has('next'))) to = next ? cardPath(next, self) : home
      else if (what === 'what' || what === 'revise') to = home
      if (to) to = focusAt(to, f.has('focus'))
    }
    return toast || to ? t.sendStream(req, res, html`${toast}${to ? stream('visit', to) : ''}`) : t.done(res)
  })
}
