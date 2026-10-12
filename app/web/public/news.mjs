// What's new (/whats-new, a row of the Trommi menu): CHANGELOG.md at the repository's root, as the build read it
// (gen/vendor/changelog.mjs; never fetched), newest first. The newest three days are open: the date, the summary line,
// the day's picture small under it (public/changelog/, a shot of the demo room), then New, Better and Fixed with
// their first three lines by their short form (dev/build.mjs changelogHead), the rest under "+N more"; a line opens to its whole text. Older days are one row with their counts, open on a tap.
// Opening it marks the newest day as seen on this device: the menu row's dot (sidebar.mjs) goes. The iOS app shows
// the same file in its menu, the same way (WhatsNew.swift).
import { changelog, runningLine } from './app.mjs'
import { Controller, controller, html, raw, settingsPage, sk, SET_CHEVRON } from './ui.mjs'

const { DAYS } = await changelog()
const LATEST = DAYS[0]?.day ?? ''
export const SEEN_KEY = 'trommi-news-seen'
const TITLE = "What's new"
const MARK = { New: 'spark', Better: 'wake', Fixed: 'tick' }

/** An item's words: plain text, a `code` span kept as code, a [link](https://…) as a link. */
const words = text => raw(String(text).split(/(`[^`]+`|\[[^\]]+\]\(https:\/\/[^)\s]+\))/).map(part => {
  if (/^`[^`]+`$/.test(part)) return `<code>${esc(part.slice(1, -1))}</code>`
  const link = /^\[([^\]]+)\]\((https:\/\/[^)\s]+)\)$/.exec(part)
  if (link) return `<a href="${esc(link[2])}" target="_blank" rel="noopener">${esc(link[1])}</a>`
  return esc(part)
}).join(''))
const esc = s => s.replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c])

const OPEN_DAYS = 3   // the newest days shown open
const TOP = 3         // the lines of a part shown before "+N more"
const ORDER = ['New', 'Better', 'Fixed']
const parts = d => [...d.groups].sort((a, b) => (ORDER.indexOf(a.kind) + 4) % 4 - (ORDER.indexOf(b.kind) + 4) % 4)
/** "19 new · 41 better · 35 fixed" */
const counts = d => parts(d).map(g => `${g.items.length} ${(g.kind || 'changes').toLowerCase()}`).join(' · ')
const line = item => html`<li><details class="news-line"><summary>${item.tag ? html`<span class="news-tag">${item.tag}</span>` : ''}<span>${item.head}</span></summary><p>${words(item.text)}</p></details></li>`
const part = g => html`<div class="news-part" data-kind="${g.kind.toLowerCase()}">${g.kind ? html`<p class="news-kind">${MARK[g.kind] ? sk(MARK[g.kind]) : ''}<b>${g.kind}</b><small>${g.items.length}</small></p>` : ''}<ol>${g.items.slice(0, TOP).map(line)}</ol>${g.items.length > TOP ? html`<details class="news-rest"><summary>+${g.items.length - TOP} more</summary><ol start="${TOP + 1}">${g.items.slice(TOP).map(line)}</ol></details>` : ''}</div>`
const body = (d, picture) => html`${d.summary ? html`<p class="news-summary">${words(d.summary)}</p>` : ''}${picture && d.picture ? html`<img class="news-pic" src="${d.picture.src}" alt="${d.picture.alt}" loading="lazy" decoding="async">` : ''}<div class="set-group news-parts">${parts(d).map(part)}</div>`
const day = (d, i) => i < OPEN_DAYS
  ? html`<section class="news-day" aria-labelledby="news-day-${i}"><h3 class="news-date" id="news-day-${i}">${d.day}${d.releases ? html` <small>${d.releases}</small>` : ''}</h3>${body(d, true)}</section>`
  : html`<details class="news-day news-old"><summary><span class="news-date">${d.day}</span><small>${counts(d)}</small>${SET_CHEVRON}</summary>${body(d, false)}</details>`

const newsMain = () => settingsPage(TITLE, html`<div class="news" data-controller="news"><p class="news-build" id="news-build">${runningLine()}</p>${DAYS.length ? DAYS.map(day) : html`<p class="set-note">Nothing written down yet.</p>`}</div>`, { back: false, lead: 'What changed in Trommi, newest first.' })

export function register(t) {
  t.get(/^\/whats-new$/, ({ req, res }) => t.page(req, res, { title: `${TITLE} · Trommi`, view: 'whats-new', css: 'room', main: newsMain() }))
}

// The page seen: the newest day is remembered on this device, and the menu's dot goes at once.
controller('news', class extends Controller {
  connect() {
    try { localStorage.setItem(SEEN_KEY, LATEST) } catch {}
    const dot = document.getElementById('menu-news-dot')
    if (dot) dot.hidden = true
    // what runs here (app.mjs runningLine), filled in when it becomes known after the page was drawn
    this.fill = () => { const line = document.getElementById('news-build'); if (line) line.textContent = runningLine() }
    this.fill()
    document.addEventListener('trommi:news', this.fill)
  }
  disconnect() { document.removeEventListener('trommi:news', this.fill) }
})
