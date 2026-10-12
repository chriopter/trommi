// Local static server for the site, as deployed: public/ with the headers of public/_headers, /imprint for
// imprint.html and a redirect there from /imprint.html and /imprint/ (Cloudflare's html_handling), public/404.html
// for what is no file (assets.not_found_handling), and byte ranges of a file (what a video player asks for).
//   node site/dev/serve.mjs [port=8910]
import http from 'node:http'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'public')
const port = Number(process.argv[2] || 8910)
const TYPES = { '.html': 'text/html; charset=utf-8', '.css': 'text/css', '.js': 'text/javascript; charset=utf-8', '.svg': 'image/svg+xml', '.png': 'image/png', '.webp': 'image/webp', '.woff2': 'font/woff2', '.mp4': 'video/mp4', '.webm': 'video/webm', '.txt': 'text/plain; charset=utf-8' }

/** The rules of _headers: [[pattern as a RegExp, { name: value }]], read on every request. */
function headerRules() {
  const rules = []
  for (const line of fs.readFileSync(path.join(root, '_headers'), 'utf8').split('\n')) {
    if (!line.trim() || line.startsWith('#')) continue
    if (!/^\s/.test(line)) rules.push([new RegExp(`^${line.trim().replace(/[.+?^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*')}$`), {}])
    else { const [name, ...value] = line.trim().split(':'); rules.at(-1)[1][name.trim()] = value.join(':').trim() }
  }
  return rules
}

http.createServer((req, res) => {
  const url = new URL(req.url, 'http://x')
  let rel
  try { rel = decodeURIComponent(url.pathname).replace(/^\/+/, '') } catch { res.writeHead(400); return res.end() }
  if (rel.split('/').includes('..')) { res.writeHead(403); return res.end() }
  const isFile = f => fs.existsSync(path.join(root, f)) && fs.statSync(path.join(root, f)).isFile()
  // one address per page: /x.html and /x/ go to /x, /index.html to /
  const page = rel.replace(/\/$/, '').replace(/\.html$/, '').replace(/(^|\/)index$/, '$1')
  if (page !== rel && isFile(`${page || 'index'}.html`)) { res.writeHead(307, { Location: `/${page}${url.search}` }); return res.end() }
  if (rel === '' || rel.endsWith('/')) rel += 'index.html'
  if (!isFile(rel) && isFile(`${rel}.html`)) rel += '.html'
  const found = isFile(rel) && rel !== '_headers'
  if (!found) rel = '404.html'
  const headers = { 'Content-Type': TYPES[path.extname(rel)] ?? 'application/octet-stream', 'Cache-Control': 'no-cache' }
  for (const [pattern, values] of headerRules()) if (pattern.test(url.pathname)) Object.assign(headers, values)
  headers['Cache-Control'] = 'no-cache'
  const size = fs.statSync(path.join(root, rel)).size
  const range = found && /^bytes=(\d*)-(\d*)$/.exec(req.headers.range ?? '')
  if (range && (range[1] || range[2])) {
    const start = range[1] ? Number(range[1]) : Math.max(0, size - Number(range[2]))
    const end = range[1] && range[2] ? Math.min(Number(range[2]), size - 1) : size - 1
    if (start > end || start >= size) { res.writeHead(416, { 'Content-Range': `bytes */${size}` }); return res.end() }
    res.writeHead(206, { ...headers, 'Accept-Ranges': 'bytes', 'Content-Range': `bytes ${start}-${end}/${size}`, 'Content-Length': end - start + 1 })
    return fs.createReadStream(path.join(root, rel), { start, end }).on('error', () => res.destroy()).pipe(res)
  }
  res.writeHead(found ? 200 : 404, { ...headers, 'Accept-Ranges': 'bytes', 'Content-Length': size })
  fs.createReadStream(path.join(root, rel)).on('error', () => res.destroy()).pipe(res)
}).listen(port, '127.0.0.1', () => console.log(`site on http://127.0.0.1:${port}`))
