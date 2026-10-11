// What's new (/whats-new, a row of the Trommi menu): CHANGELOG.md at the repository's root, as the build read it
// (gen/vendor/changelog.mjs; never fetched), one day under the other, newest first: its summary and its picture
// (public/changelog/, a shot of the demo room), its parts New, Better, Fixed folded under "All changes" (open when the
// day has no summary).
// Opening it marks the newest day as seen on this device: the menu row's dot (sidebar.mjs) goes. The iOS app shows
// the same file in its menu.
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

/** "12 new · 29 better · 25 fixed": how many lines each part has. */
const counts = d => d.groups.map(g => `${g.items.length} ${(g.kind || 'changes').toLowerCase()}`).join(' · ')
const parts = d => html`<div class="news-parts">${d.groups.map(g => html`<div class="news-part" data-kind="${g.kind.toLowerCase()}">${g.kind ? html`<p class="news-kind">${MARK[g.kind] ? sk(MARK[g.kind]) : ''}<b>${g.kind}</b></p>` : ''}<ul>${g.items.map(item => html`<li>${words(item)}</li>`)}</ul></div>`)}</div>`
const day = (d, i) => html`<section class="news-day" aria-labelledby="news-day-${i}">
<h3 class="news-date" id="news-day-${i}">${d.day}${d.releases ? html` <small>${d.releases}</small>` : ''}</h3>
<div class="set-group news-card">${d.summary ? html`<p class="news-summary">${words(d.summary)}</p>` : ''}${d.picture ? html`<img class="news-pic" src="${d.picture.src}" alt="${d.picture.alt}" loading="${i ? 'lazy' : 'eager'}" decoding="async">` : ''}
<details class="news-more"${d.summary ? '' : raw(' open')}><summary><b>All changes</b><small>${counts(d)}</small>${SET_CHEVRON}</summary>${parts(d)}</details></div>
</section>`

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
