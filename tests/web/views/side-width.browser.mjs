// side-width.browser.mjs: on a wide screen the sidebar's parts follow its width (the owner, 17 October 2026: dragged
// wider, Search and + stayed in the middle). Demo room (?mock=1) at the sidebar's default width and dragged wide
// (localStorage 'trommi-side-w', read by the page's head): Search and + stand at the sidebar's right edge, every
// team's head card reaches it, and the foot's button stays inside the sidebar.
//   node tests/web/views/side-width.browser.mjs      (needs Chromium; serves the app from its sources itself)
import net from 'node:net'
import path from 'node:path'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { launchChromium } from '../../../app/web/dev/cdp.mjs'

const here = path.dirname(fileURLToPath(import.meta.url))
const repo = path.join(here, '..', '..', '..')
const sleep = ms => new Promise(r => setTimeout(r, ms))
const WIDTHS = [null, 380]

const PROBE = `(() => {
  const box = sel => document.querySelector(sel)?.getBoundingClientRect() ?? null
  const side = box('#agents'), add = box('#side-add'), search = box('.side-tool'), foot = box('#brand-menu')
  const heads = [...document.querySelectorAll('#agents .team-head .team-go')].map(e => e.getBoundingClientRect())
  return { side: side && [side.left, side.right], add: add && add.right, search: search && search.right, foot: foot && [foot.left, foot.right], heads: heads.map(h => h.right) }
})()`

async function freePort() {
  return new Promise((resolve, reject) => { const s = net.createServer(); s.unref(); s.on('error', reject); s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => resolve(port)) }) })
}
const port = await freePort()
const server = spawn(process.execPath, [path.join(repo, 'app', 'web', 'dev', 'serve.mjs'), String(port)], { stdio: ['ignore', 'pipe', 'inherit'] })
process.on('exit', () => server.kill())
await new Promise((resolve, reject) => { server.stdout.on('data', d => { if (String(d).includes('http://')) resolve() }); server.on('exit', c => reject(new Error(`the app server ended (${c})`))) })
const APP = `http://127.0.0.1:${port}`
const failed = []
for (const width of WIDTHS) {
  const browser = await launchChromium({ width: 1440, height: 900 })
  try {
    const page = await browser.page()
    await page.send('Runtime.enable'); await page.send('Page.enable')
    await page.send('Emulation.setDeviceMetricsOverride', { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false })
    if (width) await page.send('Page.addScriptToEvaluateOnNewDocument', { source: `try{localStorage.setItem('trommi-side-w','${width}')}catch(e){}` })
    const ev = async x => { const r = await page.send('Runtime.evaluate', { expression: x, awaitPromise: true, returnByValue: true }); if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text); return r.result.value }
    await page.send('Page.navigate', { url: `${APP}/?mock=1` })
    for (let i = 0; i < 100; i++) { if (await ev("document.readyState === 'complete' && document.documentElement.hasAttribute('data-ready') && Boolean(document.querySelector('#agents .team-head'))").catch(() => false)) break; await sleep(100) }
    await sleep(600)
    const got = await ev(PROBE)
    const bad = []
    const right = got.side?.[1] ?? 0
    if (width && Math.abs(right - width) > 2) bad.push(`the sidebar is ${right}px wide, not ${width}`)
    if (got.add == null || right - got.add > 24 || got.add > right) bad.push(`+ ends at ${got.add}, the sidebar at ${right}`)
    if (got.search == null || got.search > got.add) bad.push('Search is not left of +')
    if (!got.heads.length || got.heads.some(h => right - h > 24 || h > right)) bad.push(`a team's head ends at ${got.heads.join(', ')}, the sidebar at ${right}`)
    if (!got.foot || got.foot[1] > right || got.foot[0] < 0) bad.push(`the foot's button stands outside the sidebar: ${got.foot}`)
    const id = width ? `${width}px` : 'default'
    console.log(`${bad.length ? 'FAIL' : 'ok  '} ${id}${bad.length ? `: ${bad.join('; ')}` : ''}`)
    if (bad.length) failed.push(id)

    // Real pointer drag through the existing order controller, then the real core's new root placement.
    await ev(`document.querySelector('#agents [data-team-fold]').click(); true`)
    await sleep(300)
    const moving = await ev(`(() => { const r=document.querySelector('#agents .agent-row[data-team-of]:not(.in-folded-team)'); const b=r?.getBoundingClientRect(); return b && {id:r.dataset.unit,x:b.x+b.width/2,y:b.y+b.height/2} })()`)
    if (!moving) throw Error('the actual demo has no visible Team chat to drag')
    if (!await ev(`!!document.getElementById('chat-root-drop')`)) throw Error('missing root drop target for moving a chat outside its Team')
    await page.send('Input.dispatchMouseEvent', { type:'mousePressed', x:moving.x, y:moving.y, button:'left', clickCount:1 })
    await page.send('Input.dispatchMouseEvent', { type:'mouseMoved', x:moving.x, y:moving.y-12, button:'left', buttons:1 })
    const target = await ev(`(() => { const b=document.getElementById('chat-root-drop').getBoundingClientRect(); return {x:b.x+b.width/2,y:b.y+b.height/2} })()`)
    await page.send('Input.dispatchMouseEvent', { type:'mouseMoved', x:target.x, y:target.y, button:'left', buttons:1 })
    await page.send('Input.dispatchKeyEvent', { type:'keyDown', key:'Escape', code:'Escape', windowsVirtualKeyCode:27 })
    await page.send('Input.dispatchMouseEvent', { type:'mouseReleased', x:target.x, y:target.y, button:'left', clickCount:1 })
    if (await ev(`window.trommi.vm().sidebar.root?.some(r=>r.id===${JSON.stringify(moving.id)})`)) throw Error('Escape must cancel a root move')
    const again = await ev(`(() => { const b=document.getElementById('agent-'+${JSON.stringify(moving.id)}).getBoundingClientRect(); return {x:b.x+b.width/2,y:b.y+b.height/2} })()`)
    await page.send('Input.dispatchMouseEvent', { type:'mousePressed', x:again.x, y:again.y, button:'left', clickCount:1 })
    await page.send('Input.dispatchMouseEvent', { type:'mouseMoved', x:again.x, y:again.y-12, button:'left', buttons:1 })
    await page.send('Input.dispatchMouseEvent', { type:'mouseMoved', x:target.x, y:target.y, button:'left', buttons:1 })
    if (process.env.TROMMI_ROOT_PROOF_DIR) {
      const fs=await import('node:fs'), shot=await page.send('Page.captureScreenshot',{format:'png'})
      fs.writeFileSync(path.join(process.env.TROMMI_ROOT_PROOF_DIR,`web-root-drop-${id}.png`),Buffer.from(shot.data,'base64'))
    }
    await page.send('Input.dispatchMouseEvent', { type:'mouseReleased', x:target.x, y:target.y, button:'left', clickCount:1 })
    // The core changes first; the shell paints on its next frame. Wait for both without relaxing any assertion.
    let moved
    for (let n=0;n<100;n++) {
      moved = await ev(`(() => { const vm=window.trommi.vm(), r=document.getElementById('agent-'+${JSON.stringify(moving.id)}); return {root:vm.sidebar.root?.some(r=>r.id===${JSON.stringify(moving.id)}),inTeam:vm.sidebar.stacks.some(s=>s.sessions?.some(r=>r.id===${JSON.stringify(moving.id)})),nested:!!r?.dataset.teamOf,first:r?.compareDocumentPosition(document.querySelector('#agents .team-head'))&Node.DOCUMENT_POSITION_FOLLOWING} })()`)
      if (moved.root && !moved.inTeam && !moved.nested && moved.first) break
      await sleep(50)
    }
    if (!moved.root || moved.inTeam || moved.nested || !moved.first) throw Error('drag-out did not put the actual chat at root: '+JSON.stringify(moved))
    console.log(`ok  ${id}: actual pointer drag moves Team chat to root`)
    if (process.env.TROMMI_ROOT_PROOF_DIR) {
      const fs = await import('node:fs')
      const shot=await page.send('Page.captureScreenshot',{format:'png'})
      fs.writeFileSync(path.join(process.env.TROMMI_ROOT_PROOF_DIR,`web-root-${id}.png`),Buffer.from(shot.data,'base64'))
    }
  } finally { await browser.close() }
}
server.kill()
if (failed.length) { console.log(`${failed.length} failed: ${failed.join(', ')}`); process.exit(1) }
console.log(`the sidebar's parts follow its width at ${WIDTHS.length} widths`)
