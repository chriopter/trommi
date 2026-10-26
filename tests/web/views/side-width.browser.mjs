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

// Newest first (the owner, 25 October 2026): a chat has no place of its own within its list, so dragging it among its
// own list's rows or Alt+↑/↓ on it changes nothing and shows no insert; moving it to another Team or to no Team, and
// the Team folders' own order (drag and Alt+↑/↓), stay. A real message sent in the demo puts its chat first, at the
// root and in a Team. Everything through the page and the demo's real core (?mock=1), in a fresh browser.
{
  const browser = await launchChromium({ width: 1440, height: 900 })
  try {
    const page = await browser.page()
    await page.send('Runtime.enable'); await page.send('Page.enable')
    await page.send('Emulation.setDeviceMetricsOverride', { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false })
    const ev = async x => { const r = await page.send('Runtime.evaluate', { expression: x, awaitPromise: true, returnByValue: true }); if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text); return r.result.value }
    const until = async (x, what, ms = 5000) => { for (const end = Date.now() + ms; Date.now() < end; await sleep(50)) { if (await ev(x).catch(() => false)) return } throw Error(`${what}: ${JSON.stringify(await ev(STATE).catch(e => String(e)))}`) }
    const mouse = (type, x, y) => page.send('Input.dispatchMouseEvent', { type, x, y, button: 'left', buttons: type === 'mouseReleased' ? 0 : 1, clickCount: 1 })
    const key = (k, code, vk, modifiers = 0) => page.send('Input.dispatchKeyEvent', { type: 'keyDown', key: k, code, windowsVirtualKeyCode: vk, modifiers }).then(() => page.send('Input.dispatchKeyEvent', { type: 'keyUp', key: k, code, windowsVirtualKeyCode: vk, modifiers }))
    const J = JSON.stringify
    // (what the core says and what the page shows: the root's and every Team's chats, the Teams' order)
    const STATE = `(() => { const vm = window.trommi.vm().sidebar, dom = sel => [...document.querySelectorAll(sel)].map(n => n.dataset.unit ?? n.dataset.team)
      return { root: (vm.root ?? []).map(r => r.id), teams: vm.stacks.map(s => s.id), stacks: Object.fromEntries(vm.stacks.map(s => [s.id, (s.sessions ?? []).map(r => r.id)])),
        domRoot: dom('#agents > .agent-row[data-unit]:not([data-team-of]):not(.order-ghost)'), domTeams: dom('#agents > .team-head[data-team]'),
        domStacks: Object.fromEntries(vm.stacks.map(s => [s.id, dom('#agents > .agent-row[data-team-of="' + s.id + '"]:not(.order-ghost)')])) } })()`
    const state = () => ev(STATE)
    const center = sel => ev(`(() => { const b = document.querySelector(${J(sel)})?.getBoundingClientRect(); return b && { x: b.x + b.width / 2, y: b.y + b.height / 2 } })()`)
    // (the demo's room lives in the page: it is opened once, and every step goes on in it)
    const unfold = async () => {
      await ev(`document.querySelectorAll('#agents .team-head[data-fold="shut"] button[data-team-fold]').forEach(b => b.click()); true`)
      await until(`!document.querySelector('#agents .team-head[data-fold="shut"]:not(.is-empty)')`, 'the Teams did not open')
    }
    const settled = s => s.root.join() === s.domRoot.join() && s.teams.join() === s.domTeams.join() && Object.keys(s.stacks).every(t => s.stacks[t].join() === s.domStacks[t].join())
    await page.send('Page.navigate', { url: `${APP}/?mock=1` })
    await until("document.readyState === 'complete' && document.documentElement.hasAttribute('data-ready') && Boolean(document.querySelector('#agents .team-head'))", 'the demo did not start')
    await unfold()
    let s = await state()
    if (!settled(s)) throw Error('the page does not draw the core\'s order: ' + J(s))

    // the core offers no rank move for a chat: no order part, no Alt+↑/↓ named on its entry; the Team heads keep theirs
    const offered = await ev(`({ order: document.querySelectorAll('#agents .agent-row[data-order]').length, keys: document.querySelectorAll('#agents .agent-row .agent-entry[aria-keyshortcuts]').length,
      heads: [...document.querySelectorAll('#agents .team-head .team-go')].every(a => a.getAttribute('aria-keyshortcuts') === 'Alt+ArrowUp Alt+ArrowDown') })`)
    if (offered.order || offered.keys || !offered.heads) throw Error('chats must offer no rank move, Team heads keep theirs: ' + J(offered))
    console.log('ok  newest first: chats offer no rank move, Team folders do')

    // the same list: a chat dragged past its neighbour opens no place there and lands where it stood
    const team = s.teams.find(t => s.stacks[t].length >= 3)
    if (!team) throw Error('the demo has no Team with three chats')
    const [first, second] = s.stacks[team]
    const a = await center(`#agent-${first} > .agent-entry`), b = await center(`#agent-${s.stacks[team][2]} > .agent-entry`)
    await mouse('mousePressed', a.x, a.y)
    await mouse('mouseMoved', a.x, a.y + 12)
    for (let i = 1; i <= 6; i++) await mouse('mouseMoved', a.x, a.y + (b.y + 10 - a.y) * i / 6)
    const during = await state()
    if (during.domStacks[team].join() !== s.stacks[team].join()) throw Error('a drag within the list must not open a place among its rows: ' + J(during.domStacks[team]))
    await mouse('mouseReleased', b.x, b.y + 10)
    await sleep(400)
    let after = await state()
    if (after.stacks[team].join() !== s.stacks[team].join() || !settled(after)) throw Error('a drag within the list changed the order: ' + J(after))
    console.log('ok  newest first: a drag within the list is no move and shows no insert')

    // Alt+↓ on a chat: nothing; Alt+↓ on a Team's head: the Teams' order, by keyboard, the focus staying on the head
    await ev(`document.querySelector('#agent-${second} > .agent-entry').focus(); true`)
    await key('ArrowDown', 'ArrowDown', 40, 1)
    await sleep(400)
    after = await state()
    if (after.stacks[team].join() !== s.stacks[team].join() || !settled(after)) throw Error('Alt+↓ on a chat must not move it: ' + J(after))
    const [t0, t1] = s.teams
    await ev(`document.querySelector('#team-${t0} > .team-go').focus(); true`)
    await key('ArrowDown', 'ArrowDown', 40, 1)
    await until(`(${STATE}).teams.join() === ${J([t1, t0, ...s.teams.slice(2)].join())} && (${STATE}).domTeams[1] === ${J(t0)}`, 'Alt+↓ did not move the Team down')
    await until(`document.activeElement === document.querySelector('#team-${t0} > .team-go')`, 'the keyboard left the moved Team')
    console.log('ok  Team folders: Alt+↓ moves a Team, Alt+↓ on a chat does nothing')

    // a Team's head dragged above the other's: the Teams' order (OrderTeams), drawn by the core
    const h0 = await center(`#team-${t0} > .team-go`), h1 = await center(`#team-${t1} > .team-go`)
    await mouse('mousePressed', h0.x, h0.y)
    await mouse('mouseMoved', h0.x, h0.y - 12)
    for (let i = 1; i <= 6; i++) await mouse('mouseMoved', h0.x, h0.y + (h1.y - 12 - h0.y) * i / 6)
    await mouse('mouseReleased', h0.x, h1.y - 12)
    await until(`(${STATE}).teams.join() === ${J(s.teams.join())}`, 'dragging the Team head back did not restore the Teams\' order')
    console.log('ok  Team folders: a dragged Team head takes its new place')

    // a real pointer drag: pressed on the chat, lifted, then the target is measured (the root's place has no height
    // until a drag is live) and the pointer goes there; `during` is read while it hovers, Escape cancels before release
    const shot = async name => { if (!process.env.TROMMI_ROOT_PROOF_DIR) return; const fs = await import('node:fs'), p = await page.send('Page.captureScreenshot', { format: 'png' }); fs.writeFileSync(path.join(process.env.TROMMI_ROOT_PROOF_DIR, `${name}.png`), Buffer.from(p.data, 'base64')) }
    const drag = async (id, target, { escape = false, during = 'null' } = {}) => {
      // (pressed only once the row stands still: the last move's drawing and its animations are over)
      let a = await center(`#agent-${id} > .agent-entry`)
      for (let n = 0; n < 40; n++) { await sleep(60); const c = await center(`#agent-${id} > .agent-entry`); if (c && a && c.x === a.x && c.y === a.y && await ev(`!document.getAnimations().length`)) break; a = c }
      await mouse('mousePressed', a.x, a.y); await mouse('mouseMoved', a.x, a.y - 12)
      await until(`document.querySelector('#agents').classList.contains('is-ordering')`, `the chat ${id} was not lifted (towards ${target}${escape ? ', Escape' : ''}; pressed at ${J(a)} on ${await ev(`document.elementFromPoint(${a.x}, ${a.y})?.outerHTML.slice(0, 160)`)})`, 1500).catch(async e => { await shot('web-not-lifted'); throw e })
      const b = await center(target)
      if (!b) throw Error(`no drop target ${target}`)
      for (let i = 1; i <= 6; i++) await mouse('mouseMoved', a.x + (b.x - a.x) * i / 6, a.y - 12 + (b.y - a.y + 12) * i / 6)
      const seen = await ev(during)
      if (escape) await key('Escape', 'Escape', 27)
      await mouse('mouseReleased', b.x, b.y)
      return seen
    }
    const where = (id, to) => `(() => { const s = ${STATE}, all = Object.values(s.stacks).flat(); return ${to
      ? `s.stacks[${J(to)}]?.includes(${J(id)}) && s.domStacks[${J(to)}]?.includes(${J(id)}) && !s.root.includes(${J(id)}) && all.filter(x => x === ${J(id)}).length === 1`
      : `s.root.includes(${J(id)}) && s.domRoot.includes(${J(id)}) && !all.includes(${J(id)})`} })()`
    const same = (x, y) => J(x.root) === J(y.root) && J(x.stacks) === J(y.stacks) && J(x.teams) === J(y.teams)

    // a Team chat dragged onto the root's place, Escape: nothing moves; then dragged and let go: it is on no Team
    s = await state()
    const out = s.stacks[team].at(-1)
    await drag(out, '#chat-root-drop', { escape: true })
    await sleep(400)
    after = await state()
    if (!same(after, s) || !settled(after)) throw Error('Escape must cancel a root move: ' + J(after))
    await drag(out, '#chat-root-drop')
    await until(where(out, null), 'dragging a Team chat onto the root did not move it to no Team')
    await unfold()
    console.log('ok  Team → root: Escape cancels, a drop moves the chat to no Team')

    // another Team, by pointer: onto its own Team's head nothing happens and nothing is marked; Escape over the other
    // Team's head cancels; let go over it, and the chat is in that Team (MoveSession), drawn there
    s = await state()
    const other = s.teams.find(t => t !== team), crossing = s.stacks[team].at(-1)
    const marked = `[...document.querySelectorAll('#agents .team-head.is-over')].map(h => h.dataset.team)`
    const ownMark = await drag(crossing, `#team-${team} > .team-go`, { during: marked })
    await sleep(400)
    after = await state()
    if (!same(after, s) || !settled(after)) throw Error('a drop on its own Team\'s head must change nothing: ' + J(after))
    if (ownMark.length) throw Error('its own Team\'s head must not be marked as a target: ' + J(ownMark))
    await drag(crossing, `#team-${other} > .team-go`, { escape: true })
    await sleep(400)
    after = await state()
    if (!same(after, s) || !settled(after)) throw Error('Escape must cancel a move to another Team: ' + J(after))
    const otherMark = await drag(crossing, `#team-${other} > .team-go`, { during: marked })
    await until(where(crossing, other), 'dragging a chat onto another Team\'s head did not move it there')
    if (J(otherMark) !== J([other])) throw Error('the other Team\'s head must be the one marked target while over it: ' + J(otherMark))
    await unfold()
    await shot('web-cross-team')
    console.log('ok  another Team, by pointer: own head no-op, Escape cancels, a drop moves the chat there')

    // a root chat dragged onto a Team's head: it is in that Team
    s = await state()
    const lone = s.root[0]
    if (!lone) throw Error('the root has no chat to drag into a Team: ' + J(s.root))
    await drag(lone, `#team-${team} > .team-go`, { escape: true })
    await sleep(400)
    after = await state()
    if (!same(after, s) || !settled(after)) throw Error('Escape must cancel a root → Team move: ' + J(after))
    await drag(lone, `#team-${team} > .team-go`)
    await until(where(lone, team), 'dragging a root chat onto a Team\'s head did not move it there')
    await unfold()
    after = await state()
    if (!settled(after)) throw Error('the page does not draw the core\'s order after the moves: ' + J(after))
    await drag(lone, '#chat-root-drop')
    await until(where(lone, null), 'dragging the chat back onto the root did not move it to no Team')
    await unfold()
    console.log('ok  root → Team, by pointer: Escape cancels, a drop moves the chat into the Team (and back out)')

    // the chat's own menu still moves it out: Move to → No team (the real form, the core's move)
    const menuMove = async (id, to, what) => {
      await ev(`document.querySelector('#agent-${id} > .agent-entry').click(); true`)
      const sel = `form[action$="/sessions/${id}/edit"] button[name="team"][value="${to}"]`
      await until(`!!document.querySelector(${J(sel)})`, `${what}: the chat's menu has no such entry`)
      await ev(`document.querySelector(${J(sel)}).click(); true`)
      await until(where(id, to || null), `${what} did not move the chat`)
      await unfold()
    }
    s = await state()
    await menuMove(s.stacks[team].at(-1), '', 'Move to → No team')
    console.log('ok  No team: the menu moves a chat out of its Team')

    // a real message: the chat it went to stands first, at the root and in its Team
    const send = async (id, where) => {
      await ev(`document.querySelector('#agent-${id} > .agent-entry').click(); true`)
      await until(`!!document.getElementById('composer-field-${id}')`, `${where}: the chat did not open`)
      await ev(`document.getElementById('composer-field-${id}').focus(); true`)
      await page.send('Input.insertText', { text: `newest first ${where} ${Date.now()}` })
      const go = await center(`#composer-${id} button.send`)
      await mouse('mousePressed', go.x, go.y); await mouse('mouseReleased', go.x, go.y)
    }
    s = await state()
    if (s.root.length < 2) throw Error('the root needs two chats for the send check: ' + J(s.root))
    const lastRoot = s.root.at(-1)
    await send(lastRoot, 'root')
    await until(`(() => { const s = ${STATE}; return s.root[0] === ${J(lastRoot)} && s.domRoot[0] === ${J(lastRoot)} })()`, 'a message sent did not put its root chat first')
    console.log('ok  a real message puts its chat first at the root')
    s = await state()
    const busy = s.teams.find(t => s.stacks[t].length >= 2), lastTeam = s.stacks[busy].at(-1)
    await send(lastTeam, 'team')
    await until(`(() => { const s = ${STATE}; return s.stacks[${J(busy)}][0] === ${J(lastTeam)} && s.domStacks[${J(busy)}][0] === ${J(lastTeam)} })()`, 'a message sent did not put its Team chat first')
    console.log('ok  a real message puts its chat first in its Team')
  } catch (e) { console.log(`FAIL newest first: ${e.message}`); failed.push('newest first') } finally { await browser.close() }
}
server.kill()
if (failed.length) { console.log(`${failed.length} failed: ${failed.join(', ')}`); process.exit(1) }
console.log(`the sidebar's parts follow its width at ${WIDTHS.length} widths`)
