// Bounded reproduction of cross-page Blob retention, without credentials or a hub.
import assert from 'node:assert/strict'
import http from 'node:http'
import fs from 'node:fs/promises'
import { launchChromium } from '../../../app/web/dev/cdp.mjs'
const root = new URL('../../../', import.meta.url)
const server = http.createServer(async (req, res) => {
  const files = { '/core.mjs':'app/web/public/core.mjs', '/platform.mjs':'app/web/public/platform.mjs', '/ui.mjs':'app/web/public/ui.mjs', '/gen/vendor/views-fold.mjs':'core/bindings/wasm/js/views-fold.js' }
  res.setHeader('Content-Type', files[req.url] ? 'text/javascript' : 'text/html')
  res.end(files[req.url] ? await fs.readFile(new URL(files[req.url], root)) : '<!doctype html><body>Isolated file lifecycle</body>')
})
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
let browser
try {
  browser = await launchChromium({ width:800, height:600 })
  const page = await browser.page()
  await page.send('Page.enable')
  await page.send('Page.navigate', { url:`http://127.0.0.1:${server.address().port}/` })
  const r = await page.send('Runtime.evaluate', { awaitPromise:true, returnByValue:true, expression:`(async () => {
    let port, ended = 0; globalThis.Worker = class { constructor() { port = this } postMessage() {} terminate() { ended++ } }
    const live = new Map(), make = URL.createObjectURL.bind(URL), revoke = URL.revokeObjectURL.bind(URL)
    URL.createObjectURL = blob => { const url = make(blob); live.set(url, blob.size); return url }
    URL.revokeObjectURL = url => { live.delete(url); revoke(url) }
    const core = await import('/core.mjs'); await core.startCore({})
    const bytes = new Uint8Array(1024 * 1024)
    for (let i = 0; i < 80; i++) port.onmessage({ data:{ t:'file', id:'file-'+i, bytes } })
    const before = [...live.values()].reduce((a,b) => a+b,0)
    const image = document.createElement('img'); image.src = core.fileUrl('file-79'); document.body.append(image)
    core.releaseUnusedFiles(document, ['file-78'])
    const retained = live.size
    const visibleStatus = (await fetch(image.src)).status
    const canvasStatus = (await fetch(core.fileUrl('file-78'))).status
    const retired = core.fileUrl('file-0')
    image.remove(); document.dispatchEvent(new Event('turbo:render'))
    const after = live.size
    port.onmessage({ data:{ t:'file', id:'last', bytes } })
    port.onmessage({ data:{ t:'taken' } })
    return { before, retained, visibleStatus, canvasStatus, retired, after, takeover:live.size, ended }
  })()` })
  if (r.exceptionDetails) throw Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text)
  assert.deepEqual(r.result.value, { before:80*1024*1024, retained:2, visibleStatus:200, canvasStatus:200, retired:null, after:0, takeover:0, ended:1 })
  console.log('file memory: 80 MiB before cleanup; only two displayed/canvas files retained; zero after leaving')
  const previews = await page.send('Runtime.evaluate', { awaitPromise:true, returnByValue:true, expression:`(async () => {
    let reads = 0, largeReads = 0
    const original = fetch
    globalThis.fetch = async src => {
      if (src.startsWith('/preview/')) { reads++; return new Response('<p>Small page</p>') }
      if (src === '/large-preview') { largeReads++; return new Response('x'.repeat(1024 * 1024 + 1)) }
      return original(src)
    }
    const ui = await import('/ui.mjs'); ui.startUi()
    async function show(src) {
      const el = document.createElement('div')
      el.dataset.controller = 'assetthumb'; el.dataset.assetthumbSrcValue = src; el.dataset.assetthumbFullValue = 'true'
      document.body.append(el)
      for (let i = 0; i < 100 && !el.querySelector('iframe'); i++) await new Promise(r => setTimeout(r, 10))
      if (!el.querySelector('iframe')) throw Error('the published page did not mount')
      el.remove(); await new Promise(r => setTimeout(r, 0))
    }
    for (let i = 0; i < 20; i++) await show('/preview/' + i)
    await show('/preview/0')
    await show('/large-preview'); await show('/large-preview')
    return { reads, largeReads }
  })()` })
  if (previews.exceptionDetails) throw Error(previews.exceptionDetails.exception?.description ?? previews.exceptionDetails.text)
  assert.deepEqual(previews.result.value, { reads:21, largeReads:2 })
  console.log('preview memory: old pages evicted; oversized HTML not retained after its viewer leaves')

} finally { await browser?.close(); await new Promise(resolve => server.close(resolve)) }
