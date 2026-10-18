// security.browser.mjs: Settings → Security in headless Chromium, in the demo room (?mock=1): the row in Settings,
// the account key (the room it opens, its key epoch), the Emergency Kit, every device of the account with its fingerprint
// and the date it came, the agents, one line per kind on what it can read, and the self test under it (whose page
// goes back to Security). At 1440 and 390 wide, light; the page never scrolls sideways.
//   node tests/web/views/security.browser.mjs      (needs Chromium; serves the app from its sources itself)
//   TROMMI_E2E_SHOTS=<folder>: pictures of the page at both widths.
import net from 'node:net'
import path from 'node:path'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { launchChromium } from '../../../app/web/dev/cdp.mjs'
import fs from 'node:fs'

const here = path.dirname(fileURLToPath(import.meta.url))
const repo = path.join(here, '..', '..', '..')
const sleep = ms => new Promise(r => setTimeout(r, ms))
const SHOTS = process.env.TROMMI_E2E_SHOTS ?? ''

async function freePort() {
  return new Promise((resolve, reject) => { const s = net.createServer(); s.unref(); s.on('error', reject); s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => resolve(port)) }) })
}
const port = await freePort()
const server = spawn(process.execPath, [path.join(repo, 'app', 'web', 'dev', 'serve.mjs'), String(port)], { stdio: ['ignore', 'pipe', 'inherit'] })
await new Promise((resolve, reject) => { server.stdout.on('data', d => { if (String(d).includes('http://')) resolve() }); server.on('exit', c => reject(new Error(`the app server ended (${c})`))) })
const APP = `http://127.0.0.1:${port}`
const browser = await launchChromium({ width: 1440, height: 1000 })
let failed = 0
const check = (ok, what, got) => { console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}${ok ? '' : `: ${JSON.stringify(got)}`}`); if (!ok) failed++ }

const READ = `(() => {
  const main = document.querySelector('main.set-page'), rows = sel => [...main.querySelectorAll(sel)]
  return {
    title: main.querySelector('.set-head h2')?.textContent, back: main.querySelector('.set-back span')?.textContent,
    labels: rows('.set-label').map(h => h.textContent.trim()),
    keyFp: document.querySelector('#security-key-fp')?.textContent, key: document.querySelector('#security-key small')?.textContent,
    kit: document.querySelector('#security-kit .set-detail')?.textContent,
    devices: rows('.security-device').map(a => ({ name: a.querySelector('b').textContent.trim(), small: a.querySelector('small').textContent, href: a.getAttribute('href'), reads: a.querySelector('.set-detail').textContent })),
    agents: document.querySelector('#security-agents small')?.textContent, agentsHref: document.querySelector('#security-agents')?.getAttribute('href'),
    notes: rows('.set-note').map(p => p.textContent.trim()),
    proof: document.querySelector('#settings-proof')?.getAttribute('href'),
    sideways: Math.max(document.scrollingElement.scrollWidth, document.body.scrollWidth) - innerWidth,
  }
})()`

try {
  const page = await browser.page()
  await page.send('Runtime.enable'); await page.send('Page.enable')
  const ev = async x => { const r = await page.send('Runtime.evaluate', { expression: x, awaitPromise: true, returnByValue: true }); if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text); return r.result.value }
  const until = async (x, what) => { for (let i = 0; i < 100; i++) { if (await ev(x).catch(() => false)) return; await sleep(100) } throw new Error(`waited in vain: ${what}`) }
  const open = async p => {
    await page.send('Page.navigate', { url: 'about:blank' })
    await page.send('Page.navigate', { url: `${APP}${p}${p.includes('?') ? '&' : '?'}mock=1` })
    await until("document.readyState === 'complete' && document.documentElement.hasAttribute('data-ready')", `${p} is ready`)
    await ev("localStorage.setItem('trommi-theme', 'light'); document.documentElement.dataset.theme = 'light'")
    await sleep(600)
  }
  const shot = async name => {
    if (!SHOTS) return
    const { data } = await page.send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: true })
    fs.mkdirSync(SHOTS, { recursive: true }); fs.writeFileSync(path.join(SHOTS, `${name}.png`), Buffer.from(data, 'base64'))
  }

  // ---- the row in Settings ----
  await open('/settings')
  await until("document.querySelector('#settings-security')", 'Settings shows the Security row')
  const row = await ev("(() => { const rows = [...document.querySelectorAll('main .set-row')]; const a = document.querySelector('#settings-security'); return { last: rows.at(-1) === a, href: a.getAttribute('href'), word: a.querySelector('b').textContent, old: !!document.querySelector('#settings-proof') } })()")
  check(row.last && row.href === '/settings/security' && row.word === 'Security' && !row.old, 'Security is the last row of Settings, in place of the self test', row)

  for (const [width, height] of [[1440, 1000], [390, 844]]) {
    await page.send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: width < 600 })
    await open('/settings/security')
    await until("document.querySelector('#security-key')", 'the Security page is drawn')
    const p = await ev(READ)
    check(p.title === 'Security' && p.back === 'Settings', `${width}: a page of Settings`, p)
    check(p.labels.join() === 'Account key,Authorized devices,Agents,Check,Dictation', `${width}: its parts, in order (Dictation's sealed connection last)`, p.labels)
    check(/ · key epoch \d+/.test(p.key ?? ''), `${width}: the account key: the room it opens, its key epoch`, p.key)
    check(p.devices.length === 3 && p.devices.every(d => /^[0-9a-f]{4}( [0-9a-f]{4}){3} · added \d+ \w+ \d{4}$/.test(d.small) && d.href.startsWith('/settings/devices/') && d.reads === 'everything'), `${width}: every device with its fingerprint, the day it came, a way to its page`, p.devices)
    check(p.devices[0].name === 'Laptop · this device', `${width}: this device says so`, p.devices[0])
    check(/agents? · \d+ sessions?: each with its own key$/.test(p.agents ?? '') && p.agentsHref === '/settings/agents', `${width}: the agents, counted, with their page`, p.agents)
    check(p.notes.some(n => n.startsWith('Opens everything; it is also your recovery key. The hub keeps it only sealed')) && p.notes.some(n => n.startsWith('Each of your devices reads everything')) && p.notes.some(n => n.startsWith('An agent reads only its own session')) && p.notes.some(n => n.startsWith('Nobody else, not the hub either')), `${width}: one line per kind on what it can read`, p.notes)
    check(p.proof === '/settings/proof', `${width}: the self test is a row of the page`, p.proof)
    check(p.sideways <= 0, `${width}: the page does not scroll sideways`, p.sideways)
    await shot(`security-${width}-light`)
  }
  await page.send('Emulation.setDeviceMetricsOverride', { width: 1440, height: 1000, deviceScaleFactor: 1, mobile: false })

  // ---- the self test goes back to Security ----
  await open('/settings/proof')
  const proof = await ev("({ title: document.querySelector('main .set-head h2')?.textContent, back: document.querySelector('main .set-back')?.getAttribute('href'), word: document.querySelector('main .set-back span')?.textContent })")
  check(proof.title === 'Self Test' && proof.back === '/settings/security' && proof.word === 'Security', 'the self test is a page of Security', proof)
} catch (err) { console.log(`FAIL ${err.message}`); failed++ } finally {
  await browser.close(); server.kill()
}
console.log(failed ? `${failed} failed` : 'all passed')
process.exit(failed ? 1 : 0)
