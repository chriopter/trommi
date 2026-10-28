// The pictures of the apps on the site and in the README (public/images/): real screenshots of the demo room (the
// made-up Fernly of demo/data) in the device frames of dev/showcase.html, taken in headless Chromium (dev/cdp.mjs) at
// 2x on a transparent page, then written as WebP and as a PNG of 256 colours with ImageMagick (`magick` on the PATH).
// The README's gallery of four phones is only a PNG, light only, and goes to .github/readme/ (the site does not show it).
//
//   node site/dev/showcase.mjs raw RAW [--app http://127.0.0.1:8900]
//       the screenshots of the web app into RAW, light and dark: the app's dev server must run
//       (`node app/web/dev/serve.mjs 8900`); the demo's own marks are left out
//   node site/dev/showcase.mjs RAW
//       the pictures from the screenshots in RAW
//
// RAW is a folder of screenshots, light and dark (<name>-light.png, <name>-dark.png):
//   web-group         the web app's group page (/team/main?mock=1, the group Fernly) at 1440x900, device scale 2
//   phone-group, phone-card, phone-chat   a phone at 402x874, device scale 3 (1206x2622, an iPhone 18 Pro): the group
//                     page, a card with two pictures, a chat with its helpers at work. `raw` takes them from the web
//                     app at that size, with 30 px kept free at the top for the island; the iPhone app in the iOS
//                     simulator gives the same names (app/ios/TrommiApp/AppStore/shots.sh, TROMMI_SCREEN and
//                     TROMMI_THEME, status bar at 9:41)
//   phone-scribble    the README's gallery only: the Scribble Board (TROMMI_SCREEN=scribble-board); without it the
//                     gallery is left as it is
// The iPhone and Mac pictures of the Apps section (app-iphone, app-mac) come from those apps and are not made here.
// Every file it writes must stay under 300 KB; a larger one is reported and the run exits 1.
import fs from 'node:fs'
import path from 'node:path'
import { execFileSync } from 'node:child_process'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { launchChromium } from './cdp.mjs'

const here = path.dirname(fileURLToPath(import.meta.url))
const args = process.argv.slice(2)
const sleep = ms => new Promise(r => setTimeout(r, ms))
const usage = () => { console.error('usage: node site/dev/showcase.mjs [raw] RAW (see the head of this file)'); process.exit(2) }

/** Waits until the expression is true in the page (10 s at most). */
async function until(p, expression) {
  for (let i = 0; i < 100; i++) {
    const r = await p.send('Runtime.evaluate', { expression, returnByValue: true }).catch(() => null)
    if (r?.result?.value) return true
    await sleep(100)
  }
  return false
}

if (args[0] === 'raw') {
  const raw = args[1] ?? usage()
  const i = args.indexOf('--app'), app = i < 0 ? 'http://127.0.0.1:8900' : args[i + 1]
  if (!await fetch(app).then(() => true, () => false)) { console.error(`No app at ${app}. Start it with "node app/web/dev/serve.mjs 8900", or name it with --app.`); process.exit(1) }
  // name, address, CSS size, device scale
  const shots = [['web-group', '/team/main', 1440, 900, 2], ['phone-group', '/team/main', 402, 874, 3],
    ['phone-card', '/card/a38afef6dbdd056db8042980f2b83336', 402, 874, 3], ['phone-chat', '/chat/9aa575fae2c64ece4ae4ab1b783673da', 402, 874, 3]]
  // the demo's marks: gone on a desktop; on a phone its band keeps its place (the layout is the demo's), unseen
  const quiet = '.side-demo, .pill-demo { display: none !important } @media (min-width: 861px) { .demo-band { display: none !important } } .demo-band { visibility: hidden !important }'
  fs.mkdirSync(raw, { recursive: true })
  let bad = 0
  for (const [name, at, width, height, scale] of shots) {
    for (const theme of ['light', 'dark']) {
      const browser = await launchChromium({ width, height })
      try {
        const p = await browser.page()
        // the page's policy allows no style of ours: the quiet marks need it lifted (this tab only)
        await p.send('Runtime.enable'); await p.send('Page.enable'); await p.send('Page.setBypassCSP', { enabled: true })
        await p.send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: scale, mobile: width < 600 })
        if (width < 600) await p.send('Emulation.setSafeAreaInsetsOverride', { insets: { top: 30, bottom: 30 } })
        await p.send('Page.addScriptToEvaluateOnNewDocument', { source: `try { localStorage.setItem('trommi-theme', '${theme}') } catch {}` })
        await p.send('Page.navigate', { url: `${app}${at}?mock=1` })
        if (!await until(p, "document.documentElement.hasAttribute('data-ready')")) { console.error(`${name}-${theme}: the app did not get ready`); bad++; continue }
        await sleep(1800)   // the core's first views, the pictures of the cards
        await p.send('Runtime.evaluate', { expression: `document.head.insertAdjacentHTML('beforeend', ${JSON.stringify(`<style>${quiet}</style>`)})` })
        await sleep(400)
        const got = await p.send('Page.captureScreenshot', { format: 'png' })
        const f = path.join(raw, `${name}-${theme}.png`)
        fs.writeFileSync(f, Buffer.from(got.data, 'base64'))
        console.log(path.relative(process.cwd(), f))
      } finally { await browser.close() }
    }
  }
  process.exit(bad ? 1 : 0)
}

const raw = args[0] ?? usage()
const out = path.join(here, '..', 'public', 'images')
const readme = path.join(here, '..', '..', '.github', 'readme')
const tmp = fs.mkdtempSync(path.join(process.env.TMPDIR ?? '/tmp', 'showcase-'))
const has = name => fs.existsSync(path.resolve(raw, `${name}.png`))
const shot = name => pathToFileURL(path.resolve(raw, `${name}.png`)).href
const page = pathToFileURL(path.join(here, 'showcase.html')).href

// name, layout, its query, CSS size
const jobs = []
for (const t of ['light', 'dark']) {
  jobs.push([`hero-${t}`, { layout: 'hero', laptop: shot(`web-group-${t}`), phone: shot(`phone-card-${t}`) }, 1100, 700])
}
// the loop of "Away from the terminal": one phone each, light (the site shows them on its own shade)
for (const s of ['group', 'card', 'chat']) jobs.push([`go-${s}`, { layout: 'shot', phone: shot(`phone-${s}-light`) }, 240, 504])
// the README shows its pictures light, on a light and on a dark GitHub alike
const gallery = ['group', 'card', 'chat', 'scribble'].map(s => `phone-${s}-light`)
if (gallery.every(has)) jobs.push(['gallery-light', { layout: 'gallery', phones: gallery.map(shot).join(',') }, 1100, 560])
else console.log(`.github/readme/gallery-light.png left as it is: no ${gallery.filter(s => !has(s)).join(', ')}.png in ${raw}`)

const MAX = 300 * 1024
let bad = 0
const report = f => {
  const size = fs.statSync(f).size
  console.log(`${path.relative(process.cwd(), f)} ${(size / 1024).toFixed(0)} KB${size > MAX ? '  over 300 KB' : ''}`)
  if (size > MAX) bad++
}
fs.mkdirSync(out, { recursive: true }); fs.mkdirSync(readme, { recursive: true })
for (const [name, query, width, height] of jobs) {
  const browser = await launchChromium({ width, height })
  try {
    const p = await browser.page()
    await p.send('Page.enable'); await p.send('Runtime.enable')
    await p.send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 2, mobile: false })
    await p.send('Emulation.setDefaultBackgroundColorOverride', { color: { r: 0, g: 0, b: 0, a: 0 } })
    await p.send('Page.navigate', { url: `${page}?${new URLSearchParams({ ...query, theme: name.endsWith('dark') ? 'dark' : 'light' })}` })
    if (!await until(p, 'document.body?.dataset.ready === "1"')) { console.error(`${name}: the page did not get ready (are the screenshots in ${raw}?)`); bad++; continue }
    const got = await p.send('Page.captureScreenshot', { format: 'png' })
    const png = path.join(tmp, `${name}.png`)
    fs.writeFileSync(png, Buffer.from(got.data, 'base64'))
    // the hero is a WebP and a PNG (the README's), the loop's phones WebP only, the gallery PNG only
    const isGallery = name.startsWith('gallery'), dir = isGallery ? readme : out, exts = isGallery ? ['png'] : name.startsWith('go-') ? ['webp'] : ['webp', 'png']
    if (exts.includes('webp')) execFileSync('magick', [png, '-strip', '-quality', '80', '-define', 'webp:method=6', '-define', 'webp:alpha-quality=90', path.join(dir, `${name}.webp`)])
    if (exts.includes('png')) execFileSync('magick', [png, '-strip', '-dither', 'None', '-colors', '256', '-define', 'png:compression-level=9', `PNG8:${path.join(dir, `${name}.png`)}`])
    for (const ext of exts) report(path.join(dir, `${name}.${ext}`))
  } finally { await browser.close() }
}
// the Apps section's web app: the group page itself, at its CSS size, no frame (the site draws the window)
if (has('web-group-light')) {
  const f = path.join(out, 'app-web.webp')
  execFileSync('magick', [path.resolve(raw, 'web-group-light.png'), '-resize', '1440x900', '-strip', '-quality', '80', '-define', 'webp:method=6', f])
  report(f)
}
fs.rmSync(tmp, { recursive: true, force: true })
process.exit(bad ? 1 : 0)
