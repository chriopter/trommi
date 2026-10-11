// What's new (/whats-new, a row of the Trommi menu): CHANGELOG.md at the repository's root, as the build read it
// (gen/vendor/changelog.mjs; never fetched), one day under the other, newest first, its parts New, Better, Fixed.
// Opening it marks the newest day as seen on this device: the menu row's dot (sidebar.mjs) goes. The iOS app shows
// the same file in its menu.
import { changelog, runningLine } from './app.mjs'
import { Controller, controller, html, raw, settingsPage, sk } from './ui.mjs'

const { DAYS } = await changelog()
const LATEST = DAYS[0]?.day ?? ''
export const SEEN_KEY = 'trommi-news-seen'
const TITLE = "What's new"
const MARK = { New: 'spark', Better: 'wake', Fixed: 'tick' }

/** An item's words: plain text, a `code` span kept as code. */
const words = text => raw(String(text).split(/(`[^`]+`)/).map(part => (/^`[^`]+`$/.test(part) ? `<code>${esc(part.slice(1, -1))}</code>` : esc(part))).join(''))
const esc = s => s.replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c])

const newsMain = () => settingsPage(TITLE, html`<div class="news" data-controller="news"><p class="news-build" id="news-build">${runningLine()}</p>${DAYS.length ? DAYS.map((d, i) => html`<section class="news-day" aria-labelledby="news-day-${i}">
<h3 class="news-date" id="news-day-${i}">${d.day}${d.releases ? html` <small>${d.releases}</small>` : ''}</h3>
<div class="set-group news-parts">${d.groups.map(g => html`<div class="news-part" data-kind="${g.kind.toLowerCase()}">${g.kind ? html`<p class="news-kind">${MARK[g.kind] ? sk(MARK[g.kind]) : ''}<b>${g.kind}</b></p>` : ''}<ul>${g.items.map(item => html`<li>${words(item)}</li>`)}</ul></div>`)}</div>
</section>`) : html`<p class="set-note">Nothing written down yet.</p>`}</div>`, { back: false, lead: 'What changed in Trommi, newest first.' })

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
