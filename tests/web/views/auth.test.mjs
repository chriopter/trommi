// auth.test.mjs: the account's screens and Settings (app/web/public/auth.mjs) drawn from a view model as the app core
// sends it: where the gate puts the page for each step, what a screen shows of AccountView (busy, problem, emoji, the
// kit's words), and what a form sends (an AccountAction). No core, no browser: a made-up view model and a stand-in for
// the board's toolkit.
//   node --test tests/web/views/
import test from 'node:test'
import assert from 'node:assert/strict'

const auth = await import('../../../app/web/public/auth.mjs')
const BASE = { phase: 'live', account: { step: 'none' }, settings: { account: null, devices: [], key_epoch: 3 }, sidebar: {}, desk: {}, notes: {} }

/** The board's toolkit as auth.mjs uses it, over a view model that a test sets. */
function board() {
  const gets = [], posts = [], sent = []
  let vm = BASE
  const t = {
    vm: () => vm, act: a => sent.push(a), account: a => sent.push({ do: 'account', action: a }), desk: () => null, toast: x => JSON.stringify(x),
    get: (p, h) => gets.push([p, h]), post: (p, h) => posts.push([p, h]),
    page: (q, res, o) => Object.assign(res, { kind: 'page', main: String(o.main), sidebar: o.sidebar }), redirect: (res, to) => Object.assign(res, { kind: 'redirect', to }),
    done: res => { res.kind = 'none' }, sendStream: (q, res, body) => Object.assign(res, { kind: 'stream', body }),
  }
  auth.register(t)
  const request = async (method, path, fields = {}) => {
    const url = new URL(path, 'http://app'), res = {}, form = new FormData()
    for (const [k, v] of Object.entries(fields)) form.append(k, v)
    if (method === 'GET') { const to = auth.gate(vm, url); if (to) return { kind: 'gate', to } }
    for (const [p, h] of method === 'GET' ? gets : posts) { const match = p.exec(url.pathname); if (match && (await h({ req: {}, res, url, match, form })) !== false) return res }
    return { kind: 'missing' }
  }
  return { set: v => { vm = { ...BASE, ...v, account: { step: 'none', ...v.account }, settings: { ...BASE.settings, ...v.settings } } }, request, sent }
}

test('the gate: signed out every address leads to the start, the screens and the self test stay; a step leads to its screen', () => {
  const at = (vm, path) => auth.gate({ ...BASE, ...vm }, new URL(path, 'http://app'))
  const out = { phase: 'signed_out', account: { step: 'sign_in' } }
  assert.equal(at(out, '/'), '/start')
  assert.equal(at(out, '/chat/x'), '/start')
  for (const p of ['/start', '/create', '/login', '/forgot', '/scan', '/settings/proof']) assert.equal(at(out, p), null, p)
  assert.equal(at({ phase: 'opening', account: { step: 'keep_kit', kit: ['a'] } }, '/'), '/kit')
  assert.equal(at({ phase: 'signed_out', account: { step: 'compare_emoji', emoji: [] } }, '/scan'), '/join')
  assert.equal(at({ phase: 'signed_out', account: { step: 'join' } }, '/join'), null)
  assert.equal(at({ phase: 'live', account: { step: 'compare_emoji', invite_id: 'ab' } }, '/'), '/settings')
  assert.equal(at({ phase: 'live', account: { step: '' } }, '/'), null, 'the demo\'s empty step is none')
})

test('a signed-out screen: the form sends its AccountAction, its button is busy while the core works, its problem is said', async () => {
  const b = board()
  b.set({ phase: 'signed_out', account: { step: 'sign_in' } })
  assert.equal((await b.request('GET', '/login')).sidebar, false, 'no frame before a room')
  await b.request('POST', '/login', { account: 'ada@example.org', password: 'pw' })
  assert.deepEqual(b.sent.at(-1), { do: 'account', action: { step: 'sign_in', email: 'ada@example.org', password: 'pw' } })
  b.set({ phase: 'signed_out', account: { step: 'working' } })
  assert.match((await b.request('GET', '/login')).main, /class="ob-go is-busy"[^>]*disabled>Logging in…|class="ob-second ob-wide is-busy"/)
  b.set({ phase: 'signed_out', account: { step: 'sign_in', problem: { key: 'account.wrong_login', en: 'The e-mail or the password is wrong.' } } })
  const page = (await b.request('GET', '/login')).main
  assert.match(page, /id="ob-error">The e-mail or the password is wrong\.</)
  assert.match(page, /value="ada@example.org"/, 'the e-mail typed stays')
  assert.doesNotMatch((await b.request('GET', '/create')).main, /wrong\./, 'a problem of the sign-in is not said on Create account')
})

test('joining: the emoji to compare, then what went wrong', async () => {
  const b = board()
  b.set({ phase: 'signed_out', account: { step: 'compare_emoji', emoji: [{ emoji: '🦊', word: 'fox' }, { emoji: '🌵', word: 'cactus' }] } })
  const page = (await b.request('GET', '/join')).main
  assert.match(page, /aria-label="Check code: fox, cactus"/)
  b.set({ phase: 'signed_out', account: { step: 'join', problem: { key: 'join.expired', en: 'The link has ended. Ask for a new one.' } } })
  assert.match((await b.request('GET', '/join')).main, /Not logged in[\s\S]*The link has ended/)
  b.set({ phase: 'signed_out', account: { step: 'sign_in' } })
  assert.deepEqual(await b.request('POST', '/join', { link: 'https://example.org/' }), { kind: 'redirect', to: '/scan?paste=1' }, 'no Trommi link: said on the scan screen')
})

test('the Emergency Kit: hidden until shown, its words only from the core, Open Trommi once it was shown', async () => {
  const b = board(), words = 'orbit lantern maple quiet saddle copper violin harbor ember thistle canyon pepper'.split(' ')
  b.set({ phase: 'live', account: { step: 'keep_kit', kit: words } })
  let page = (await b.request('GET', '/kit')).main
  assert.ok(!words.some(w => page.includes(`>${w}<`)), 'no word while hidden')
  assert.match(page, /id="kit-done"[^>]*disabled/)
  await b.request('POST', '/kit/do', { do: 'show', back: '/kit' })
  page = (await b.request('GET', '/kit')).main
  assert.ok(words.every(w => page.includes(`<li>${w}</li>`)) && !/id="kit-done"[^>]*disabled/.test(page))
  await b.request('POST', '/kit/do', { do: 'done' })
  assert.deepEqual(b.sent.at(-1), { do: 'account', action: { step: 'kit_kept' } })
})

test('Settings: rows from SettingsView, devices with their words, Push sends its level, a device invite in place', async () => {
  const b = board()
  const d = (id, name, o = {}) => ({ id, name, this_device: false, role: 'human', active: true, online: false, state: 'away', says: { key: 'x', en: 'away' }, fingerprint: 'abcd abcd abcd abcd', icon: 'phone', ...o })
  b.set({ settings: { agents: { en: '2 agents · 5 sessions' }, devices_says: { en: '2 devices' }, devices: [d('aa', 'Laptop', { this_device: true, says: { en: 'this device' }, icon: 'laptop' }), d('bb', 'Phone')], push: { level: 'knocking', busy: false } } })
  const home = (await b.request('GET', '/settings')).main
  assert.match(home, /id="settings-agents"[\s\S]*2 agents · 5 sessions/)
  assert.match(home, /id="settings-devices"[\s\S]*2 devices/)
  const devices = (await b.request('GET', '/settings/devices')).main
  assert.equal(devices.match(/room-device/g).length, 2)
  assert.match(devices, /value="knocking"[^<]*checked/)
  await b.request('POST', '/settings/push', { level: 'all' })
  assert.deepEqual(b.sent.at(-1), { do: 'push', level: 'all' })
  await b.request('POST', '/pair', { role: 'human' })
  assert.deepEqual(b.sent.at(-1), { do: 'account', action: { step: 'invite_device' } })
  const invite = (state, o = {}) => ({ invite_id: 'ab', role: 'human', state, expires_ms: Date.now() + 600_000, ...o })
  b.set({ account: { step: 'invite', invite_id: 'ab', link: 'https://app.trommi.com/join#v1.a.b.c', invite: invite('open') } })
  assert.match((await b.request('GET', '/settings')).main, /data-state="open"[\s\S]*set-qr is-real[\s\S]*works once, 10 more min\./)
  b.set({ account: { step: 'compare_emoji', invite_id: 'ab', emoji: [{ emoji: '🦊', word: 'fox' }], invite: invite('confirm_code') } })
  assert.match((await b.request('GET', '/settings')).main, /data-state="confirm_code"[\s\S]*They match/)
  // in: the core keeps it joined, with the newcomer's name, until Done drops it
  b.set({ account: { step: 'none', invite: invite('joined', { newcomer: 'Pixel 9', expires_ms: 0 }) } })
  assert.match((await b.request('GET', '/settings')).main, /data-state="joined"[\s\S]*Pixel 9 is in now/)
  await b.request('POST', '/pair/drop', { invite_id: 'ab' })
  assert.deepEqual(b.sent.at(-1), { do: 'account', action: { step: 'drop_invite', invite_id: 'ab' } })
})
