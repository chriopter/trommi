// dock.browser.mjs: the two docked stacks of the Desk (demo room, ?mock=1, five cards starred): two small tabs,
// Starred and Running, each its sign, word and count and no card at rest; a tap fans the cards upward inside the
// window, one stack at a time, the running one ending in the way to the complete list; Escape lays it together.
// As a 1440 px window (the tabs stand on the lower edge beside the sidebar, clear of the note in its corner) and as a
// phone (above the tab pill, which keeps Chat, Desk and Note).
//   node tests/web/views/dock.browser.mjs      (needs Chromium; serves the app from its sources itself)
import net from 'node:net'
import path from 'node:path'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { launchChromium } from '../../../app/web/dev/cdp.mjs'

const here = path.dirname(fileURLToPath(import.meta.url))
const repo = path.join(here, '..', '..', '..')
const sleep = ms => new Promise(r => setTimeout(r, ms))
const SIZES = [{ name: '1440 wide', width: 1440, height: 900, mobile: false }, { name: 'phone', width: 390, height: 844, mobile: true }]

/** Stars five cards of the room, so both stacks stand (run in the page). */
const STAR = `(async () => {
  const cards = [...trommi.model().byCard.values()].filter(c => c.status === 'open').slice(0, 5)
  for (const c of cards) await trommi.client.star(c.id, true)
  await new Promise(r => setTimeout(r, 500))
  return cards.length
})()`
/** What stands where (run in the page). */
const PROBE = `(() => {
  const rect = el => { const r = el.getBoundingClientRect(); return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width, height: r.height } }
  const seen = el => { const r = el.getBoundingClientRect(), c = getComputedStyle(el); return c.display !== 'none' && c.visibility !== 'hidden' && r.width > 0 && r.height > 0 }
  const dock = document.querySelector('#desk-dock'), c = getComputedStyle(dock)
  const stacks = [...dock.querySelectorAll('.dock-stack')].map(d => {
    const tab = d.querySelector('.dock-tab'), fan = d.querySelector('.dock-fan'), cards = [...fan.querySelectorAll('.dock-card')]
    return { k: d.dataset.k, open: d.open, tab: rect(tab), word: tab.querySelector('b').textContent, count: tab.querySelector('em').textContent, sign: Boolean(tab.querySelector('svg')) && seen(tab.querySelector('svg')),
      cards: cards.length, shown: cards.filter(seen).map(rect), titles: cards.filter(seen).map(a => a.querySelector('b').textContent), all: fan.querySelector('.dock-all')?.getAttribute('href') ?? null, allSeen: Boolean(fan.querySelector('.dock-all')) && seen(fan.querySelector('.dock-all')) }
  })
  const one = s => { const el = document.querySelector(s); return el && seen(el) ? rect(el) : null }
  return { vw: innerWidth, vh: innerHeight, side: parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--side')) || 0, stacks,
    bare: c.borderTopWidth === '0px' && c.boxShadow === 'none' && c.backgroundColor === 'rgba(0, 0, 0, 0)' && c.paddingTop === '0px',
    topbar: one('.topbar'), pill: one('#tabbar'), pillWords: [...document.querySelectorAll('#tabbar a, #tabbar button')].map(a => a.textContent.replace(/\\d+/g, '').trim()), note: one('#corner-note-box'), page: document.scrollingElement.scrollWidth - innerWidth }
})()`
const meet = (a, b) => a && b && a.left < b.right - 1 && b.left < a.right - 1 && a.top < b.bottom - 1 && b.top < a.bottom - 1

async function freePort() {
  return new Promise((resolve, reject) => { const s = net.createServer(); s.unref(); s.on('error', reject); s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => resolve(port)) }) })
}

const port = await freePort()
const server = spawn(process.execPath, [path.join(repo, 'app', 'web', 'dev', 'serve.mjs'), String(port)], { stdio: ['ignore', 'pipe', 'inherit'] })
await new Promise((resolve, reject) => { server.stdout.on('data', d => { if (String(d).includes('http://')) resolve() }); server.on('exit', c => reject(new Error(`the app server ended (${c})`))) })
const APP = `http://127.0.0.1:${port}`
const browser = await launchChromium({ width: 1440, height: 900 })
const failed = []
try {
  const page = await browser.page()
  await page.send('Runtime.enable'); await page.send('Page.enable')
  const ev = async x => { const r = await page.send('Runtime.evaluate', { expression: x, awaitPromise: true, returnByValue: true }); if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text); return r.result.value }
  const press = async sel => { await ev(`document.querySelector(${JSON.stringify(sel)}).click()`); await sleep(250) }
  for (const size of SIZES) {
    await page.send('Emulation.setDeviceMetricsOverride', { width: size.width, height: size.height, deviceScaleFactor: 1, mobile: size.mobile })
    await page.send('Page.navigate', { url: 'about:blank' })
    await page.send('Page.navigate', { url: `${APP}/?mock=1` })
    for (let i = 0; i < 100; i++) { if (await ev("document.readyState === 'complete' && document.documentElement.hasAttribute('data-ready') && Boolean(document.querySelector('#desk-dock'))").catch(() => false)) break; await sleep(100) }
    await sleep(700)
    const bad = []
    const starred = await ev(STAR)
    if (starred !== 5) bad.push(`${starred} cards to star, not 5`)

    // closed
    const shut = await ev(PROBE), [star, run] = shut.stacks
    if (shut.stacks.map(s => s.k).join() !== 'star,run') bad.push(`stacks ${shut.stacks.map(s => s.k)}`)
    else {
      if (star.word !== 'Starred' || run.word !== 'Running') bad.push(`the tabs say "${star.word}" and "${run.word}"`)
      for (const s of shut.stacks) {
        if (!s.sign) bad.push(`${s.k}: no sign on the tab`)
        if (Number(s.count) !== s.cards || !s.cards) bad.push(`${s.k}: the tab counts ${s.count}, the fan holds ${s.cards}`)
        if (s.open || s.shown.length) bad.push(`${s.k}: ${s.shown.length} card(s) in view at rest`)
        if (s.tab.left < shut.side || s.tab.right > shut.vw || s.tab.bottom > shut.vh + 1) bad.push(`${s.k}: the tab lies at ${Math.round(s.tab.left)}..${Math.round(s.tab.right)}, bottom ${Math.round(s.tab.bottom)}`)
        if (s.tab.height < 36 || s.tab.height > 48 || s.tab.width > 170) bad.push(`${s.k}: the tab is ${Math.round(s.tab.width)} by ${Math.round(s.tab.height)}`)
        if (meet(s.tab, shut.note)) bad.push(`${s.k}: the tab lies on the note`)
        if (meet(s.tab, shut.pill)) bad.push(`${s.k}: the tab lies on the tab pill`)
      }
      if (star.count !== '5') bad.push(`Starred counts ${star.count}`)
      if (Math.abs(star.tab.top - run.tab.top) > 1 || star.tab.right > run.tab.left) bad.push('the two tabs do not stand side by side')
      if (!shut.bare) bad.push('the dock has a container of its own')
      if (shut.page > 0) bad.push(`the page scrolls ${shut.page}px sideways`)
      if (size.mobile) {
        if (!shut.pill) bad.push('no tab pill')
        else if (star.tab.bottom > shut.pill.top) bad.push('the tabs are not above the tab pill')
        if (shut.pillWords.join() !== 'Chat,Desk,Note') bad.push(`the tab pill holds ${shut.pillWords}`)
      } else {
        if (!shut.topbar || shut.side < 200) bad.push('no sidebar')
        if (!shut.note) bad.push('no note in its corner')
        if (Math.abs(star.tab.bottom - shut.vh) > 1) bad.push(`the tabs end at ${Math.round(star.tab.bottom)}, not on the lower edge`)
        if (shut.pill) bad.push('a tab pill on the desktop')
      }

      // fanned: the starred stack, then the running one in its place, then Escape
      await press('.dock-stack[data-k="star"] > summary')
      const a = await ev(PROBE)
      if (!a.stacks[0].open || a.stacks[0].shown.length !== 5) bad.push(`Starred fanned shows ${a.stacks[0].shown.length} of 5 cards`)
      if (a.stacks[0].shown.some(r => r.left < a.side - 1 || r.right > a.vw + 1 || r.top < 0 || r.bottom > a.stacks[0].tab.top + 8)) bad.push('a starred card of the fan lies outside the window or below its tab')
      if (a.stacks[0].titles.some(t => !t.trim())) bad.push('a fan card without its title')
      if (a.stacks[0].all !== null) bad.push('the starred fan leads to the complete list')
      await press('.dock-stack[data-k="run"] > summary')
      const b = await ev(PROBE)
      if (b.stacks[0].open || !b.stacks[1].open) bad.push('two stacks open at once, or Running did not open')
      if (b.stacks[1].shown.length !== b.stacks[1].cards) bad.push(`Running fanned shows ${b.stacks[1].shown.length} of ${b.stacks[1].cards} cards`)
      if (b.stacks[1].shown.some(r => r.left < b.side - 1 || r.right > b.vw + 1 || r.top < 0)) bad.push('a running card of the fan lies outside the window')
      if (!/\/stacks\/off$/.test(b.stacks[1].all ?? '') || !b.stacks[1].allSeen) bad.push(`the running fan's way to the complete list: ${b.stacks[1].all}`)
      await page.send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27 }); await sleep(250)
      const c = await ev(PROBE)
      if (c.stacks.some(s => s.open)) bad.push('Escape leaves a stack open')
    }
    console.log(`${bad.length ? 'FAIL' : 'ok  '} ${size.name}: ${shut.stacks.map(s => `${s.word} ${s.count}`).join(', ')}${bad.length ? `; ${bad.join('; ')}` : ''}`)
    if (bad.length) failed.push(size.name)
  }
} finally {
  await browser.close()
  server.kill()
}
process.exit(failed.length ? 1 : 0)
