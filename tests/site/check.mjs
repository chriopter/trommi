// The site checked as files, without a browser:
//   - every link, picture, font, script and stylesheet a page names is there, every #anchor exists on its page;
//   - nothing in public/ is named by nobody, nothing is larger than 1 MB but the demo video (public/video/, under
//     16 MB a file: Cloudflare serves static assets up to 25 MiB);
//   - the pages keep to their Content-Security-Policy: no inline script or style, nothing from another origin;
//   - no link opens a new tab; the one mail address is the contact;
//   - head and foot are the same on every page;
//   - public/_headers carries the policy, HTTPS only for a year with every name under it (no preload);
//   - the worker sends http to https and www to the apex, /beta to the TestFlight link (or the install steps), and
//     nothing else anywhere.
//
//   node tests/site/check.mjs
// Exit 3 when a check fails.
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { redirectOf, betaOf, BETA, BETA_FALLBACK } from '../../site/worker.js'

const site = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..', 'site')
const pub = path.join(site, 'public')
const ORIGIN = 'https://trommi.com'
const CONTACT = 'trommi@mail101.de'
const LIMIT = 1024 * 1024
const VIDEO_LIMIT = 16 * 1024 * 1024

let failed = 0
const check = (ok, what, got) => { if (!ok) failed++; console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}${ok || got === undefined ? '' : `: ${typeof got === 'string' ? got : JSON.stringify(got)}`}`) }

/** Every file under dir, as paths relative to it; what a tool leaves behind is not the site. */
function walk(dir, rel = '') {
  return fs.readdirSync(path.join(dir, rel), { withFileTypes: true }).flatMap(e => {
    if (['.git', '.shots', '.wrangler', 'node_modules'].includes(e.name)) return []
    const at = path.posix.join(rel, e.name)
    return e.isDirectory() ? walk(dir, at) : [at]
  })
}
const isFile = rel => fs.existsSync(path.join(pub, rel)) && fs.statSync(path.join(pub, rel)).isFile()
/** The file an address of the site is served from, as Cloudflare's assets do it: / is index.html, /x is x.html. */
function fileOf(pathname) {
  let rel = decodeURIComponent(pathname).replace(/^\/+/, '')
  if (rel === '' || rel.endsWith('/')) rel += 'index.html'
  if (!isFile(rel) && isFile(`${rel}.html`)) rel += '.html'
  return isFile(rel) ? rel : null
}
/** The tags of a page with their attributes: [{ tag, attrs: { name: value }, text }], comments left out. */
function tagsOf(html) {
  const tags = []
  for (const m of html.replace(/<!--[\s\S]*?-->/g, '').matchAll(/<([a-zA-Z][\w-]*)((?:\s+[\w:-]+(?:="[^"]*")?)*)\s*\/?>/g)) {
    const attrs = {}
    for (const a of m[2].matchAll(/([\w:-]+)(?:="([^"]*)")?/g)) attrs[a[1].toLowerCase()] = a[2] ?? ''
    tags.push({ tag: m[1].toLowerCase(), attrs, at: m.index + m[0].length })
  }
  return tags
}

const files = walk(pub)
const pages = files.filter(f => f.endsWith('.html')).sort()
check(JSON.stringify(pages) === JSON.stringify(['404.html', 'imprint.html', 'index.html', 'privacy.html']), 'the four pages are there', pages)

// ---- sizes ----
const isVideo = f => /^public\/video\/[\w-]+\.(mp4|webm)$/.test(f)
const large = walk(site).filter(f => fs.statSync(path.join(site, f)).size > (isVideo(f) ? VIDEO_LIMIT : LIMIT))
check(large.length === 0, 'no file is larger than 1 MB, no video larger than 16 MB', large)

// ---- every page: what it names, and how ----
const named = new Set(['_headers', ...pages])
const idsOf = Object.fromEntries(pages.map(p => [p, new Set(tagsOf(fs.readFileSync(path.join(pub, p), 'utf8')).map(t => t.attrs.id).filter(Boolean))]))
const chrome = {}
for (const page of pages) {
  const html = fs.readFileSync(path.join(pub, page), 'utf8')
  const tags = tagsOf(html)
  const bad = []
  const local = (ref, what) => {
    const url = new URL(ref, `${ORIGIN}/${page}`)
    if (url.origin !== ORIGIN) return bad.push(`${what} from another origin: ${ref}`)
    if (url.pathname === BETA && !url.search && !url.hash) return // the worker answers it (the TestFlight link)
    const file = fileOf(url.pathname)
    if (!file) return bad.push(`${what} leads to no file: ${ref}`)
    named.add(file)
    if (url.hash && !idsOf[file]?.has(url.hash.slice(1))) bad.push(`${what} names an anchor that is not on ${file}: ${ref}`)
  }
  for (const { tag, attrs, at } of tags) {
    if (tag === 'style' || 'style' in attrs) bad.push(`inline style on <${tag}>`)
    for (const name of Object.keys(attrs)) if (/^on/.test(name)) bad.push(`inline handler ${name} on <${tag}>`)
    if (tag === 'script') {
      if (!attrs.src || html.slice(at, html.indexOf('</script>', at)).trim()) bad.push('inline script')
      else local(attrs.src, 'script')
    }
    if (tag === 'base' || tag === 'iframe' || tag === 'form' || tag === 'object' || tag === 'embed') bad.push(`<${tag}> is not allowed by the policy`)
    if (tag === 'link') local(attrs.href, `link rel=${attrs.rel}`)
    if (tag === 'img') local(attrs.src, 'picture')
    if (tag === 'video') { if (attrs.src) local(attrs.src, 'video'); if (attrs.poster) local(attrs.poster, 'poster') }
    if (tag === 'source' && attrs.src) local(attrs.src, 'video')
    if (tag === 'video' && !(attrs.width && attrs.height && attrs.poster)) bad.push('video without width, height and poster')
    if (tag === 'video' && ('autoplay' in attrs || attrs.preload !== 'none')) bad.push('video that loads or plays before it is asked to')
    if (tag === 'img' || tag === 'source') for (const part of (attrs.srcset ?? '').split(',').map(s => s.trim().split(/\s+/)[0]).filter(Boolean)) local(part, 'picture')
    if (tag === 'img' && !('alt' in attrs)) bad.push(`picture without alt: ${attrs.src}`)
    if (tag === 'img' && !(attrs.width && attrs.height)) bad.push(`picture without width and height: ${attrs.src}`)
    if (attrs.popovertarget && !idsOf[page].has(attrs.popovertarget)) bad.push(`popovertarget names nothing: ${attrs.popovertarget}`)
    if (attrs['aria-labelledby'] && !idsOf[page].has(attrs['aria-labelledby'])) bad.push(`aria-labelledby names nothing: ${attrs['aria-labelledby']}`)
    if (tag === 'a') {
      const href = attrs.href ?? ''
      if (href.startsWith('mailto:')) { if (href !== `mailto:${CONTACT}`) bad.push(`a mail address that is not the contact: ${href}`) }
      else if (/^https:\/\//.test(href) && new URL(href).origin !== ORIGIN) {
        if (attrs.target) bad.push(`a link that opens a new tab: ${href}`)
      }
      else if (/^[a-z][a-z0-9+.-]*:/i.test(href)) bad.push(`a link that is not https: ${href}`)
      else local(href, 'link')
    }
  }
  for (const mail of html.match(/[\w.+-]+@[\w-]+(?:\.[\w-]+)+/g) ?? []) if (mail !== CONTACT) bad.push(`a mail address that is not the contact: ${mail}`)
  check(bad.length === 0, `${page}: links, pictures, anchors, no inline script or style`, bad)
  // the way in and the code: in the head and the foot of every page
  const app = tags.filter(t => t.tag === 'a' && t.attrs.href === 'https://app.trommi.com/').length
  const code = tags.filter(t => t.tag === 'a' && t.attrs.href === 'https://github.com/chriopter/trommi').length
  check(app >= 2 && code >= 2, `${page}: the app and the code are linked from the head and the foot`, [app, code])
  // the reading above knows attributes in double quotes only: a tag written otherwise would go unread
  const written = (html.replace(/<!--[\s\S]*?-->/g, '').match(/<[a-zA-Z]/g) ?? []).length
  check(written === tags.length, `${page}: every tag was read`, [written, tags.length])
  check(/<html lang="en">/.test(html) && /<title>[^<]+<\/title>/.test(html) && /<meta name="viewport"/.test(html), `${page}: language, title, viewport`)
  // head and foot: what stands around <main>, without the marks of the page one is on
  chrome[page] = [html.slice(html.indexOf('<body>'), html.indexOf('<main')), html.slice(html.indexOf('</main>'))]
    .map(s => s.replaceAll(' aria-current="page"', '').replaceAll('href="/#', 'href="#'))
}
check(pages.every(p => chrome[p][0] === chrome['index.html'][0] && chrome[p][1] === chrome['index.html'][1]), 'head and foot are the same on every page', pages.filter(p => chrome[p].join() !== chrome['index.html'].join()))

// ---- the stylesheet names its fonts; nothing in public/ is left over ----
const css = fs.readFileSync(path.join(pub, 'site.css'), 'utf8')
const cssBad = []
for (const m of css.matchAll(/url\(\s*['"]?([^'")]+)['"]?\s*\)/g)) {
  const url = new URL(m[1], `${ORIGIN}/site.css`)
  const file = url.origin === ORIGIN ? fileOf(url.pathname) : null
  if (file) named.add(file); else cssBad.push(m[1])
}
if (/@import/.test(css)) cssBad.push('@import')
check(cssBad.length === 0, 'site.css: every url() is a file of the site', cssBad)
const dead = files.filter(f => !named.has(f))
check(dead.length === 0, 'every file in public/ is named by a page or the stylesheet', dead)

// ---- the headers ----
const rules = {}
let at = null
for (const line of fs.readFileSync(path.join(pub, '_headers'), 'utf8').split('\n')) {
  if (!line.trim() || line.startsWith('#')) continue
  if (!/^\s/.test(line)) { at = line.trim(); rules[at] = {} }
  else { const [name, ...value] = line.trim().split(':'); rules[at][name.trim().toLowerCase()] = value.join(':').trim() }
}
const all = rules['/*'] ?? {}
const csp = Object.fromEntries((all['content-security-policy'] ?? '').split(';').map(d => d.trim()).filter(Boolean).map(d => { const [name, ...v] = d.split(/\s+/); return [name, v.join(' ')] }))
const want = { 'default-src': "'none'", 'script-src': "'self'", 'style-src': "'self'", 'img-src': "'self'", 'font-src': "'self'", 'media-src': "'self'", 'base-uri': "'none'", 'form-action': "'none'", 'frame-ancestors': "'none'" }
check(JSON.stringify(csp) === JSON.stringify(want), '_headers: the Content-Security-Policy allows the site itself and nothing else', csp)
check(all['x-content-type-options'] === 'nosniff' && all['referrer-policy'] === 'no-referrer', '_headers: nosniff, no referrer', all)
check(all['x-frame-options'] === 'DENY' && all['permissions-policy'] === 'camera=(), geolocation=(), microphone=()', '_headers: no framing, no camera, place or microphone', all)
check(/^same-origin$/.test(all['cross-origin-opener-policy'] ?? '') && /^same-origin$/.test(all['cross-origin-resource-policy'] ?? ''), '_headers: the pages and their files are for this origin only', all)
check(all['strict-transport-security'] === 'max-age=31536000; includeSubDomains', '_headers: HTTPS only, a year, with every name under trommi.com, no preload', all['strict-transport-security'])
check(Object.entries(rules).every(([k, r]) => k === '/*' || !('strict-transport-security' in r)), '_headers: Strict-Transport-Security once, for every address')
check(Object.keys(rules).filter(k => k !== '/*').every(k => fileOf(k.replace(/\*$/, '')) === null && fs.existsSync(path.join(pub, k.replace(/\/\*$/, '')))), '_headers: every other rule is for a folder that is there', Object.keys(rules))

// ---- the worker ----
const redirects = [
  ['http://trommi.com/', 'https://trommi.com/'],
  ['http://trommi.com/privacy?x=1', 'https://trommi.com/privacy?x=1'],
  ['https://www.trommi.com/privacy#keep', 'https://trommi.com/privacy#keep'],
  ['http://www.trommi.com/', 'https://trommi.com/'],
  ['https://trommi.com/', null],
  ['https://trommi.com/privacy', null],
  ['http://127.0.0.1:8910/', null],
  ['http://localhost:8910/privacy', null],
]
const wrong = redirects.filter(([from, to]) => redirectOf(from) !== to).map(([from]) => `${from} -> ${redirectOf(from)}`)
check(wrong.length === 0, 'worker: http goes to https, www to the apex, everything else is served', wrong)
const betas = [
  ['https://testflight.apple.com/join/AbCd1234', 'https://testflight.apple.com/join/AbCd1234'],
  ['', BETA_FALLBACK],
  [undefined, BETA_FALLBACK],
  ['https://testflight.apple.com/join/', BETA_FALLBACK],
  ['https://example.com/join/AbCd1234', BETA_FALLBACK],
  ['http://testflight.apple.com/join/AbCd1234', BETA_FALLBACK],
]
const wrongBeta = betas.filter(([link, to]) => betaOf(link) !== to).map(([link]) => `${link} -> ${betaOf(link)}`)
check(wrongBeta.length === 0 && fileOf(BETA) === null, 'worker: /beta goes to a TestFlight link or the install steps, and is no file', wrongBeta)
const wrangler = fs.readFileSync(path.join(site, 'wrangler.jsonc'), 'utf8')
const link = wrangler.match(/"TESTFLIGHT_URL":\s*"([^"]*)"/)
check(link && (link[1] === '' || betaOf(link[1]) === link[1]), 'wrangler.jsonc: TESTFLIGHT_URL is empty or a TestFlight public link', link?.[1])

console.log(failed ? `${failed} check(s) failed` : 'all checks passed')
process.exit(failed ? 3 : 0)
