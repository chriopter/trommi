// The demo's own pages: /screens, a review page of every screen and state as live frames, and the ?state= clicks
// that put a frame into a state. The demo room itself is the app core's (Config.demo: the fixture of
// /demo/fixture.json runs in the core, app.mjs); nothing of the room is made here.
import { STATES } from '../gen/vendor/demo-screens.mjs'

// ---- All screens (demo only): /screens, a review page of every screen and state as live frames ----
// The Trommi menu shows "All screens" while the demo is on (sidebar.mjs); app.mjs adds this view only in the demo. A
// state that needs a click is driven by ?state=<name> on the frame's address (demoState, run once after the page is in).
// The screens that really exist: one per page of the app, each with its states beside it (a menu
// open, the selection, a toast, empty…) that switch the same pair of frames in place. The list is data, the same for
// every client: demo/data/screens.json in the repository (demo/README.md), which the build hands over as
// gen/vendor/demo-screens.mjs. A state there is { id, screen, state, web: { path, state, mock } } (path: the address;
// web.state: demoState's click; mock: a demo room); here [title, path, states, first state, room], a state
// [label, path, state, mock].
const SCREENS = []
for (const s of STATES) {
  const state = [s.state, s.web.path, s.web.state ?? '', s.web.mock]
  if (SCREENS.at(-1)?.[0] === s.screen) SCREENS.at(-1)[2].push(state)
  else SCREENS.push([s.screen, state[1], [], state[2], state[3]])
}
const frameSrc = (path, state, mock = '1') => `${path}${path.includes('?') ? '&' : '?'}mock=${mock || '1'}${state ? `&state=${state}` : ''}`
function screensMain() {
  // One screen: its title (opens it alone), its states as small buttons; the pair of frames shows the chosen one.
  const screen = ([title, path, states, first = '', base = '']) => {
    const all = [['As it is', path, first, base], ...states.map(([label, p, st, mock]) => [label, p || path, st ?? '', mock])]
    const src = frameSrc(path, first, base)
    const list = states.length ? `<div class="scr-states" role="group" aria-label="${title}: states">${all.map(([label], i) => `<button type="button" class="scr-state" data-action="screens#state" data-at="${i}"${i ? '' : ' aria-pressed="true"'}>${label}</button>`).join('')}</div>` : ''
    return `<figure class="scr-item" data-states='${JSON.stringify(all.map(([label, p, st, mock]) => ({ label, src: frameSrc(p, st, mock) }))).replace(/'/g, '&#39;')}'><figcaption><a href="${src}" target="_blank" rel="noopener" class="scr-title">${title}</a> <code>${path}</code></figcaption><div class="scr-row"><div class="scr-pair"><div class="scr-box is-wide"><iframe data-src="${src}" title="${title}, desktop" loading="lazy" width="1440" height="900" tabindex="-1"></iframe></div><div class="scr-box is-phone"><iframe data-src="${src}" title="${title}, phone" loading="lazy" width="390" height="844" tabindex="-1"></iframe></div></div>${list}</div></figure>`
  }
  return `<main id="screens" class="scr-page" data-controller="screens"><link rel="stylesheet" href="/demo/screens.css"><header class="scr-head"><h1>All screens</h1><p>Every screen once, its states beside it; a title opens it alone.</p><button type="button" class="scr-theme is-tour" data-action="screens#tour">▶ Play tour</button><button type="button" class="scr-theme" data-action="screens#eager" title="Load all">Load all</button><button type="button" class="scr-theme" data-action="screens#theme">Light / dark</button></header>
<div class="scr-grid">${SCREENS.length ? SCREENS.map(screen).join('') : '<p class="scr-none">No screens are listed (demo/data/screens.json has no states).</p>'}</div></main>`
}
export const screensView = { register(t) { t.get(/^\/screens$/, ({ req, res }) => t.page(req, res, { title: 'All screens · Trommi', view: 'screens', sidebar: false, stream: null, main: screensMain() })) } }

/** Frames load as they come into view; the page's light/dark goes into every frame. */
export function screensController({ Controller, controller }) {
  controller('screens', class extends Controller {
    connect() {
      this.dark = document.documentElement.dataset.theme === 'dark'
      this.io = new IntersectionObserver(es => { for (const e of es) if (e.isIntersecting) { const f = e.target; f.src = f.dataset.src; f.addEventListener('load', () => this.paint(f)); this.io.unobserve(f) } }, { rootMargin: '400px' })
      for (const f of this.element.querySelectorAll('iframe[data-src]')) this.io.observe(f)
      if (new URLSearchParams(location.search).get('tour') === '1') requestAnimationFrame(() => this.tour())
      this.ro = new ResizeObserver(() => this.layout())
      this.ro.observe(this.element)
      this.layout()
    }
    disconnect() { this.io?.disconnect(); this.ro?.disconnect(); this.endTour() }
    // The whole width: the pair of frames as large as the row allows (never above its real size), the states at the far
    // right in as few columns as keep their list no taller than the frames. A narrow window: the frames stacked, the
    // states a wrapping grid under them (screens.css).
    layout() {
      const W = this.element.clientWidth - 2 * parseFloat(getComputedStyle(this.element).paddingLeft || '0')
      if (!(W > 0) || W === this.laidOut) return
      this.laidOut = W
      const narrow = W < 860, COL = 196, GAP = 6, ROW = 30, PAIR = 1440 + 390 + 12
      for (const item of this.element.querySelectorAll('.scr-item')) {
        const n = item.querySelectorAll('.scr-state').length
        let cols = n ? 1 : 0, s = Math.min(1, W / 1440), p = Math.min(.6, W / 390)
        if (!narrow) {
          const fit = k => Math.min(1, (W - (k ? k * COL + (k - 1) * GAP + 20 : 0)) / PAIR)
          while (n && cols < 4 && Math.ceil(n / cols) * ROW > 900 * fit(cols)) cols++
          s = p = fit(cols)
        }
        item.style.setProperty('--s', s.toFixed(4)); item.style.setProperty('--p', p.toFixed(4)); item.style.setProperty('--cols', String(cols || 1)); item.style.setProperty('--rows', String(Math.ceil(n / (cols || 1)) || 1))
      }
      this.element.classList.toggle('is-narrow', narrow)
    }
    // A state of a screen: the same pair of frames shows it
    state(e) {
      const b = e.currentTarget, item = b.closest('.scr-item'), st = JSON.parse(item.dataset.states)[Number(b.dataset.at)]
      for (const x of item.querySelectorAll('.scr-state')) x.setAttribute('aria-pressed', String(x === b))
      const url = new URL(st.src, location.origin), q = url.searchParams, mock = q.get('mock') || '1', name = q.get('state') || ''
      q.delete('mock'); q.delete('state')
      const path = url.pathname + (q.toString() ? `?${q}` : '')
      const t0 = performance.now()
      for (const f of item.querySelectorAll('iframe')) {
        const w = f.contentWindow, app = f.src && (() => { try { return w.trommi } catch { return null } })()
        if (app?.router?.forget && (f.dataset.mock || '1') === mock) {
          // the same demo room: the frame's own router paints the page anew and its click is made in place (no reload)
          f.dataset.src = st.src
          try { w.document.querySelectorAll('#says-host > *').forEach(n => n.remove()) } catch {}
          app.router.forget()
          Promise.resolve(app.router.visit(path, { action: 'replace' })).then(() => app.demoState?.(name, { now: true })).then(() => { item.dataset.took = String(Math.round(performance.now() - t0)); this.paint(f) })
        } else {
          this.io.unobserve(f); f.dataset.src = st.src; f.dataset.mock = mock; f.src = st.src
          f.addEventListener('load', () => { this.paint(f); item.dataset.took = String(Math.round(performance.now() - t0)) }, { once: true })
        }
      }
      item.querySelector('.scr-title').href = st.src
    }
    // Load every frame now (for scrolling through all of them)
    eager() { for (const f of this.element.querySelectorAll('iframe[data-src]:not([src])')) { this.io.unobserve(f); f.src = f.dataset.src; f.addEventListener('load', () => this.paint(f)) } }
    // ---- the tour: one screen at a time at its real size, desktop then phone, held a while once it is in ----
    stops() {
      // every screen, and within it each of its states
      const out = []
      for (const el of this.element.querySelectorAll('.scr-item')) {
        const group = el.querySelector('.scr-title').textContent
        for (const st of JSON.parse(el.dataset.states)) out.push({ group, name: st.label, src: st.src })
      }
      return out
    }
    tour() {
      if (this.box) return
      this.list = this.stops(); this.at = 0; this.playing = true; this.hold = 4000
      if (!this.list.length) return
      const box = this.box = document.createElement('div')
      box.className = 'tour'
      box.innerHTML = '<div class="tour-stage"></div><div class="tour-bar" role="toolbar" aria-label="Tour"><button type="button" class="tour-play">Pause</button><button type="button" class="tour-prev" title="Back (←)">←</button><button type="button" class="tour-next" title="The next (→)">→</button><select class="tour-speed" aria-label="How long each screen stands"><option value="2000">2 s</option><option value="4000" selected>4 s</option><option value="8000">8 s</option></select><span class="tour-where"></span><span class="tour-wait"></span><button type="button" class="tour-close" title="Grid (Esc)">Close</button></div>'
      document.body.append(box)
      const q = s => box.querySelector(s)
      q('.tour-play').addEventListener('click', () => this.toggle())
      q('.tour-prev').addEventListener('click', () => this.go(this.at - 1))
      q('.tour-next').addEventListener('click', () => this.go(this.at + 1))
      q('.tour-speed').addEventListener('change', e => { this.hold = Number(e.target.value); this.arm() })
      q('.tour-close').addEventListener('click', () => this.endTour())
      this.keys = e => {
        if (e.key === ' ') { e.preventDefault(); this.toggle() } else if (e.key === 'ArrowRight') { e.preventDefault(); this.go(this.at + 1) } else if (e.key === 'ArrowLeft') { e.preventDefault(); this.go(this.at - 1) } else if (e.key === 'Escape') { e.preventDefault(); this.endTour() }
      }
      addEventListener('keydown', this.keys, true)
      this.go(0)
    }
    toggle() { this.playing = !this.playing; this.box.querySelector('.tour-play').textContent = this.playing ? 'Pause' : 'Play'; this.arm() }
    endTour() { clearTimeout(this.timer); if (this.keys) removeEventListener('keydown', this.keys, true); this.box?.remove(); this.box = null }
    go(i) {
      if (!this.box) return
      this.at = (i + this.list.length) % this.list.length
      const s = this.list[this.at], stage = this.box.querySelector('.tour-stage')
      clearTimeout(this.timer); this.ready = false
      // one step: the screen at a desktop's size on the left and at a phone's on the right, both at their real size
      const frame = cls => { const f = document.createElement('iframe'); f.className = cls; f.tabIndex = -1; f.style.pointerEvents = 'none'; f.title = `${s.name}${cls === 'is-phone' ? ', phone' : ''}`; return f }
      const wide = frame('is-wide'), phone = frame('is-phone')
      const loaded = f => new Promise(done => f.addEventListener('load', async () => {
        this.paint(f)
        await new Promise(r => setTimeout(r, 1300))   // (the frame's own state click, demoState, waits 500 ms)
        // the pictures in view decoded (a lazy one below the fold never comes: at most 3 s)
        try {
          const vh = f.contentWindow.innerHeight
          const shown = [...f.contentDocument.images].filter(im => { const r = im.getBoundingClientRect(); return r.width && r.top < vh && r.bottom > 0 })
          await Promise.race([Promise.all(shown.map(im => (im.complete ? (im.decode?.() ?? Promise.resolve()) : new Promise(r => { im.addEventListener('load', r, { once: true }); im.addEventListener('error', r, { once: true }) })).catch(() => {}))), new Promise(r => setTimeout(r, 3000))])
        } catch {}
        done()
      }, { once: true }))
      const both = Promise.all([loaded(wide), loaded(phone)])
      wide.src = s.src; phone.src = s.src
      // The next pair loads in a layer of its own over the one in view, unseen (opacity 0); the old pair stays until the
      // new one is in, then the new layer fades in (140 ms) and the old goes: never an empty or white stage between two.
      // A step not yet in when the next is asked for is dropped.
      const layer = document.createElement('div')
      layer.className = 'tour-layer is-next'; layer.append(wide, phone)
      for (const l of stage.querySelectorAll('.tour-layer.is-next')) l.remove()
      stage.append(layer)
      const at = this.at
      both.then(() => {
        if (!this.box || this.at !== at || !layer.isConnected) return
        const old = [...stage.querySelectorAll('.tour-layer:not(.is-next)')]
        layer.classList.remove('is-next')
        setTimeout(() => old.forEach(l => l.remove()), 160)
        this.ready = true; this.arm()
      })
      this.box.querySelector('.tour-where').innerHTML = `<b>${this.at + 1} / ${this.list.length}</b> · ${s.group} · ${s.name}`
      this.box.querySelector('.tour-wait').textContent = 'loading…'
    }
    arm() {
      clearTimeout(this.timer)
      if (!this.box) return
      this.box.querySelector('.tour-wait').textContent = this.ready ? (this.playing ? '' : 'paused') : 'loading…'
      if (this.playing && this.ready) this.timer = setTimeout(() => this.go(this.at + 1), this.hold)
    }
    paint(f) { try { const d = f.contentDocument?.documentElement; if (!d) return; if (this.dark) d.dataset.theme = 'dark'; else delete d.dataset.theme } catch {} }
    theme() {
      this.dark = !this.dark
      if (this.dark) document.documentElement.dataset.theme = 'dark'; else delete document.documentElement.dataset.theme
      for (const f of this.element.querySelectorAll('iframe[src]')) this.paint(f)
    }
  })
}

/** ?state=<name> on a demo page: the click a state needs, done once the page is in (only in the demo). */
export async function demoState(name, { now = false } = {}) {
  const $ = s => document.querySelector(s), wait = ms => new Promise(r => setTimeout(r, ms)), click = s => { const el = $(s); el?.click(); return Boolean(el) }
  if (!name) return
  await wait(now ? 60 : 500)
  const S = {
    select: async () => { for (const b of [...document.querySelectorAll('#desk-list .inbox-row .row-mark')].slice(1, 3)) { b.click(); await wait(150) } },
    duck: () => click('.desk-duck-open'),
    toast: () => click('#desk-list .inbox-row .inbox-answer.is-thumb.is-lead:not(.is-ack)'),
    bottom: () => { const m = $('#inbox'); if (m) m.scrollTop = m.scrollHeight },
    menu: () => click('#brand-menu'),
    switch: async () => { document.documentElement.dataset.rail = 'folded'; await new Promise(r => setTimeout(r, 300)); click('.rail-tag') },
    rail: () => { document.documentElement.dataset.rail = 'folded'; dispatchEvent(new Event('resize')) },
    'phone-note': () => click('.tab[data-tab="note"]'),
    keys: () => document.dispatchEvent(new Event('trommi:keys')),
    note: () => click('.corner-note-head'),
    inside: () => { const l = $('.tc-card > .tc-left'); if (l) l.scrollTop = 900 },
    strip: () => { const m = $('#cardpage'); if (m) m.scrollTop = 2600 },
    more: () => click('.tc-more-open'),
    'with-agent': () => { const id = window.trommi.vm().desk?.off_your_mind?.[0]; if (id) window.trommi.router.visit(`/card/${id}`) },
    'session-more': () => click('.t-head-more'),
    trail: () => { const d = $('.msg-work details.work'); if (d) { d.open = true; d.scrollIntoView({ block: 'center' }) } },
    pair: () => click('#settings-pair'),
    'pair-emoji': () => click('#settings-pair'),   // (the demo's new device answers after a moment: the six emoji come in place)
    'agent-more': () => click('#agent-0 .ag-more > summary'),
    invite: () => click('#settings-invite-agent'),
    'desk-invite': () => click('#desk-invite-go'),
    'invite-emoji': async () => { click('.t-head-more'); await wait(200); [...document.querySelectorAll('.desk-move button')].find(b => /invite link/.test(b.textContent))?.click() },
    'invite-ended': () => click('#settings-invite-agent'),
    share: async () => { click('#artifacts-list .shr-copy'); await wait(400); const d = $('#artifacts-list .shr.is-on details'); if (d) d.open = true },
    'board-help': () => click('#help-btn'),
  }
  await S[name]?.()
}
