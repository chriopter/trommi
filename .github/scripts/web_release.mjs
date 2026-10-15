// Puts together what the web deploy delivers, from what the build wrote:
//   node .github/scripts/web_release.mjs PUBLIC_DIR OUT_DIR
// Environment: VERSION (the build run's number), GITHUB_SHA (the commit), GITHUB_REPOSITORY,
// WEB_HUB_URL (the hub the app speaks to).
// OUT_DIR then holds
//   public/          the files the worker serves: PUBLIC_DIR as it is (its _headers must carry Strict-Transport-Security,
//                    COOP and CORP for every address, or nothing is put together)
//   worker.js        app/web/worker.js
//   whats-new.mjs    app/web/whats-new.mjs (worker.js imports it)
//   entry.js         what the worker runtime loads: worker.js's default export alone. worker.js also exports
//                    constants for the dev server and the tests, and the runtime refuses a module whose named
//                    exports are not handlers ("Incorrect type for map entry 'FAVICON'")
//   wrangler.jsonc   app/web/wrangler.jsonc without its build step (the deploy delivers these files and builds
//                    nothing), with entry.js as the script and app.trommi.com as the worker's address
//   public/gen/release.json   the release's number, commit and time, for the app's menu (not in the build's manifest)
//   manifest.json    what this release is (product, repository, version, commit, hub) and every file
//                    above with its size and SHA-256
//   checksums.txt    the same hashes and manifest.json's, in the form of sha256sum: `sha256sum -c checksums.txt`
// ship-web.yml checks the files against checksums.txt, attests them and runs `wrangler deploy` in OUT_DIR.
import fs from 'node:fs'
import path from 'node:path'
import crypto from 'node:crypto'
import { fileURLToPath } from 'node:url'

const REPO = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const [from, out] = process.argv.slice(2)
if (!from || !out) { console.error('usage: node .github/scripts/web_release.mjs PUBLIC_DIR OUT_DIR'); process.exit(2) }
const env = (name, fallback) => process.env[name] || fallback
const version = Number(env('VERSION', '0'))
const commit = env('GITHUB_SHA', 'dev')
const hub = env('WEB_HUB_URL', 'https://hub.trommi.com')
const HOST = 'app.trommi.com'
// what the block for every address ("/*") of _headers must say (app/web/dev/check.mjs checks the same)
const MUST = ['Strict-Transport-Security: max-age=31536000; includeSubDomains', 'Cross-Origin-Opener-Policy: same-origin', 'Cross-Origin-Resource-Policy: same-origin']
const fail = why => { console.error(`web_release: ${why}`); process.exit(1) }
if (!Number.isSafeInteger(version) || version < 0) fail('VERSION is not a whole number')
if (!/^https:\/\/[a-z0-9.-]+(:\d+)?$/.test(hub)) fail(`WEB_HUB_URL is not a plain https origin: ${hub}`)

/** JSON with comments (outside strings) and trailing commas, as wrangler reads it. */
function jsonc(text) {
  let plain = '', inString = false
  for (let i = 0; i < text.length; i++) {
    const c = text[i]
    if (inString) { plain += c; if (c === '\\') plain += text[++i]; else if (c === '"') inString = false }
    else if (c === '"') { inString = true; plain += c }
    else if (c === '/' && text[i + 1] === '/') { while (i < text.length && text[i] !== '\n') i++; plain += '\n' }
    else if (c === '/' && text[i + 1] === '*') { const end = text.indexOf('*/', i + 2); if (end < 0) throw new Error('comment not closed'); i = end + 1 }
    else plain += c
  }
  return JSON.parse(plain.replace(/,(\s*[}\]])/g, '$1'))
}

const config = jsonc(fs.readFileSync(path.join(REPO, 'app/web/wrangler.jsonc'), 'utf8'))
delete config.build
delete config.$schema
if (config.main !== 'worker.js' || config.assets?.directory !== './public') fail('app/web/wrangler.jsonc no longer names worker.js and ./public; this script must follow it')
config.main = 'entry.js'
// The worker's address. Named here so that a delivery never depends on what was clicked in the dashboard.
config.routes ??= [{ pattern: HOST, custom_domain: true }]
for (const need of ['index.html', '_headers', 'gen/build.txt']) if (!fs.existsSync(path.join(from, need))) fail(`${from} has no ${need}: not a built app`)

// The hub the build speaks to must be the one that was asked for: the page's policy lets it reach no other.
const headers = fs.readFileSync(path.join(from, '_headers'), 'utf8')
const connect = /connect-src ([^;\n]*)/.exec(headers)?.[1].split(/\s+/) ?? []
const hubs = connect.filter(source => source.startsWith('http'))
if (hubs.length !== 1 || hubs[0] !== hub) fail(`_headers lets the app reach ${hubs.join(', ') || 'no hub'}, the build was asked for ${hub}`)
const app = fs.readFileSync(path.join(REPO, 'app/web/public/app.mjs'), 'utf8')
if (!app.includes(`'${hub}'`)) fail(`app/web/public/app.mjs does not name ${hub} as its hub`)
const lines = headers.split('\n')
const at = lines.indexOf('/*'), end = lines.findIndex((line, i) => i > at && /^\S/.test(line))
const all = at < 0 ? [] : lines.slice(at + 1, end < 0 ? undefined : end)
for (const need of MUST) if (!all.some(line => line.trim() === need)) fail(`_headers: the block /* does not say ${need}`)

fs.rmSync(out, { recursive: true, force: true })
fs.mkdirSync(out, { recursive: true })
fs.cpSync(from, path.join(out, 'public'), { recursive: true })
// What runs there, for the line under What's new in the app's menu: the release's number, its commit, when it was made.
// Not in the build's own manifest (gen/manifest.json): the same source built anywhere stays the same bytes.
fs.writeFileSync(path.join(out, 'public', 'gen', 'release.json'), `${JSON.stringify({ version, commit, released: new Date().toISOString() })}\n`)
fs.copyFileSync(path.join(REPO, 'app/web/worker.js'), path.join(out, 'worker.js'))
fs.copyFileSync(path.join(REPO, 'app/web/whats-new.mjs'), path.join(out, 'whats-new.mjs'))
fs.writeFileSync(path.join(out, 'entry.js'), "import worker from './worker.js'\nexport default worker\n")
fs.writeFileSync(path.join(out, 'wrangler.jsonc'), `${JSON.stringify(config, null, 2)}\n`)

const walk = dir => fs.readdirSync(dir, { withFileTypes: true }).flatMap(e => (e.isDirectory() ? walk(path.join(dir, e.name)) : [path.join(dir, e.name)]))
const sha = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex')
const names = walk(out).map(file => path.relative(out, file).split(path.sep).join('/')).sort()
for (const name of names) if (/[\s\\]/.test(name)) fail(`a file name with white space or a backslash: ${name}`)
const files = names.map(name => ({ name, sha256: sha(path.join(out, name)), size: fs.statSync(path.join(out, name)).size }))
const manifest = { product: 'trommi-web', repository: env('GITHUB_REPOSITORY', 'chriopter/trommi'), version, commit, worker: config.name, host: HOST, hub, files }
fs.writeFileSync(path.join(out, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`)
const sums = [...files, { name: 'manifest.json', sha256: sha(path.join(out, 'manifest.json')) }]
fs.writeFileSync(path.join(out, 'checksums.txt'), sums.map(f => `${f.sha256}  ${f.name}\n`).join(''))
console.log(`web_release: ${out} holds worker "${config.name}" for ${HOST}, hub ${hub}, ${files.length} files`)
