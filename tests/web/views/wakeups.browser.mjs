// wakeups.browser.mjs: the agents' wakeups in the web app (demo room, ?mock=1). The page Wakeups lists what the core
// lists (WakeupsView.rows, in its order); Cancel and Other time go through the core (Action::WakeupCancel, WakeupMove)
// and the row then shows his word as pending; the chat of an agent with a wakeup shows its line (SessionView.wake_line),
// which opens the page. As a 1440 px window and as a phone.
//   node tests/web/views/wakeups.browser.mjs      (needs Chromium; serves the app from its sources itself)
import net from 'node:net'
import path from 'node:path'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { launchChromium } from '../../../app/web/dev/cdp.mjs'

const here = path.dirname(fileURLToPath(import.meta.url))
const repo = path.join(here, '..', '..', '..')
const sleep = ms => new Promise(r => setTimeout(r, ms))
const SIZES = [{ name: '1440 wide', width: 1440, height: 900, mobile: false }, { name: 'phone', width: 390, height: 844, mobile: true }]

/** What the page shows beside what the core says (run in the page). */
const PROBE = `(() => {
  const w = trommi.vm().wakeups, rows = [...document.querySelectorAll('#wk-list > .wk-item')]
  return { vm: w ? { count: w.count, rows: (w.rows ?? []).map(r => r.id) } : null, shown: rows.map(li => li.id.replace(/^wk-/, '')),
    pending: rows.filter(li => li.classList.contains('is-pending')).map(li => li.id.replace(/^wk-/, '')),
    none: Boolean(document.querySelector('#wk-none')), page: document.scrollingElement.scrollWidth - innerWidth }
})()`

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
  const until = async (x, ms = 4000) => { for (const end = Date.now() + ms; Date.now() < end; await sleep(100)) if (await ev(x).catch(() => false)) return true; return false }
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b)
  for (const size of SIZES) {
    const bad = []
    await page.send('Emulation.setDeviceMetricsOverride', { width: size.width, height: size.height, deviceScaleFactor: 1, mobile: size.mobile })
    await page.send('Page.navigate', { url: 'about:blank' })
    await page.send('Page.navigate', { url: `${APP}/wakeups?team=main&mock=1` })
    await until("document.readyState === 'complete' && document.documentElement.hasAttribute('data-ready') && Boolean(document.querySelector('#wk-list'))", 10000)
    let p = await ev(PROBE)
    if (!p.vm || !p.shown.length || !same(p.shown, p.vm.rows) || p.vm.count !== p.shown.length) bad.push(`the list is not the core's: ${JSON.stringify(p)}`)
    const [first, second] = p.shown
    // Cancel: through the core, then pending
    await ev(`document.querySelector('#wk-${first} .kn-del-ask').requestSubmit()`)
    if (!(await until(`Boolean(trommi.vm().wakeups?.rows.find(r => r.id === '${first}')?.pending) && document.querySelector('#wk-${first}')?.classList.contains('is-pending')`))) bad.push('Cancel is not shown as pending')
    // Other time: two hours from now
    if (second) {
      await ev(`(() => { const f = document.querySelector('#wk-${second} .wk-move form'), d = new Date(Date.now() + 2 * 3600e3), z = n => String(n).padStart(2, '0'); f.querySelector('input').value = d.getFullYear() + '-' + z(d.getMonth() + 1) + '-' + z(d.getDate()) + 'T' + z(d.getHours()) + ':' + z(d.getMinutes()); f.requestSubmit() })()`)
      if (!(await until(`/Moving to/.test(trommi.vm().wakeups?.rows.find(r => r.id === '${second}')?.pending?.en ?? '')`))) bad.push('Other time is not shown as pending')
    }
    // the chat's line opens the page
    const chat = await ev("trommi.vm().wakeups.rows[0].session_id")
    await ev(`window.Turbo.visit('/chat/${chat}')`)
    if (!(await until(`Boolean(document.querySelector('#wake-line-${chat}.wake-line'))`, 6000))) bad.push('no wake line in the chat')
    else {
      const line = await ev(`[document.querySelector('#wake-line-${chat} span').textContent, trommi.vm().sessions['${chat}']?.wake_line?.en]`)
      if (line[0] !== line[1]) bad.push(`the line is not the core's: ${JSON.stringify(line)}`)
      await ev(`document.querySelector('#wake-line-${chat}').click()`)
      if (!(await until("location.pathname === '/wakeups' && Boolean(document.querySelector('#wk-list'))"))) bad.push('the line does not open the page')
    }
    p = await ev(PROBE)
    if (p.page > 0) bad.push(`the page scrolls ${p.page}px sideways`)
    console.log(`${bad.length ? 'FAIL' : 'ok  '} ${size.name}: the list, Cancel, Other time, the chat's line${bad.length ? `; ${bad.join('; ')}` : ''}`)
    if (bad.length) failed.push(size.name)
  }
} finally {
  await browser.close()
  server.kill()
}
process.exit(failed.length ? 1 : 0)
