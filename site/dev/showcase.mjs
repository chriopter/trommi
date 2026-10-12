// The pictures of the apps on the site and in the README (public/images/): real screenshots of the demo room (the
// made-up Fernly of demo/data) in the device frames of dev/showcase.html, taken in headless Chromium (dev/cdp.mjs) at
// 2x on a transparent page, then written as WebP and as a PNG of 256 colours with ImageMagick (`magick` on the PATH).
// The README's gallery of four phones is only a PNG, light only, and goes to .github/readme/ (the site does not show it).
//
//   node site/dev/showcase.mjs RAW
//
// RAW is a folder of screenshots, light and dark (<name>-light.png, <name>-dark.png):
//   web-desk          the web app's Desk at 1440x900, device scale 2 (app/web: `node dev/serve.mjs`, /?mock=1)
//   ios-desk, ios-card, ios-chat, ios-scribble   the iPhone app in the iOS simulator, an iPhone 18 Pro (1206x2622),
//                     opened with TROMMI_SCREEN=desk, card--two-pictures, chat, scribble-board and TROMMI_THEME
//                     (the launch hooks of app/ios/TrommiApp/AppStore/shots.sh), status bar at 9:41.
// Every file it writes must stay under 300 KB; a larger one is reported and the run exits 1.
import fs from 'node:fs'
import path from 'node:path'
import { execFileSync } from 'node:child_process'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { launchChromium } from './cdp.mjs'

const here = path.dirname(fileURLToPath(import.meta.url))
const raw = process.argv[2]
if (!raw) { console.error('usage: node site/dev/showcase.mjs RAW (see the head of this file)'); process.exit(2) }
const out = path.join(here, '..', 'public', 'images')
const readme = path.join(here, '..', '..', '.github', 'readme')
const tmp = fs.mkdtempSync(path.join(process.env.TMPDIR ?? '/tmp', 'showcase-'))
const sleep = ms => new Promise(r => setTimeout(r, ms))
const shot = name => pathToFileURL(path.resolve(raw, `${name}.png`)).href
const page = pathToFileURL(path.join(here, 'showcase.html')).href

// name, layout, its query, CSS size
const jobs = []
for (const t of ['light', 'dark']) {
  jobs.push([`hero-${t}`, { layout: 'hero', laptop: shot(`web-desk-${t}`), phone: shot(`ios-card-${t}`) }, 1100, 700])
}
// the README shows its pictures light, on a light and on a dark GitHub alike
jobs.push(['gallery-light', { layout: 'gallery', phones: ['desk', 'card', 'chat', 'scribble'].map(s => shot(`ios-${s}-light`)).join(',') }, 1100, 560])

const MAX = 300 * 1024
let bad = 0
fs.mkdirSync(out, { recursive: true }); fs.mkdirSync(readme, { recursive: true })
for (const [name, query, width, height] of jobs) {
  const browser = await launchChromium({ width, height })
  try {
    const p = await browser.page()
    await p.send('Page.enable'); await p.send('Runtime.enable')
    await p.send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 2, mobile: false })
    await p.send('Emulation.setDefaultBackgroundColorOverride', { color: { r: 0, g: 0, b: 0, a: 0 } })
    await p.send('Page.navigate', { url: `${page}?${new URLSearchParams({ ...query, theme: name.endsWith('dark') ? 'dark' : 'light' })}` })
    let ready = false
    for (let i = 0; i < 100 && !ready; i++) {
      const r = await p.send('Runtime.evaluate', { expression: 'document.body?.dataset.ready === "1"', returnByValue: true }).catch(() => null)
      ready = !!r?.result?.value
      if (!ready) await sleep(100)
    }
    if (!ready) { console.error(`${name}: the page did not get ready (are the screenshots in ${raw}?)`); bad++; continue }
    const got = await p.send('Page.captureScreenshot', { format: 'png' })
    const png = path.join(tmp, `${name}.png`)
    fs.writeFileSync(png, Buffer.from(got.data, 'base64'))
    const gallery = name.startsWith('gallery'), dir = gallery ? readme : out
    if (!gallery) execFileSync('magick', [png, '-strip', '-quality', '80', '-define', 'webp:method=6', '-define', 'webp:alpha-quality=90', path.join(dir, `${name}.webp`)])
    execFileSync('magick', [png, '-strip', '-dither', 'None', '-colors', '256', '-define', 'png:compression-level=9', `PNG8:${path.join(dir, `${name}.png`)}`])
    for (const ext of gallery ? ['png'] : ['webp', 'png']) {
      const f = path.join(dir, `${name}.${ext}`), size = fs.statSync(f).size
      console.log(`${path.relative(process.cwd(), f)} ${(size / 1024).toFixed(0)} KB${size > MAX ? '  over 300 KB' : ''}`)
      if (size > MAX) bad++
    }
  } finally { await browser.close() }
}
fs.rmSync(tmp, { recursive: true, force: true })
process.exit(bad ? 1 : 0)
