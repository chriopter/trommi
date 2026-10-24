// ---- the account and Settings ----
// Drawn from the app core's view model: before a room (vm.phase 'signed_out') and while an account step is under way
// (vm.account.step), the screens of their own: /start, /create, /login, /forgot, /scan, /join, /kit, /new-way (one calm
// column, auth.css "ob"); inside a room the pages of Settings: /settings (the list, with Invite a Device and Invite an
// Agent), /settings/devices (each device a page of its own, Push on this device), /settings/account (its ways in, each a
// page), /settings/security, /settings/theme, /settings/keys, /pair/agent (an agent's clipboard), /logout. What the
// person does goes to the core as an AccountAction (t.account) or an Action (t.act); the core says how far it is
// (step 'working'), what went wrong (problem), the emoji, the kit's words. The page only remembers what it drew: which
// form was sent (its busy button), which invite was asked for here, whether the kit's words are shown.
// app.mjs asks gate(vm, url) on every visit while signed out or while a step runs: the address the page must be on.
// The UI says "account", never "room". Calm and sober: pen drawings only on the choice buttons.
import { BELL, Controller, PLUS, SET_CHEVRON, controller, copyText, doodleSvg, errorLine, html, keysList, raw, roomPage, roomShell, setRow, setThemeMode, settingsPage, sk, sketchSvg, themeMode } from './ui.mjs'
import { actCore, passkeysOn, pushObstacle } from './app.mjs'

// (the page's core and the QR reader come when first needed: this module also loads in Node, for its tests)
const qrReader = () => import('./app.mjs').then(app => app.qrReader())
const art = name => raw(['phone', 'house'].includes(name) ? doodleSvg(`draw:${name}`) : sketchSvg(name))
const isDemo = () => Boolean(globalThis.document?.documentElement.classList.contains('is-demo'))
const day = ms => new Date(ms).toLocaleDateString('en-GB', { day: 'numeric', month: 'short', year: 'numeric' })

// ---- what the page remembers (drawing state only; nothing of the account) ----
const local = {
  sent: null,          // the form sent last: { path, word, passkey } (its button is busy while the step is 'working')
  email: '',           // the e-mail typed last, to start the next form with
  begun: false,        // this page signed in, made an account or joined: its screens lead to the Desk once in
  loggedOut: false,    // "Logged out." on the start page
  invite: null,        // the invite asked for here: { kind: 'device' | 'agent' } (where it stands: vm.account.invite)
  kitFrom: 'start',    // where a new kit is shown: 'start' (/kit) or 'settings' (/settings/account/kit)
  kit: { shown: false, saved: false },
  words: null,         // the Emergency Kit's words of a recovery, held for its new passkey or password (/new-way)
  paste: '',           // what the pasted link was refused for
  readAt: 0,           // when the account was last asked for (ReadAccount)
}
const stepOf = vm => vm.account?.step || 'none'
const signedOut = vm => vm.phase === 'signed_out'

// ---- the gate ----
/** The signed-out screens, by address. */
const OUT = { '/start': 'start', '/create': 'create', '/login': 'login', '/forgot': 'forgot', '/scan': 'scan', '/join': 'join', '/kit': 'kit', '/new-way': 'newway' }
const kitPath = () => (local.kitFrom === 'settings' ? '/settings/account/kit' : '/kit')
/** The address the page must be on (app.mjs, on every visit while signed out or while an account step runs), or null. */
export function gate(vm, url) {
  const av = vm.account ?? {}, step = stepOf(vm), p = url.pathname
  const go = to => (p === to ? null : to)
  if (step === 'keep_kit') return go(kitPath())
  // (a kit that was never saved, on any device of the person: its page first, where a new one is made)
  if (av.kit_due && step === 'none' && !signedOut(vm) && p !== '/logout') return go('/kit')
  // (an invite of this device: its emoji and its adding stand where it was asked for)
  if (av.invite_id && ['compare_emoji', 'working'].includes(step)) return go(local.invite?.kind === 'agent' ? '/pair/agent' : '/settings')
  if (step === 'join' || step === 'compare_emoji') return go('/join')
  if (!signedOut(vm)) return null
  if (p in OUT || p === '/settings/proof') return null
  return '/start'
}

// ---- the screens before the board: one calm column (auth.css "ob") ----
const UNDER = raw('<svg viewBox="0 0 100 8" preserveAspectRatio="none" aria-hidden="true"><path d="M1.0 5.0 Q18.0 3.4 26.0 4.3 Q34.0 5.2 43.0 4.4 Q52.0 3.6 61.0 4.3 Q70.0 5.0 78.0 4.4 Q86.0 3.8 92.5 4.2 L99.0 4.6"/></svg>')
const DEMO_LINK = html`<a href="/?mock=1" id="ob-demo" data-turbo="false">Try the demo</a>`
/** On the start page the demo is the third way, right under Create account and Log in: a clear quieter button. */
const DEMO_WAY = html`<a href="/?mock=1" id="ob-demo" class="ob-demo-way" data-turbo="false"><span>Try the demo</span><small>A made-up room: nothing is sent</small></a>`
/** A button that leads to another screen (the look of a button, the way of a link: controller "ob-nav"). */
const goButton = (path, cls, id, inner) => html`<button type="button" class="${cls}" id="${id}" data-controller="ob-nav" data-action="ob-nav#go" data-ob-nav-to-value="${path}">${inner}</button>`
/** One screen: the bell (the way back to the start), a short headline, at most one line under it, the demo at the
 *  foot (demo: false where the screen offers it itself, the start page). */
const obShell = (title, inner, { lead = '', cls = '', home = true, foot = '', demo = true, focus = 'input:not([type=hidden], [hidden]), button.ob-go' } = {}) => html`<main id="room" class="ob${cls ? ` ${cls}` : ''}" aria-labelledby="ob-title"${focus ? html` data-controller="ob-focus" data-ob-focus-at-value="${focus}"` : ''}>
<header class="ob-brand">${home ? html`<a href="/start" data-nav id="room-home" aria-label="Trommi: back to the start">${BELL}<b>Trommi</b></a>` : html`<span>${BELL}<b>Trommi</b></span>`}</header>
<h1 id="ob-title" class="ob-title">${title}</h1>${lead ? html`<p class="ob-lead">${lead}</p>` : ''}
${inner}
<footer class="ob-foot">${foot}${demo ? DEMO_LINK : ''}</footer></main>`
/** The one line a form says when it failed: its place is kept, so nothing jumps. */
const obError = (error = '') => html`<p class="ob-error" role="alert" id="ob-error">${error}</p>`
const obEmail = (value = '', optional = false) => html`<div class="ob-field"><span class="ob-label"><label for="ob-email">Email</label>${optional ? html`<span class="ob-hint" id="ob-email-optional">optional</span>` : ''}</span><input type="email" id="ob-email" name="email" value="${value}" autocomplete="username" autocapitalize="off" spellcheck="false" inputmode="email"${optional ? raw(' aria-describedby="ob-email-optional"') : ''}></div>`
/** The one field that names an account: its email, or the account ID printed on its Emergency Kit. */
const obAccount = (value = '', passkeys = false) => html`<label class="ob-field"><span class="ob-label">Email or account ID</span><input type="text" name="account" value="${value}" autocomplete="${passkeys ? 'username webauthn' : 'username'}" autocapitalize="off" spellcheck="false" inputmode="email"></label>`
const PW_RULE = 'At least 12 characters'
/** A password, with Show/Hide in the field (controller "ob-pw"); a new one says the rule under it. */
const obPassword = ({ label = 'Password', fresh = false, side = '', name = 'password' } = {}) => html`<div class="ob-field ob-pw${fresh ? ' is-new' : ''}" data-controller="ob-pw"><span class="ob-label"><label for="ob-pw">${label}</label>${side}</span>
<span class="ob-pwrow"><input type="password" id="ob-pw" name="${name}" autocomplete="${fresh ? 'new-password' : 'current-password'}" spellcheck="false" autocapitalize="off" data-ob-pw-target="field"${fresh ? raw(' aria-describedby="ob-pw-hint"') : ''}><button type="button" class="ob-eye" aria-pressed="false" aria-controls="ob-pw" data-action="ob-pw#eye">Show</button></span>
${fresh ? html`<span class="ob-under"><span class="ob-hint" id="ob-pw-hint">${PW_RULE}</span></span>` : ''}</div>`
/** A form's button; busy (its word: "Creating…") while the core works on what it sent. */
const obSubmit = (word, busy = '', cls = 'ob-go', id = '') => html`<button type="submit" class="${cls}${busy ? ' is-busy' : ''}"${id ? raw(` id="${id}"`) : ''} data-word="${word}"${busy ? raw(' disabled') : ''}>${busy || word}</button>`
const NO_PASSKEYS = html`<p class="ob-alt ob-way ob-none" id="no-passkeys">No passkeys in this browser. A password works everywhere.</p>`
const NO_PRF = 'The passkey was saved but cannot unlock Trommi. Delete it in your password manager and use a password.'
const NO_PRF_HERE = 'This passkey can\'t unlock Trommi here. Use your password.'
const ID_NO_PASSWORD = 'Enter your email to log in with a password.'
/** The six emoji with a word under each: the same on both devices (and in a connector's terminal). */
const emojiRow = (emoji = [], id = '') => html`<ol class="check-emoji"${id ? raw(` id="${id}"`) : ''} aria-label="Check code: ${emoji.map(e => e.word).join(', ')}">${emoji.map(e => html`<li><span class="check-emoji-glyph" aria-hidden="true">${e.emoji}</span><span class="check-emoji-word">${e.word}</span></li>`)}</ol>`

// The Emergency Kit: its words come from the core (AccountView.kit) and are drawn only while shown; what it is for (the
// e-mail, the account's ID) from the account once read (SettingsView.login).
const KIT_EMPTY = 'orbit lantern maple quiet saddle copper violin harbor ember thistle canyon pepper'
const kitOpensWith = kit => (!kit.email ? 'account ID' : 'email')
const LOST = way => `If you lose your ${way} and this kit, your account is lost. Nobody can recover it, not even Trommi.`
const kitText = (kit, words) => `Trommi Emergency Kit

${kit.account ? `Account ID: ${kit.account}\n` : ''}${kit.email ? `Email: ${kit.email}\n` : ''}Recovery words: ${words.join(' ')}

Lost your way in? Open https://app.trommi.com, choose "Log in", then "Forgot?".
Enter your ${kitOpensWith(kit)} and these 12 words, then choose a new ${kit.email ? 'password or passkey' : 'passkey'}.
${kit.link ? `Or open ${kit.link} (it fills in the account ID; it holds no words).\n` : ''}
Keep this kit private and offline: with these words and your ${kitOpensWith(kit)}, anyone can get into your account.
${LOST(kit.has_password === false || !kit.email ? 'passkey' : 'password')}

Made ${new Date().toISOString().slice(0, 10)}
`
function downloadKit(text) {
  const a = document.createElement('a')
  a.href = URL.createObjectURL(new Blob([text], { type: 'text/plain;charset=utf-8' }))
  a.download = 'Trommi-Emergency-Kit.txt'
  document.body.append(a); a.click(); a.remove()
  setTimeout(() => URL.revokeObjectURL(a.href), 10000)
}
/** The account's ID with the code that opens the recovery screen with it filled in (AccountView.kit_address). */
const kitIdentity = kit => (kit.account ? html`<div class="set-pair kit-id">${kit.link ? html`<div class="set-qr is-real">${raw(qrSvg(kit.link, 'QR code: opens the recovery screen with this account ID'))}</div>` : ''}<div class="set-text"><b>Account ID</b><small class="room-mono" id="kit-account">${kit.account}</small>${kit.link ? html`<small>The code opens the recovery screen with this ID filled in. It holds no words.</small>` : ''}</div></div>` : '')
/** The kit as a sheet of paper. Hidden, it holds no word at all: twelve drawn strokes. */
const kitSheet = (kit, words) => html`<div class="kit-sheet${words ? ' is-shown' : ''}" id="kit"><p class="kit-head">${BELL}<b>Trommi Emergency Kit</b></p><p class="kit-mail">${kit.email ?? ''}</p>
<ol class="kit-words" id="kit-words"${words ? '' : raw(' aria-hidden="true"')}>${words ? words.map(w => html`<li>${w}</li>`) : KIT_EMPTY.split(' ').map(w => html`<li><i style="--w:${3 + w.length}"></i></li>`)}</ol>
${kitIdentity(kit)}
<p class="kit-how">Lost your way in? app.trommi.com → Log in → Forgot? → your ${kitOpensWith(kit)} and these 12 words.</p></div>`
/** The kit's buttons post one form (/kit/do): shown or hidden, saved as a file, printed, kept. */
const KIT_FORM = (back) => html`<form id="kit-acts" method="post" action="/kit/do" hidden><input type="hidden" name="back" value="${back}"></form>`
const kitButton = (what, label, cls, id, extra = '') => html`<button type="submit" form="kit-acts" name="do" value="${what}" class="${cls}" id="${id}"${raw(extra)}>${label}</button>`
/** What a kit is of: the account as the core read it (AccountView.kit_email, kit_account, kit_address; the ways in
 *  from SettingsView.login); nothing while it is not. */
const kitOf = m => { const av = m?.account ?? {}, login = m?.settings?.login; return { email: av.kit_email ?? login?.email ?? null, account: av.kit_account ?? login?.account ?? null, link: av.kit_address ?? null, has_password: login?.has_password } }

// ---- the screens, each drawn from the core's AccountView and what this page remembers ----
// o: { pk (passkeys offered), way, busy (the busy button's word), passkeyBusy, problem (words), email, loggedOut,
//      note, still, paste, shown, saved, kit (whose kit), words (the kit's), ways ({ password, passkey }), demo }
const SCREENS = {
  start: o => obShell(html`Your agents ring.<br>Answer <span class="ob-pen">anywhere.${UNDER}</span>`, html`${o.loggedOut ? html`<p class="ob-said" id="logged-out" role="status">Logged out.</p>` : ''}${o.problem ? html`<p class="ob-error is-shown" role="alert">${o.problem}</p>` : ''}
<div class="ob-stack">${goButton('/create', 'ob-go', 'way-create', 'Create account')}${goButton('/login', 'ob-second', 'way-login', 'Log in')}${DEMO_WAY}</div>`, { lead: 'Claude Code and Codex ask as cards on your phone. One tap, and they keep working. End-to-end encrypted.', cls: 'ob-welcome', home: false, demo: false, focus: '' }),

  // Passkey first where the browser has passkeys (the email is optional then); the password is the quiet way.
  create: o => {
    const passkey = o.way === 'passkey'
    return obShell('Create account', html`<form id="create-form" class="ob-form" method="post" action="/create" novalidate><input type="hidden" name="way" value="${passkey ? 'passkey' : 'password'}">${o.note ? html`<p class="ob-said" role="status" id="passkey-note">${o.note}</p>` : ''}${obEmail(o.email, passkey)}${passkey ? '' : obPassword({ fresh: true })}
${obError(o.problem)}${obSubmit(passkey ? 'Create with passkey' : 'Create account', o.busy)}</form>
${o.pk ? html`<p class="ob-alt ob-way"><a class="ob-link" id="way-swap" href="/create?way=${passkey ? 'password' : 'passkey'}" data-nav>${passkey ? 'Use a password instead' : 'Use a passkey instead'}</a></p>` : o.pkOff ? '' : NO_PASSKEYS}
<p class="ob-alt">Have an account? <a class="ob-link" id="alt-login" href="/login" data-nav>Log in</a></p>`, { ...(passkey ? { lead: 'No password to remember.' } : {}), ...(o.note ? { focus: '#ob-pw' } : passkey ? { focus: 'button.ob-go' } : {}) })
  },

  // Passkey first where the browser has passkeys: the one filled button; email and password under "or".
  login: o => {
    const pk = o.pk, atKey = o.passkeyBusy || o.at === 'passkey'
    return obShell('Log in', html`${pk ? html`<form method="post" action="/login/passkey"><button type="submit" class="ob-go ob-key${o.passkeyBusy ? ' is-busy' : ''}" id="way-passkey"${o.passkeyBusy ? raw(' disabled') : ''}>${art('key')}<span>${o.passkeyBusy || 'Log in with passkey'}</span></button></form>
${o.problem && atKey ? html`<p class="ob-error ob-key-error" role="alert" id="passkey-error">${o.problem}</p>` : ''}
<p class="ob-or"><span>or</span></p>` : ''}
<form id="login-form" class="ob-form" method="post" action="/login" novalidate>${obAccount(o.email, pk)}${obPassword({ side: html`<a class="ob-link" id="way-forgot" href="/forgot" data-nav>Forgot?</a>` })}
${obError(atKey ? '' : o.problem)}${pk ? obSubmit('Log in with password', o.busy, 'ob-second ob-wide') : obSubmit('Log in', o.busy)}</form>
${pk ? '' : html`<p class="ob-or"><span>or</span></p>`}
<div class="ob-stack ob-ways">${goButton('/scan', 'ob-second ob-wide', 'way-pair', html`${art('phone')}Scan a code`)}</div>
${pk || o.pkOff ? '' : NO_PASSKEYS}
<p class="ob-alt">New here? <a class="ob-link" id="alt-create" href="/create" data-nav>Create account</a></p>`, pk && !(o.problem && !atKey) ? { focus: '#way-passkey' } : {})
  },

  // The Emergency Kit: the one field (email or account ID) + twelve words; then a new password, or (the quiet way
  // where the device can) a new passkey, made once the words opened the account (/new-way).
  forgot: o => {
    const passkey = o.way === 'passkey'
    return obShell(passkey ? 'New passkey' : 'New password', html`<form id="forgot-form" class="ob-form" method="post" action="/forgot" novalidate><input type="hidden" name="way" value="${passkey ? 'passkey' : 'password'}">${o.otherHub ? html`<p class="ob-said" role="status" id="kit-hub">This code leads to an account at <b>${o.otherHub}</b>. Go on only if that is where your account is.</p>` : ''}${obAccount(o.email)}
<label class="ob-field"><span class="ob-label">Twelve words</span><textarea name="words" rows="3" autocomplete="off" autocapitalize="off" spellcheck="false"></textarea></label>
${passkey ? '' : obPassword({ label: 'New password', fresh: true })}
${obError(o.problem)}${obSubmit(passkey ? 'Continue' : 'Set password', o.busy)}</form>
${o.pk ? html`<p class="ob-alt ob-way"><a class="ob-link" id="way-swap" href="/forgot?way=${passkey ? 'password' : 'passkey'}" data-nav>${passkey ? 'Set a password instead' : 'Use a passkey instead'}</a></p>` : ''}
<p class="ob-alt"><a class="ob-link" id="alt-login" href="/login" data-nav>Back to log in</a></p>`, { lead: 'With the words of your Emergency Kit.', ...(o.fromKit ? { focus: 'textarea' } : {}) })
  },

  // The words opened the account and this device is in: the way in for next time (and a new kit with it).
  newway: o => {
    const passkey = o.way === 'passkey'
    return obShell(passkey ? 'New passkey' : 'New password', html`<form id="newway-form" class="ob-form" method="post" action="/new-way" novalidate><input type="hidden" name="way" value="${passkey ? 'passkey' : 'password'}">${o.note ? html`<p class="ob-said" role="status" id="passkey-note">${o.note}</p>` : ''}${passkey ? '' : obPassword({ label: 'New password', fresh: true })}
${obError(o.problem)}${passkey ? obSubmit('Create passkey', o.busy, 'ob-go', 'newway-passkey') : obSubmit('Set password', o.busy)}</form>
${o.pk ? html`<p class="ob-alt ob-way"><a class="ob-link" id="way-swap" href="/new-way?way=${passkey ? 'password' : 'passkey'}" data-nav>${passkey ? 'Set a password instead' : 'Use a passkey instead'}</a></p>` : ''}`, { lead: 'Your account is open. Choose how you log in next time.', home: false })
  },

  // The Emergency Kit's page: with words (shown once after an account was made or recovered) the sheet, hidden until
  // "Show", Download, Print, and "Open Trommi" once it was downloaded, printed or shown. Without: a new kit is made
  // with the way in given again (NewKit).
  kit: o => {
    const ways = o.ways ?? { password: true, passkey: false }
    return obShell('Your Emergency Kit', o.words ? html`${KIT_FORM('/kit')}
<div class="kit-stage"><div class="kit-cover" id="kit-cover" data-controller="kit" data-action="click->kit#flip">${kitSheet(o.kit, o.shown ? o.words : null)}</div>
${kitButton('show', o.shown ? 'Hide' : 'Show', 'ob-chip kit-show', 'kit-show', ` aria-pressed="${o.shown ? 'true' : 'false'}"`)}</div>
<div class="ob-row">${kitButton('download', 'Download', 'ob-second', 'kit-download')}${kitButton('print', 'Print', 'ob-second', 'kit-print')}</div>
<p class="ob-warn" id="kit-warn">${LOST(ways.password ? 'password' : 'passkey')}</p>
${obError(o.problem)}${kitButton('done', 'Open Trommi', 'ob-go', 'kit-done', o.saved ? '' : ' disabled')}
<p class="ob-hint ob-center" id="kit-first"${o.saved ? raw(' hidden') : ''}>Download, print or show it first.</p>`
      : html`<form id="kit-form" class="ob-form" method="post" action="/kit/new" novalidate>${ways.password ? html`<input type="text" name="email" value="${o.kit.email ?? ''}" autocomplete="username" hidden>${obPassword()}` : ''}
${obError(o.problem)}${obSubmit(ways.password ? 'Make kit' : 'Use passkey', o.busy)}</form>${ways.password && ways.passkey ? html`<p class="ob-or"><span>or</span></p><form method="post" action="/kit/new"><input type="hidden" name="by" value="passkey"><button type="submit" class="ob-second ob-wide" id="kit-passkey">${art('key')}Use passkey</button></form>` : ''}`,
    { lead: o.words ? (ways.password ? 'Twelve words that set a new password if you forget yours.' : 'Twelve words that open your account if you lose your passkey.') : ways.password ? 'Enter your password to make it.' : 'Unlock with your passkey to make it.', cls: 'ob-kit', home: false, focus: o.words ? '#kit-show' : ways.password ? '#ob-pw' : '.ob-go', foot: o.words ? '' : html`<a class="ob-link" id="kit-logout" href="/logout" data-nav>Log out</a>` })
  },

  // Scan a code: this device's camera reads the code a signed-in device shows (Settings → Invite a Device); "Paste the
  // link" is the quiet way under it (open by itself when there is no camera). `still`: drawn without a camera.
  scan: o => {
    const state = o.still || 'asking'
    return obShell('Scan a code', html`<div class="scan" id="scan" data-state="${state === 'wrong' ? 'scanning' : state}"${o.still ? '' : raw(' data-controller="scan"')}><video id="scan-video" muted playsinline aria-label="Camera" data-scan-target="video"></video>${o.still ? html`<span class="scan-still">${FAKE_QR}</span>` : ''}${SCAN_CORNERS}
<button type="button" class="ob-chip scan-start" id="scan-start" data-action="scan#begin">Start camera</button></div>
<p class="scan-say" id="scan-say" role="status">${SCAN_SAYS[state]}</p>
<p class="scan-where">On the other device: Settings → Invite a Device</p>
<details class="ob-paste" id="scan-paste"${o.paste || state === 'denied' || state === 'none' ? raw(' open') : ''}><summary>Paste the link</summary>
<form id="paste-form" class="ob-form" method="post" action="/join" novalidate><input name="link" inputmode="url" autocomplete="off" autocapitalize="off" spellcheck="false" aria-label="The link" placeholder="https://app.trommi.com/join#v1…">
${obError(o.problem)}${obSubmit('Next')}</form></details>
<p class="ob-alt"><a class="ob-link" id="alt-login" href="/login" data-nav>Back to log in</a><span${state === 'denied' ? '' : raw(' hidden')}> · <button type="button" class="ob-link" id="scan-again">Try the camera</button></span></p>`, { cls: 'ob-scan', focus: o.paste || state === 'denied' || state === 'none' ? '#scan-paste input' : '' })
  },

  // Joining with a link: asking, then the six emoji to compare with the other device; or what went wrong.
  join: o => (o.problem ? obShell('Not logged in', html`<p class="ob-error is-shown" role="alert">${o.problem}</p>${goButton('/scan', 'ob-go', 'scan-again', 'Scan again')}`, { lead: 'Show a new code on the other device.', focus: '#scan-again' })
    : o.emoji?.length ? obShell('Same six emoji?', html`${emojiRow(o.emoji, 'check-code')}<p class="room-wait">Waiting for the other device…</p>`, { lead: 'If they match, tap “They match” on the other device.', cls: 'ob-emoji', home: false, focus: '' })
      : obShell('Log in', html`<p class="room-wait">Asking the other device…</p>`, { focus: '' })),

  /** (the demo only: a stored account that did not open; the core has no such state for the web yet) */
  broken: o => obShell('Not opened', html`<p class="ob-error is-shown" role="alert" id="broken-why">This browser lost this device's keys.</p>
<div class="ob-stack"><button type="button" class="ob-go" id="broken-logout">Log out</button><button type="button" class="ob-second" id="broken-retry">Retry</button></div>
<p class="ob-hint" id="broken-detail">${o.detail ?? ''}</p>`, { lead: 'Log out here, then log in again. Your account stays.', home: false, focus: '#broken-retry' }),
  joinOffline: () => obShell('Not logged in', html`<p class="ob-error is-shown" role="alert">Trommi is not reachable right now. Check your connection.</p><button type="button" class="ob-go" id="join-again">Try again</button>
<p class="ob-alt">${goButton('/scan', 'ob-link', 'scan-again', 'Scan a new code')}</p>`, { lead: 'The same code still works until it runs out.', focus: '#join-again' }),
}
const TITLES = { start: 'Trommi', create: 'Create account', login: 'Log in', forgot: 'Forgot password', newway: 'New way in', kit: 'Emergency Kit', scan: 'Scan a code', join: 'Log in' }
/** Which steps a screen's problem comes with (a problem of another screen's step is not said here). */
const FAMILY = { start: ['none'], create: ['found', 'none'], login: ['sign_in', 'none'], forgot: ['sign_in', 'recover', 'none'], newway: ['none'], kit: ['none', 'keep_kit'], scan: ['join'], join: ['join'] }
/** A screen's main, in the frame's place (#room-screen: auth.css). */
const screenMain = (name, o) => html`<div id="room-screen">${SCREENS[name](o)}</div>`

/** What a screen needs from the view model and the page's memory. */
function screenOpts(name, vm, url) {
  const av = vm.account ?? {}, step = stepOf(vm), q = url.searchParams
  const pk = passkeysOn()
  const sentHere = local.sent?.path === url.pathname && step === 'working'
  // (a sign-in cut off before its sessions were joined: the log in finishes it, and says why it is asked for)
  const problem = (FAMILY[name]?.includes(step) ? av.problem?.en ?? '' : '') || (name === 'login' ? av.resume?.en ?? '' : '')
  const login = vm.settings?.login
  return {
    pk, pkOff: !pk, way: q.get('way') ?? ((name === 'create' || name === 'newway') && pk ? 'passkey' : 'password'),
    busy: sentHere && !local.sent.passkey ? local.sent.word : '', passkeyBusy: sentHere && local.sent.passkey ? local.sent.word : '',
    at: local.sent?.passkey ? 'passkey' : 'form',
    problem: name === 'scan' ? local.paste || '' : problem, email: local.email, loggedOut: local.loggedOut,
    emoji: av.emoji, words: av.kit?.length ? av.kit : null, shown: local.kit.shown, saved: local.kit.saved,
    kit: kitOf(vm), ways: { password: login?.has_password !== false, passkey: Boolean(login?.passkeys?.length) },
  }
}

// ---- the routes ----
export function register(t) {
  const vm = () => t.vm()
  const page = (req, res, title, main, extra = {}) => t.page(req, res, { title: `${title} · Trommi`, view: 'room', main, ...extra })
  const sent = (path, word, passkey = false) => { local.sent = { path, word, passkey }; local.begun = true; local.loggedOut = false; if (path !== '/forgot' && path !== '/new-way') local.words = null }
  const field = (form, name) => String(form.get(name) ?? '')
  /** The way in a change of the account is proved with: the form's password, else one of the account's passkeys. */
  const unlockOf = form => (form.has('password') ? { by: 'password', password: field(form, 'password') } : form.has('words') ? { by: 'words', words: field(form, 'words') } : { by: 'passkey' })
  /** The account, asked for once in a while (a page that shows it): it comes as vm.settings.login. */
  const readAccount = () => { if (!vm().settings?.login && Date.now() - local.readAt > 10_000 && !signedOut(vm())) { local.readAt = Date.now(); t.account({ step: 'read_account' }) } }

  // ---- the screens before the board ----
  for (const [path, name] of Object.entries(OUT)) t.get(new RegExp(`^${path}$`), ({ req, res, url }) => {
    const m = vm(), step = stepOf(m)
    // A link in the address leaves the address bar before anything else: a join link's secret (/join#v1.…), an
    // Emergency Kit's address (#k1.…).
    const hash = globalThis.location?.hash ?? ''
    if (name === 'join' && /^#v1\./.test(hash)) {
      const link = location.href
      history.replaceState(history.state, '', '/join')
      if (signedOut(m) && step !== 'join') { local.begun = true; t.account({ step: 'join', link }) }
      else if (!signedOut(m)) return page(req, res, 'Pair a device', alreadyIn())
      return t.redirect(res, '/join')
    }
    if (hash.startsWith('#k1.')) { history.replaceState(history.state, '', location.pathname + location.search); return t.redirect(res, '/forgot?way=passkey') }
    const flowing = signedOut(m) || !['none', 'invite'].includes(step) || (name === 'login' && Boolean(m.account?.resume))
    if (!flowing) {
      // (signed in: the words of a recovery wait for the new way in; a screen this page led through leads to the Desk)
      if (name === 'newway' && local.words) return t.page(req, res, { title: 'New way in · Trommi', view: 'account', sidebar: false, main: screenMain('newway', screenOpts('newway', m, url)) })
      if (local.words && name === 'forgot') return t.redirect(res, '/new-way')
      if (!local.begun && (name === 'login' || name === 'join')) return page(req, res, 'Pair a device', alreadyIn())
      return t.redirect(res, '/')
    }
    if (name === 'join' && !['join', 'compare_emoji'].includes(step)) return t.redirect(res, '/scan')
    if (name === 'scan' && !url.searchParams.has('paste')) local.paste = ''
    t.page(req, res, { title: `${TITLES[name]} · Trommi`, view: 'account', sidebar: false, main: screenMain(name, screenOpts(name, m, url)) })
  })
  const alreadyIn = () => roomShell('Already logged in', html`<p class="room-lead">This device is logged in already. Invite another device under <a href="/settings" data-nav>Settings</a>.</p><p class="room-meta">Another account? <a href="/logout" data-nav id="login-logout-first">Log out of this device first</a>, then log in.</p><p class="room-meta"><a href="/" data-nav>Open your Desk</a></p>`)

  t.post(/^\/create$/, ({ res, form }) => {
    const email = field(form, 'email').trim()
    local.email = email
    if (field(form, 'way') === 'passkey') { sent('/create', 'Waiting for passkey…'); t.account({ step: 'found_with_passkey', ...(email ? { email } : {}) }) }
    else { sent('/create', 'Creating…'); t.account({ step: 'found', email, password: field(form, 'password') }) }
    local.kitFrom = 'start'
    t.done(res)
  })
  t.post(/^\/login$/, ({ res, form }) => {
    local.email = field(form, 'account').trim()
    sent('/login', 'Logging in…')
    t.account({ step: 'sign_in', email: local.email, password: field(form, 'password') })
    t.done(res)
  })
  t.post(/^\/login\/passkey$/, ({ res }) => { sent('/login', 'Waiting for passkey…', true); t.account({ step: 'sign_in_with_passkey' }); t.done(res) })
  t.post(/^\/forgot$/, ({ res, form }) => {
    local.email = field(form, 'account').trim()
    const words = field(form, 'words')
    if (field(form, 'way') === 'passkey') {
      // (the words open the account here; the new passkey is made with them on /new-way)
      sent('/forgot', 'Opening…'); local.words = words
      t.account({ step: 'sign_in_with_kit', email: local.email, words })
    } else {
      sent('/forgot', 'Setting…'); local.kitFrom = 'start'
      t.account({ step: 'recover', email: local.email, words, password: field(form, 'password') })
    }
    t.done(res)
  })
  t.post(/^\/new-way$/, ({ res, form }) => {
    const words = local.words ?? ''
    local.kitFrom = 'start'
    if (field(form, 'way') === 'password') { sent('/new-way', 'Setting…'); t.account({ step: 'set_password', unlock: { by: 'words', words }, next: field(form, 'password') }) }
    else { sent('/new-way', 'Waiting for passkey…'); t.account({ step: 'add_passkey', unlock: { by: 'words', words } }) }
    t.done(res)
  })
  // A pasted link (Scan a code), or one read by the camera (controller "scan"): joined with as it is.
  t.post(/^\/join$/, ({ res, form }) => {
    const link = field(form, 'link').trim()
    if (!joinHash(link)) { local.paste = 'That is not a Trommi link.'; return t.redirect(res, '/scan?paste=1') }
    local.paste = ''; local.begun = true
    t.account({ step: 'join', link })
    t.redirect(res, '/join')
  })
  // The kit's buttons. Its words are the core's (AccountView.kit): read here for the file, never kept.
  t.post(/^\/kit\/do$/, ({ res, form }) => {
    const back = field(form, 'back') || kitPath(), words = vm().account?.kit ?? []
    switch (field(form, 'do')) {
      case 'show': local.kit.shown = !local.kit.shown; if (local.kit.shown) local.kit.saved = true; break
      case 'download': if (words.length && !isDemo()) downloadKit(kitText(kitOf(vm()), words)); local.kit.saved = true; break
      case 'print': {
        const was = local.kit.shown
        local.kit.shown = local.kit.saved = true
        // (the printed sheet carries the words: shown for the print, hidden again after it if they were hidden)
        setTimeout(() => { if (!was) addEventListener('afterprint', () => { local.kit.shown = false; window.Turbo?.visit(back, { action: 'replace' }) }, { once: true }); window.print() }, 300)
        break
      }
      case 'done': local.kit = { shown: false, saved: false }; local.words = null; t.account({ step: 'kit_kept' }); return t.done(res)
    }
    t.redirect(res, back)
  })
  t.post(/^\/kit\/new$/, ({ res, form }) => { sent('/kit', 'Making…'); local.kitFrom = 'start'; t.account({ step: 'new_kit', unlock: form.get('by') === 'passkey' ? { by: 'passkey' } : unlockOf(form) }); t.done(res) })

  // ---- /logout: asks once, then SignOut (the core wipes what this device stored) ----
  const logoutMain = () => {
    const l = vm().settings?.logout ?? {}
    return roomShell(l.title?.en ?? '', html`<p class="room-lead" id="logout-ask">${l.ask?.en ?? ''}</p>
${l.note ? html`<p class="room-lead" id="logout-last">${l.note.en}</p>` : ''}<p class="room-meta">${l.meta?.en ?? ''}</p>
<form method="post" action="/logout" class="room-inline room-logout" id="logout-form"><button type="submit" class="room-danger" id="logout-go">${l.go?.en ?? ''}</button><a href="/" data-nav class="room-back" id="logout-cancel">${l.cancel?.en ?? ''}</a></form>`)
  }
  t.get(/^\/logout$/, ({ req, res }) => { readAccount(); page(req, res, vm().settings?.logout?.title?.en ?? '', logoutMain(), { view: 'logout' }) })
  t.post(/^\/logout$/, ({ res }) => {
    // (the demo has no account: leaving it is all there is to do)
    if (isDemo()) { try { sessionStorage.removeItem('trommi-mock') } catch {} location.replace('/'); return t.done(res) }
    local.loggedOut = true; local.begun = false
    t.account({ step: 'sign_out' })
    t.done(res)
  })

  // ---- /settings: the list. At its top the two invites, each drawn blurred until it is asked for; a device's code,
  // its emoji and "They match" then stand in place (vm.account while this page asked for it). ----
  const SHOWN = { light: 'Light', dark: 'Dark', system: 'System' }
  const TERMINAL = html`<span class="set-term-bar" aria-hidden="true"><i></i><i></i><i></i></span><span class="set-term-code" aria-hidden="true"><span>$ claude</span><span>&gt; join trommi ab3f…9c2e</span><span>&gt; check the six emoji</span></span>`
  const inviteTiles = () => html`<section class="set-invite-tiles" aria-label="Invite">
<form method="post" action="/pair" class="set-tile"><input type="hidden" name="role" value="human"><button type="submit" class="set-tile-art set-qr" id="settings-pair" aria-label="Show Code: the code to invite a device"><span class="set-qr-code">${FAKE_QR}</span><span class="set-qr-show">Show Code</span></button><span class="set-text"><b>Invite a Device</b><small>A phone or another computer scans the code. Made when you ask, works once.</small></span></form>
<form method="post" action="/pair" class="set-tile set-agent"><input type="hidden" name="role" value="computer"><button type="submit" class="set-tile-art set-term" id="settings-invite-agent" aria-label="Show Command: the command to invite an agent"><span class="set-term-in">${TERMINAL}</span><span class="set-qr-show">Show Command</span></button><span class="set-text"><b>Approve a Computer</b><small>Approve it once; activate each agent session with one command.</small></span></form>
</section>`
  const matchButtons = (inviteId, back) => html`<div class="check-answer">${['yes', 'no'].map(v => html`<form method="post" action="/pair/confirm"><input type="hidden" name="invite_id" value="${inviteId}"><input type="hidden" name="in" value="${back}"><input type="hidden" name="match" value="${v}"><button type="submit" class="${v === 'yes' ? 'room-primary check-yes' : 'check-no'}">${v === 'yes' ? 'They match' : 'They don\'t match'}</button></form>`)}</div>`
  const copyBox = (value, label) => html`<div class="room-link" data-controller="room"><input readonly value="${value}" aria-label="${label}" data-room-target="field" data-action="focus->room#select"><button type="button" data-action="room#copy" data-room-text-param="${value}"><span data-room-target="label">Copy</span></button></div>`
  /** Where this device's invite of that kind stands (vm.account.invite): 'open', 'confirm_code', 'adding', 'joined';
   *  'failed' when the invite asked for here could not be made or ended; else 'none'. */
  const inviteState = (kind) => {
    const m = vm(), av = m.account ?? {}, inv = av.invite
    if (inv && inv.role === (kind === 'agent' ? 'agent' : 'human')) return inv.state === 'open' && !av.link ? 'none' : inv.state
    return stepOf(m) === 'invite' && av.problem && local.invite?.kind === kind ? 'failed' : 'none'
  }
  /** What is left of an open link's time ("12 more min."). */
  const leftWords = until => { const ms = until - Date.now(); return ms > 90_000 ? `${Math.round(ms / 60000)} more min.` : ms > 0 ? 'less than a minute.' : 'no time left.' }
  /** "Done" (a joined invite) and "Cancel" (an open one) end the invite: it leaves the view, an open one is burned. */
  const dropButton = (inv, word, cls, id) => html`<form method="post" action="/pair/drop" class="room-inline"><input type="hidden" name="invite_id" value="${inv.invite_id}"><button type="submit" class="${cls}" id="${id}">${word}</button></form>`
  const deviceInvite = () => {
    const av = vm().account ?? {}, state = inviteState('device'), inv = av.invite ?? {}
    const head = (words, line) => html`<div class="set-text"><b>${words}</b>${line ? html`<small>${line}</small>` : ''}</div>`
    const fresh = (word = 'Show Code', line = 'A phone or another computer scans the code. It is made when you ask for it and works once.') => html`<form method="post" action="/pair" class="set-pair"><input type="hidden" name="role" value="human"><button type="submit" class="set-qr" id="settings-pair" aria-label="${word}: the code to invite a device"><span class="set-qr-code">${FAKE_QR}</span><span class="set-qr-show">${word}</span></button>${head('Invite a Device', line)}</form>`
    let inner
    if (state === 'open') inner = html`<div class="set-pair"><div class="set-qr is-real">${raw(qrSvg(av.link, 'QR code to invite a device'))}</div><div class="set-text"><b>Invite a Device</b><small>On the new device, open the camera and scan the code. Both devices then show six emoji.</small><p class="room-wait">Waiting for the new device… The code works once, ${leftWords(inv.expires_ms)}</p><details class="room-more"><summary>No scanner? Send the link</summary><p class="room-meta">Send it to yourself and open it on the new device. The secret is after the #; it never reaches a server.</p>${copyBox(av.link, 'Invite link')}</details></div></div>`
    else if (state === 'confirm_code') inner = html`${head('A device wants to join', 'Does it show these six emoji, in this order?')}${emojiRow(av.emoji)}${matchButtons(av.invite_id, 'settings')}<small class="set-note">"They don't match" burns the code: nobody is added.</small>`
    else if (state === 'adding') inner = html`${head('Invite a Device')}<p class="room-wait">Adding ${inv.newcomer || 'the device'}…</p>`
    else if (state === 'joined') inner = html`<div class="set-pair"><span class="set-in">${sk('tick')}</span>${head(`${inv.newcomer || 'The new device'} is in now.`, 'It holds keys of its own; you see it under Devices.')}${dropButton(inv, 'Done', 'set-pill', 'settings-pair-done')}</div>`
    else if (state === 'failed') inner = html`${errorLine(av.problem.en)}${fresh('Show a New Code', '')}`
    else return null
    return html`<div class="set-device" id="set-device" data-state="${state}">${inner}</div>`
  }
  /** Sealed dictation on this device: the switch, and the enclave's state as the core says it (a dot and a line). */
  const dictationRow = d => (!d?.available ? '' : html`<label class="set-row set-switch" id="settings-dictation" data-controller="set-dictation"><span class="set-ico">${sk('mic')}</span><b>Dictation</b><span class="set-detail"><i class="set-dot is-${d.status_kind || 'unknown'}" aria-hidden="true"></i>${d.status?.en ?? ''}</span><input type="checkbox" role="switch" aria-label="Dictation"${d.on ? raw(' checked') : ''} data-action="change->set-dictation#flip"><span class="set-knob" aria-hidden="true"></span></label>`)
  const homeMain = () => {
    const s = vm().settings ?? {}, invite = deviceInvite()
    return settingsPage('Settings', html`${!invite ? inviteTiles() : html`<section class="set-group set-invites" aria-label="Invite">${invite}
<form method="post" action="/pair" class="set-agent"><input type="hidden" name="role" value="computer"><button type="submit" class="set-row" id="settings-invite-agent"><span class="set-ico">${PLUS}</span><b>Approve Computer…</b><span class="set-detail">a command for Claude Code</span>${SET_CHEVRON}</button></form></section>`}
<nav class="set-group" aria-label="Settings">
${setRow({ href: '/settings/agents', icon: sk('terminal'), word: 'Agents', detail: s.agents?.en ?? '', id: 'settings-agents' })}
${setRow({ href: '/settings/teams', icon: sk('desk'), word: 'Teams', detail: s.teams?.en ?? '', id: 'settings-teams' })}
${setRow({ href: '/settings/devices', icon: art('phone'), word: 'Devices', detail: s.devices_says?.en ?? '', id: 'settings-devices' })}
${setRow({ href: '/settings/account', icon: sk('key'), word: 'Account', detail: s.account ?? '', id: 'settings-account' })}
</nav>
<nav class="set-group" aria-label="This device">
${setRow({ href: '/settings/theme', icon: sk('moon'), word: 'Theme', detail: SHOWN[themeMode()] ?? 'System', id: 'settings-theme' })}
${dictationRow(vm().dictation)}
${setRow({ href: '/settings/keys', icon: sk('keycap'), word: 'Keyboard Shortcuts', id: 'settings-keys' })}
</nav>
<nav class="set-group" aria-label="About">
${setRow({ href: '/settings/security', icon: sk('lock'), word: 'Security', detail: 'keys, devices, self test', id: 'settings-security' })}
</nav>
`, { back: false })
  }
  t.get(/^\/settings$/, ({ req, res, url }) => {
    readAccount()
    page(req, res, 'Settings', homeMain(), { view: 'settings' })
  })
  t.post(/^\/pair$/, ({ res, form }) => {
    if (['agent', 'computer'].includes(String(form.get('role')))) {
      const team = t.team()
      local.invite = { kind: 'agent' }
      if (form.get('role') === 'computer') t.account({ step: 'approve_computer', label: field(form, 'label').trim() })
      else t.account({ step: 'invite_agent', label: field(form, 'label').trim(), ...(team ? { team } : {}) })
      return t.redirect(res, '/pair/agent')
    }
    local.invite = { kind: 'device' }
    t.account({ step: 'invite_device' })
    t.redirect(res, '/settings')
  })
  t.post(/^\/pair\/confirm$/, ({ res, form }) => {
    t.account({ step: 'confirm_emoji', invite_id: field(form, 'invite_id'), matches: form.get('match') === 'yes' })
    t.redirect(res, form.get('in') === 'settings' ? '/settings' : '/pair/agent')
  })
  t.post(/^\/pair\/drop$/, ({ res, form }) => {
    const back = vm().account?.invite?.role === 'agent' ? '/' : '/settings'
    t.account({ step: 'drop_invite', invite_id: field(form, 'invite_id') })
    local.invite = null
    t.redirect(res, back)
  })
  // ---- /pair/agent: inviting an agent, a clipboard with a short checklist ----
  // Two lines, each one big thing to press (it copies): install the connector (once per machine), then a plain prompt
  // with the link pasted into the agent in the project folder; then the emoji to compare, and the agent is in.
  const CLAMP = raw('<svg class="clip-clamp" viewBox="0 0 120 44" aria-hidden="true"><path class="clamp-plate" d="M22 40 Q21 25 26 22 L43 21 Q46 9 60 8 Q74 9 77 21 L94 22 Q99 25 98 40 Z"/><path d="M52 21 Q53 15 60 14.6 Q67 15 68 21"/><path d="M30 31 Q60 29.4 90 31"/></svg>')
  const TICKBOX = raw('<svg class="clip-box" viewBox="0 0 24 24" aria-hidden="true"><path d="M4.6 5.2 Q12 4.4 19.3 4.9 Q20 12 19.5 19.2 Q12 20 4.9 19.4 Q4.2 12 4.6 5.2 Z"/><path class="clip-tick" d="M7.4 12.6 Q9.6 14.6 10.9 16.6 Q14.6 10.2 20.6 5.2"/></svg>')
  // (a line breaks after a slash, after the scheme, after a short first host label: a phone never breaks inside a word)
  const wrapAt = text => text.split(/(?<=[^/:]\/|:\/\/|\/\/[^/.]{1,12}\.)/).map((part, i) => html`${i ? raw('<wbr>') : ''}${part}`)
  const copyLine = (text, word, line, shown = wrapAt(text)) => html`<button type="button" class="clip-copy" data-line="${line}" data-action="invite-clip#copy" data-invite-clip-text-param="${text}" title="Copy"><code>${shown}</code><span class="clip-copy-word" data-word="${word}">${word}</span></button>`
  /** The connect command as shown: the part every invite of an account shares folded into an ellipsis. */
  const connectShown = command => {
    const at = command.indexOf('#'), parts = at < 0 ? [] : command.slice(at + 1).split('.')
    if (parts.length < 5) return wrapAt(command)
    const head = command.slice(0, at + 1) + parts[0] + '.', mid = `${parts[1]}.${parts[2]}.`, tail = parts.slice(3).join('.')
    return html`${wrapAt(head)}<span class="clip-link-same" data-same="…">${mid}</span><wbr><span class="clip-link-own">${tail}</span>`
  }
  const step = (state, inner) => html`<li class="clip-step${state ? ` is-${state}` : ''}">${TICKBOX}<div class="clip-step-body">${inner}</div></li>`
  const altLine = (text, line = 'slash', shown = text.replace(/#.*'$/, "#…'")) => html`<button type="button" class="clip-alt" data-line="${line}" data-action="invite-clip#copy" data-invite-clip-text-param="${text}" title="Copy"><code>${shown}</code><span class="clip-copy-word" data-word="Copy">Copy</span></button>`
  /** A command as shown: its --flags kept whole, its link broken after a slash. */
  const commandShown = text => text.split(' ').map((word, i) => html`${i ? ' ' : ''}${word.startsWith('--') ? html`<span class="clip-nb">${word}</span>` : wrapAt(word)}`)
  // (the ways in, one tab each: Claude Code, Codex, and for a computer Hermes; the core's AccountView.ways)
  const wayTabs = (ways, on) => ways.length < 2 ? '' : html`<nav class="clip-ways" aria-label="Which agent">${ways.map(w => html`<a href="/pair/agent?with=${w.key}" data-nav data-way="${w.key}"${w.key === on.key ? raw(' aria-current="page"') : ''}>${w.label.en}</a>`)}</nav>`
  const nameStep = (n, done) => step(done, html`<b>${n.title.en}</b><form method="post" action="/pair/hermes" class="clip-name"><input name="name" value="${n.value}" placeholder="${n.placeholder.en}" maxlength="40" autocomplete="off" spellcheck="false" aria-label="${n.title.en}" data-action="input->invite-clip#typed change->invite-clip#name"></form><small>${n.note.en}</small>`)
  const clipboard = (wayKey) => {
    const av = vm().account ?? {}, state = inviteState('agent'), inv = av.invite ?? {}
    const ways = av.ways ?? [], way = ways.find(w => w.key === wayKey) ?? ways[0]
    if (!inv.invite_id && !av.problem) return html`<main id="room" class="room room-clip" aria-busy="true"><section class="clip">${CLAMP}<progress></progress></section></main>`
    const open = state === 'open', joined = state === 'joined', coming = state === 'adding', ask = state === 'confirm_code', dead = state === 'failed'
    const done = open ? '' : 'done'
    const last = ask ? html`<div class="clip-ask" role="group" aria-label="Confirm the agent"><b>An agent wants to join.</b>
<small>Its terminal shows six emoji, each with a word. Are they these, in this order?</small>
${emojiRow(av.emoji)}
${matchButtons(av.invite_id, 'agent')}
<small>"They don't match" burns the link: nobody is added.</small></div>`
      : joined ? html`<b class="clip-in"><span>${inv.newcomer || 'The agent'} is in</span></b>`
      : coming ? html`<b>Adding ${inv.newcomer || 'the agent'}…</b>`
      : dead ? html`<b>${av.problem.en}</b>`
      : html`<b>${way?.compare.en ?? 'Compare the six emoji with the ones here'}</b><small>${way?.compare_note.en ?? ''}</small>`
    const foot = joined ? dropButton(inv, 'Done', 'room-done clip-done', 'clip-done')
      : dead ? html`<form method="post" action="/pair" class="clip-again"><input type="hidden" name="role" value="computer"><button type="submit">New link</button></form>`
      : coming ? html`<p class="clip-note">The link is in use: this page stays until the agent is in.</p>`
      : html`<p class="clip-note">The link works once · ${leftWords(inv.expires_ms)}</p>`
    return html`<main id="room" class="room room-clip" aria-label="${inv.title?.en ?? ''}"><div id="invite-agent" class="room-invite" data-state="${state}">
<section class="clip" data-controller="invite-clip">${CLAMP}
<h2>${inv.title?.en ?? ''}</h2><p class="clip-sub">${inv.notice?.en ?? ''}</p>
${open ? wayTabs(ways, way) : ''}
<ol class="clip-list" data-way="${way?.key ?? ''}">
${way?.install_step ? step(done, html`<b>${way.install_title.en}</b>${open ? html`${copyLine(way.install, 'Copy', 'install')}<small>Skip this if you have installed Trommi before (check: <code>trommi-connector --version</code>).</small>` : ''}`) : ''}
${open && way?.name ? nameStep(way.name, way.name.value ? 'done' : '') : ''}
${way ? step(done, html`<b>${way.paste.en}</b>${open ? html`${copyLine(way.line, 'Copy', way.key === 'hermes' ? 'setup' : 'connect', way.key === 'hermes' ? commandShown(way.line) : connectShown(way.line))}${way.alt ? html`<small class="clip-or">Or in Claude Code, the slash command: ${altLine(way.alt)}</small>` : ''}<small>${way.note.en}</small>${way.install_step ? '' : html`<small class="clip-or">${way.install_title.en} ${altLine(way.install, 'install', way.install.replace(/https:\/\/\S*\//, '…/'))}</small>`}` : ''}`) : ''}
${step(joined ? 'done' : dead ? 'dead' : ask ? 'ask' : open ? '' : 'wait', last)}
</ol>${foot}</section></div></main>`
  }
  t.get(/^\/pair\/agent$/, ({ req, res, url }) => {
    if (inviteState('agent') === 'none' && local.invite?.kind !== 'agent') return t.redirect(res, '/settings')
    page(req, res, vm().account?.invite?.title?.en ?? '', clipboard(url.searchParams.get('with')), { view: 'invite' })
  })
  // (the Hermes tab's name, without a script: Enter sends it; with one, the field sends it as typed)
  t.post(/^\/pair\/hermes$/, ({ res, form }) => {
    t.account({ step: 'hermes_name', name: field(form, 'name') })
    t.redirect(res, '/pair/agent?with=hermes')
  })

  // ---- /settings/devices: the person's devices (phones, tablets, browsers), each with a page of its own, and Push.
  // The agents are Settings · Agents (agents.mjs). ----
  const devIcon = d => (d.icon === 'terminal' ? sk('terminal') : d.icon === 'phone' ? art('phone') : sk('laptop'))
  const onlineDot = on => html`<i class="set-dot" data-online="${on ? 'yes' : 'no'}"></i>`
  const devLine = d => html`<span class="set-ico">${devIcon(d)}</span><span class="set-two"><b>${d.name}</b><small class="room-fp" title="Key fingerprint">${d.fingerprint}</small>${d.seen ? html`<small>${d.seen.en}</small>` : ''}</span><span class="set-detail">${d.active ? onlineDot(d.online) : ''}${d.says.en}</span>`
  // (.room-device: one per device listed, as the end-to-end runs count them)
  const member = d => (d.active
    ? html`<a class="set-row set-row-two room-device" id="device-${d.id}" href="/settings/devices/${d.id}" data-nav>${devLine(d)}${SET_CHEVRON}</a>`
    : html`<div class="set-row set-row-two set-fact room-device is-removed" id="device-${d.id}">${devLine(d)}<span></span></div>`)
  const devicesMain = () => {
    const s = vm().settings ?? {}, push = s.push, gone = s.removed ?? [], stuck = s.unfinished ?? []
    return roomPage('Devices', html`<h3 class="set-label" id="people-head">Your devices</h3><div class="set-group" id="room-people">${(s.devices ?? []).map(member)}</div>${stuck.length ? html`<form method="post" action="/devices/remove" class="set-group" id="devices-unfinished">${stuck.map(id => html`<input type="hidden" name="device_id" value="${id}">`)}<button type="submit" class="set-row set-danger"><span class="set-ico">${sk('logout')}</span><b>${s.remove_unfinished?.en ?? ''}</b><span class="set-detail"></span><span></span></button></form><p class="set-note" id="devices-unfinished-note">A login that hung or was given up leaves a device behind that never came. While it is there, new sessions can't be opened.</p>` : ''}
${push ? html`<h3 class="set-label" id="push-head">Push on this device</h3><form method="post" action="/settings/push" data-controller="set-push"><fieldset class="set-group set-choice set-push" id="push-level" aria-labelledby="push-head"${push.busy ? raw(' disabled') : ''}>${(s.push_choices ?? []).map(c => html`<label class="set-row"><input type="radio" name="level" value="${c.level}" data-action="change->set-push#pick"${push.level === c.level ? raw(' checked') : ''}><span class="set-ico">${sk(c.icon)}</span><b>${c.title.en}</b><span class="set-detail">${c.detail?.en ?? ''}</span>${sk('tick')}</label>`)}</fieldset></form>
<p class="push-level-note" id="push-level-note" role="status">${push.level === 'off' ? pushObstacle() : ''}</p>` : ''}
${gone.length ? html`<div class="set-group" id="room-gone">${setRow({ href: '/settings/devices/removed', icon: sk('archive'), word: 'Removed', detail: String(gone.length), id: 'devices-removed' })}</div>` : html`<div id="room-gone" hidden></div>`}
<p class="set-note">A new device: Invite a Device in <a href="/settings" data-nav>Settings</a>, or open app.trommi.com on it and log in. Every device holds its own keys; the hub sees sealed envelopes only. Your agents are under <a href="/settings/agents" data-nav>Agents</a>.</p>`, 'The phones, tablets and browsers where you read and answer.')
  }
  const deviceMain = d => settingsPage(d.name, html`<div class="set-group">
<div class="set-row set-fact"><span class="set-ico">${devIcon(d)}</span><b>Kind</b><span class="set-detail">${d.role === 'human' ? 'Person' : 'Agent'}</span><span></span></div>
<div class="set-row set-fact"><span class="set-ico">${onlineDot(d.active && d.online)}</span><b>Status</b><span class="set-detail" id="device-state">${d.says.en}</span><span></span></div>
<div class="set-row set-fact set-fp-row"><span class="set-ico">${sk('key')}</span><b>Key fingerprint</b><span class="set-detail room-mono" id="device-fp">${d.fingerprint}</span><span></span></div></div>
<p class="set-note">The fingerprint comes from the signed member list: it must look the same on every device.</p>
${d.this_device && d.active ? html`<h3 class="set-label">Name</h3><form method="post" action="/devices/rename" class="set-group set-form" id="device-rename"><label class="set-field"><span>Name of this device</span><input name="device_name" value="${d.name}" maxlength="40" required autocomplete="off"></label><button type="submit" class="room-primary">Rename</button></form><p class="set-note">A device names itself; this name is what your other devices see.</p>` : ''}
${d.active && !d.this_device ? html`<form method="post" action="/devices/remove" class="set-group" id="device-remove"><input type="hidden" name="device_id" value="${d.id}"><button type="submit" class="set-row set-danger"><span class="set-ico">${sk('logout')}</span><b>Remove ${d.name}</b><span class="set-detail"></span><span></span></button></form><p class="set-note">${d.name} can open nothing new after this. Everyone else gets a new key; that takes a moment.</p>` : ''}`,
  { lead: d.this_device ? 'This device' : !d.active ? 'Removed' : 'One of your devices', back: '/settings/devices', backWord: 'Devices' })
  // After a device not in the person's hands was removed: a new recovery code, the way in given kept (8.6).
  const removedMain = () => {
    const login = vm().settings?.login
    return settingsPage('Removed', html`<div class="set-group">${(vm().settings?.removed ?? []).map(member)}</div><p class="set-note">They open nothing new; what they had before stays with them.</p>
${login ? html`<section id="code-new"><h3 class="set-label">Not in your hands?</h3>${form('/settings/recovery-code', html`<div class="set-group set-proof-g">${proofField(login)}</div>`, 'Replace the Recovery Code', 'code-form')}<p class="set-note">A new Emergency Kit replaces the old one; your other ways in except this one are removed.</p></section>` : ''}`, { back: '/settings/devices', backWord: 'Devices' })
  }
  t.get(/^\/settings\/devices$/, ({ req, res }) => page(req, res, 'Devices · Settings', devicesMain(), { view: 'settings' }))
  t.get(/^\/settings\/devices\/removed$/, ({ req, res }) => { readAccount(); page(req, res, 'Removed · Devices', removedMain(), { view: 'settings' }) })
  t.get(/^\/settings\/devices\/([^/]+)$/, ({ req, res, match }) => {
    const id = decodeURIComponent(match[1]), d = [...(vm().settings?.devices ?? []), ...(vm().settings?.removed ?? [])].find(x => x.id === id)
    if (!d) return t.redirect(res, '/settings/devices')
    page(req, res, `${d.name} · Devices`, deviceMain(d), { view: 'settings' })
  })
  t.post(/^\/devices\/remove$/, ({ res, form }) => { t.account({ step: 'remove_devices', device_ids: form.getAll('device_id').map(String) }); t.redirect(res, '/settings/devices') })
  t.post(/^\/devices\/rename$/, ({ res, form }) => { const name = field(form, 'device_name').trim().slice(0, 40); if (name) t.act({ do: 'rename_device', name }); t.redirect(res, '/settings/devices') })
  t.post(/^\/settings\/push$/, ({ res, form }) => { t.act({ do: 'push', level: field(form, 'level') }); t.done(res) })

  // ---- /settings/theme, /settings/keys: this device's own ----
  t.get(/^\/settings\/theme$/, ({ req, res }) => page(req, res, 'Theme · Settings', settingsPage('Theme', html`<div class="set-group set-choice" role="radiogroup" aria-label="Theme" data-controller="set-theme">${['light', 'dark', 'system'].map(v => html`<label class="set-row"><input type="radio" name="theme" value="${v}" data-action="change->set-theme#pick"${themeMode() === v ? raw(' checked') : ''}><span class="set-ico">${sk(v === 'light' ? 'sun' : v === 'dark' ? 'moon' : 'grid')}</span><b>${SHOWN[v]}</b>${sk('tick')}</label>`)}</div>
<p class="set-note">System follows this device's own setting. The key T switches anywhere.</p>`, { lead: 'On this device.' }), { view: 'settings-theme' }))
  t.get(/^\/settings\/keys$/, ({ req, res }) => page(req, res, 'Keyboard Shortcuts · Settings', settingsPage('Keyboard Shortcuts', html`<div class="set-group set-keys-group">${keysList()}</div><p class="set-note">More keys later. Keys rest while you type in a field. <a href="/help.html#keys">On the help page</a></p>`), { view: 'settings-keys' }))

  // ---- /settings/security: what opens the account's data, and the self test ----
  /** Dictation's sealed connection, as the core says it (DictationView.facts, in order), and its test. */
  const FACT_ICON = { enclave: 'lock', model: 'mic', attestation: 'tick', code: 'key', hub: 'terminal' }
  const dictationFacts = d => {
    const facts = Array.isArray(d?.facts) ? d.facts : []
    if (!d?.available || !d.on || !facts.length) return ''
    const value = f => html`${f.dot ? html`<i class="set-dot is-${f.dot}" aria-hidden="true"></i>` : ''}${f.href ? html`<a class="dl-code" href="${f.href}" target="_blank" rel="noopener">${f.value?.en ?? ''} ↗</a>` : f.value?.en ?? ''}`
    const running = d.test_state === 'running'
    return html`<h3 class="set-label">Dictation</h3><div class="set-group" id="security-dictation">
${facts.map(f => html`<div class="set-row set-fact" id="dictation-fact-${f.key}"><span class="set-ico">${sk(FACT_ICON[f.key] ?? 'lock')}</span><b>${f.label?.en ?? ''}</b><span class="set-detail">${value(f)}</span><span></span></div>`)}
<form method="post" action="/settings/dictation/test" class="dl-form"><button type="submit" class="set-row dl-run" id="dictation-test"${running ? raw(' disabled') : ''}><span class="set-ico">${sk('mic')}</span><b>Test the sealed connection</b><span class="set-detail${d.test_state === 'passed' ? ' dl-ok' : d.test_state === 'failed' ? ' dl-bad' : ''}" id="dictation-test-result">${d.test_result?.en ?? 'a second of silence'}</span><span></span></button></form></div>`
  }
  const securityMain = () => {
    const s = vm().settings ?? {}, sec = s.security ?? {}
    const ICONS = { key: () => sk('key'), page: () => sk('page'), terminal: () => sk('terminal'), tick: () => sk('tick'), phone: () => art('phone'), laptop: () => sk('laptop') }
    const hrefOf = r => ({ kit: '/settings/account/kit', agents: '/settings/agents', proof: '/settings/proof', device: `/settings/devices/${r.id}` })[r.open]
    const ROW_IDS = { key: 'security-key', kit: 'security-kit', agents: 'security-agents', proof: 'settings-proof' }
    const row = (sec, r) => {
      const id = sec === 'devices' ? `security-device-${r.id.slice(0, 16)}` : ROW_IDS[r.id] ?? `security-${r.id}`, href = hrefOf(r)
      const second = r.fingerprint || r.detail ? html`<small>${r.fingerprint ? html`<span class="room-mono"${r.id === 'key' ? raw(' id="security-key-fp"') : ''}>${r.fingerprint}</span>` : ''}${r.fingerprint && r.detail ? ' · ' : ''}${r.detail?.en ?? ''}</small>` : ''
      const inner = html`<span class="set-ico">${(ICONS[r.icon] ?? ICONS.laptop)()}</span><span class="set-two"><b>${r.title.en}${r.mark ? html` <span class="set-small set-inline">· ${r.mark.en}</span>` : ''}</b>${second}</span><span class="set-detail">${r.says?.en ?? ''}</span>`
      return href ? html`<a class="set-row${second ? ' set-row-two' : ''}${sec === 'devices' ? ' security-device' : ''}" id="${id}" href="${href}" data-nav>${inner}${SET_CHEVRON}</a>`
        : html`<div class="set-row set-row-two set-fact" id="${id}">${inner}<span></span></div>`
    }
    const NOTE_IDS = { key: 'security-key-note' }
    const group = g => html`<h3 class="set-label">${g.label.en}</h3><div class="set-group"${g.id === 'devices' ? raw(' id="security-devices"') : ''}>${(g.rows ?? []).map(r => row(g.id, r))}</div>
${(g.notes ?? []).map(n => html`<p class="set-note"${NOTE_IDS[g.id] ? raw(` id="${NOTE_IDS[g.id]}"`) : ''}>${n.en}</p>`)}${g.strong ? html`<p class="set-note set-note-strong" id="security-hub">${g.strong.en}</p>` : ''}`
    return settingsPage('Security', html`${(sec.sections ?? []).map(group)}${dictationFacts(vm().dictation)}`, { lead: sec.lead?.en ?? '' })
  }
  t.get(/^\/settings\/security$/, ({ req, res }) => page(req, res, 'Security · Settings', securityMain(), { view: 'settings-security' }))
  t.post(/^\/settings\/dictation\/test$/, ({ res }) => { t.act({ do: 'dictate', step: { step: 'test' } }); t.done(res) })

  // ---- /settings/account: the account's ways in (each a page: password, passkeys, kit; email where there is none).
  // A change asks for the way in of now once more (proofField); its answer is a toast, a problem, or a new kit. ----
  const SUBS = { password: 'Password', passkeys: 'Passkeys', kit: 'Emergency Kit', email: 'Email', delete: 'Delete Account' }
  const sublead = { password: 'Opens your account with your email.', passkeys: 'Each opens your account without a password.', kit: 'Twelve words for the day you lose your way in.', email: 'A second name for your account.', delete: 'The account, its rooms and everything in them.' }
  const accountMain = sub => {
    const m = vm(), st = m.settings?.login, av = m.account ?? {}, step = stepOf(m)
    const busy = step === 'working'
    const top = html`${step === 'none' && av.problem ? errorLine(av.problem.en) : ''}${busy ? html`<p class="room-wait" role="status">Working…</p>` : ''}`
    const subPage = inner => settingsPage(SUBS[sub], html`${top}${inner}`, { back: '/settings/account', backWord: 'Account', lead: sublead[sub] })
    if (sub === 'kit' && step === 'keep_kit' && av.kit?.length) {
      return subPage(html`${KIT_FORM('/settings/account/kit')}<p class="room-lead">Download or print it, and keep it somewhere safe. It is shown only now. The old kit no longer works.</p>${kitBox(kitOf(m), av.kit)}`)
    }
    if (!st) return roomPage('Account', html`${top}<div class="set-group"><p class="set-empty room-wait">Loading…</p></div>${storagePart()}`, 'Your login, and what this device keeps.')
    const pk = st.passkeys ?? [], noPassword = st.has_password === false
    if (sub === 'password') {
      return subPage(noPassword
        ? html`<div class="set-group"><div class="set-row set-fact"><span class="set-ico">${sk('lock')}</span><b>Password</b><span class="set-detail" id="password-none">none: your passkey opens this account</span><span></span></div></div>
${st.email ? html`<section id="pw-add"><h3 class="set-label">Add a password</h3>${form('/settings/password', html`<div class="set-group set-proof-g">${proofField(st)}</div><div class="set-group set-form">${pwField({ name: 'next', label: 'New password' })}</div>`, 'Add Password', 'pw-add-form')}</section>` : html`<p class="set-note" id="pw-needs-email">A password needs an email. <a href="/settings/account/email" data-nav>Add one first.</a></p>`}
<p class="set-note set-note-after">${NO_RECOVERY_PASSKEY}</p>`
        : html`<section id="pw-change">${form('/settings/password', html`<div class="set-group set-proof-g">${proofField(st, 'Current password')}</div><div class="set-group set-form">${pwField({ name: 'next', label: 'New password' })}</div>`, 'Change Password', 'pw-form')}</section>
<p class="set-note set-note-after">${NO_RECOVERY}</p>`)
    }
    if (sub === 'passkeys') {
      return subPage(html`${pk.length ? html`<div class="set-group" id="passkeys">${pk.map((p, i) => html`<details class="set-exp room-passkey" id="passkey-${p.credential_id.slice(0, 12)}"><summary class="set-row set-row-two"><span class="set-ico">${sk('key')}</span><span class="set-two"><b>Passkey${pk.length > 1 ? ` ${i + 1}` : ''}</b><small>added ${day(p.created_at)} · ${p.last_used_at ? `used ${day(p.last_used_at)}` : 'not used yet'}</small></span><span class="set-detail">Remove</span>${SET_CHEVRON}</summary>
<form method="post" action="/settings/passkeys/remove" class="set-exp-body set-stack room-remove"><input type="hidden" name="id" value="${p.credential_id}"><p class="set-small">This passkey no longer opens your account after this.</p><div class="set-group set-proof-g">${proofField(st)}</div><button type="submit" class="room-danger">Remove Passkey</button></form></details>`)}</div>`
        : html`<div class="set-group"><p class="set-empty" id="passkeys-none">None yet. A passkey logs you in without a password.</p></div>`}
${!noPassword && pk.length ? html`<p class="set-note" id="password-still">Your password still opens this account.</p>` : ''}
${passkeysOn() ? html`<section id="passkey-new"><h3 class="set-label">Add a passkey</h3>${form('/settings/passkeys', html`<div class="set-group set-proof-g">${proofField(st)}</div>`, 'Add Passkey', 'passkey-form')}</section>` : ''}`)
    }
    if (sub === 'kit') {
      return subPage(html`<div class="set-group"><div class="set-row set-fact"><span class="set-ico">${sk('page')}</span><b>Your kit</b><span class="set-detail" id="kit-state">opens with your ${st.kit_form === 'id' ? 'account ID' : 'email'}</span><span></span></div></div>
<p class="set-note">${noPassword ? 'It opens your account if you lose your passkey.' : 'With it you can set a new password if you forget yours.'} A new kit makes the old one stop working.</p>
<section id="kit-new"><h3 class="set-label">Make a new kit</h3>${form('/settings/kit', html`<div class="set-group set-proof-g">${proofField(st)}</div>`, 'Make the Kit', 'kit-form')}</section>`)
    }
    if (sub === 'email') {
      return subPage(st.email ? html`<div class="set-group"><div class="set-row set-fact"><span class="set-ico">${sk('letter')}</span><b>Email</b><span class="set-detail">${st.email}</span><span></span></div></div>`
        : html`<section id="email-add"><p class="set-note set-note-after">It cannot be changed later. Your Emergency Kit keeps its twelve words and opens with the email from then on: type them once more. They also confirm it is you.</p>${form('/settings/email', html`<div class="set-group set-form"><label class="set-field">Email<input type="email" name="email" required autocomplete="username" autocapitalize="off" spellcheck="false"></label><label class="set-field">Twelve words<textarea name="words" rows="3" required autocomplete="off" autocapitalize="off" spellcheck="false"></textarea></label></div>`, 'Add Email', 'email-form')}</section>`)
    }
    if (sub === 'delete') {
      return subPage(html`<p class="set-note set-note-after">Every device of yours is signed out, and what the hub keeps of your account is deleted. This cannot be undone.</p>${form('/settings/account/delete', html`<div class="set-group set-form">${pwField({ label: 'Your password', gen: false })}</div>`, 'Delete Account', 'delete-form', 'room-danger')}`)
    }
    const ways = [
      ...(st.email ? [] : [{ key: 'email', icon: sk('letter'), word: 'Email', detail: 'none yet' }]),
      { key: 'password', icon: sk('lock'), word: 'Password', detail: noPassword ? 'none' : 'set' },
      ...(!passkeysOn() && !pk.length ? [] : [{ key: 'passkeys', icon: sk('key'), word: 'Passkeys', detail: pk.length ? String(pk.length) : 'none' }]),
      { key: 'kit', icon: sk('page'), word: 'Emergency Kit', detail: '' },
    ]
    return roomPage('Account', html`${top}<div class="set-group set-who"><div class="set-row set-row-two set-fact"><span class="set-ico">${sk(st.email ? 'letter' : 'key')}</span><span class="set-two">${st.email ? html`<b id="account-email">${st.email}</b>` : html`<b>Logged in with a passkey</b>`}<small>${st.email ? '' : html`<span id="account-email">No email.</span> · `}Account ID <span class="room-mono" id="account-id">${st.account}</span></small></span><span></span><span></span></div></div>
<h3 class="set-label">Ways in</h3><nav class="set-group" aria-label="Ways in">${ways.map(w => setRow({ href: `/settings/account/${w.key}`, icon: w.icon, word: w.word, detail: w.detail, id: `settings-way-${w.key}` }))}</nav>
<p class="set-note">Each opens your account on a new device. Changing one asks for one of them again: being logged in here is not enough.</p>
${storagePart()}<div class="set-group"><a class="set-row set-danger" href="/logout" data-nav id="settings-logout"><span class="set-ico">${sk('logout')}</span><b>Log Out</b><span class="set-detail">of this device</span>${SET_CHEVRON}</a>${st.has_password !== false ? html`<a class="set-row set-danger" href="/settings/account/delete" data-nav id="settings-delete"><span class="set-ico">${sk('logout')}</span><b>Delete Account</b><span class="set-detail">everywhere</span>${SET_CHEVRON}</a>` : ''}</div>`, 'Your login, and what this device keeps.')
  }
  const accountGet = sub => ({ req, res }) => { readAccount(); page(req, res, `${SUBS[sub] ?? 'Account'} · Settings`, accountMain(sub), { view: 'settings' }) }
  t.get(/^\/settings\/account$/, accountGet(''))
  for (const sub of Object.keys(SUBS)) t.get(new RegExp(`^/settings/account/${sub}$`), accountGet(sub))
  // (every change: the core works, then says it with a toast, a problem, or a new kit shown on the kit's page)
  const change = (path, action) => t.post(path, ({ res, form }) => {
    local.kitFrom = 'settings'
    if (isDemo()) return t.sendStream(null, res, t.toast({ head: 'The demo changes nothing here.' }))
    t.account(action(form))
    t.done(res)
  })
  change(/^\/settings\/password$/, f => ({ step: 'set_password', unlock: unlockOf(f), next: field(f, 'next') }))
  change(/^\/settings\/passkeys$/, f => ({ step: 'add_passkey', unlock: unlockOf(f) }))
  change(/^\/settings\/passkeys\/remove$/, f => ({ step: 'remove_passkey', credential_id: field(f, 'id'), unlock: unlockOf(f) }))
  change(/^\/settings\/kit$/, f => ({ step: 'new_kit', unlock: unlockOf(f) }))
  change(/^\/settings\/email$/, f => ({ step: 'set_email', email: field(f, 'email').trim(), words: field(f, 'words') }))
  change(/^\/settings\/recovery-code$/, f => ({ step: 'replace_recovery_code', unlock: unlockOf(f) }))
  change(/^\/settings\/account\/delete$/, f => ({ step: 'delete_account', password: field(f, 'password') }))
}

// ---- Settings · Account's shared pieces ----
const form = (action, inner, word, id, cls = 'room-primary') => html`<form method="post" action="${action}" class="set-stack" data-controller="room" id="${id}">${inner}<button type="submit" class="${cls}">${word}</button></form>`
/** A password field (Settings' look). */
const pwField = ({ name = 'password', label = 'Password', gen = true } = {}) => html`<label>${label}<span class="room-pwrow"><input type="password" name="${name}" required minlength="${gen ? 12 : 1}" autocomplete="${gen ? 'new-password' : 'current-password'}" spellcheck="false" autocapitalize="off"></span>${gen ? html`<small class="room-hint">At least 12 characters.</small>` : ''}</label>`
const CONFIRM_PASSWORD = 'To change how your account opens, enter your current password. Being logged in here is not enough.'
const CONFIRM_PASSKEY = 'To change how your account opens, confirm with one of your passkeys. Being logged in here is not enough.'
/** The way in a change asks for again: the password where the account has one, else one of its passkeys (asked by
 *  the core when the form is sent). The one place that draws it. */
const proofField = (login, label = 'Your password') => html`<div class="set-proof" id="proof"><p class="set-proof-why"><span class="set-ico">${sk('lock')}</span><span><b>Confirm it is you</b><small>${login.has_password === false ? CONFIRM_PASSKEY : CONFIRM_PASSWORD}</small></span></p>${login.has_password === false ? '' : pwField({ label, gen: false })}</div>`
const NO_RECOVERY = 'If you lose your password and your Emergency Kit, nobody (not even Trommi) can recover your data.'
const NO_RECOVERY_PASSKEY = 'If you lose your passkeys and your Emergency Kit, nobody (not even Trommi) can recover your data.'
/** A new kit in Settings: its words in full, Download, Print, and Done (the words leave the view). */
const kitBox = (kit, words) => html`<div class="room-kit" id="kit"><p class="room-kit-title">${BELL} Trommi Emergency Kit</p>${kit.email ? html`<p class="room-meta">Email: <b>${kit.email}</b></p>` : ''}
<ol class="room-kit-words" id="kit-words">${words.map(w => html`<li>${w}</li>`)}</ol>
${kitIdentity(kit)}
<p class="room-meta">Lost your way in? app.trommi.com → Log in → "Forgot?" → your ${kitOpensWith(kit)} and these 12 words.</p></div>
<div class="room-actions room-kit-actions">${kitButton('download', 'Download', '', 'kit-download')}${kitButton('print', 'Print', '', 'kit-print')}${kitButton('done', 'Done', 'room-primary', 'kit-done')}</div>`
const storagePart = () => html`<h3 class="set-label" id="store-head">Storage</h3><dl class="set-group set-facts" data-controller="room"><div class="set-row set-fact"><span class="set-ico">${sk('laptop')}</span><dt>On this device</dt><dd class="set-detail" data-room-target="local">…</dd><span></span></div></dl><p class="set-note">The hub deletes envelopes after 30 days; your devices keep what they decrypted.</p>`

// ---- the demo (/screens: ?mock=1&onboard=<state>): every state of the screens before the board, each drawn from a
// made-up AccountView and page memory; no core, nothing sent ----
const DEMO_ID = '0f8fad5b-d9cb-469f-a165-70867728950e'
const DEMO_EMOJI = [['🦊', 'fox'], ['🌵', 'cactus'], ['🎈', 'balloon'], ['🐢', 'turtle'], ['🍋', 'lemon'], ['⚓', 'anchor']].map(([emoji, word]) => ({ emoji, word }))
const DEMO_WORDS = KIT_EMPTY.split(' ')
const DEMO_KIT = { email: 'ada@example.org', account: DEMO_ID, has_password: true }
const W = (words = DEMO_WORDS) => ({ words })
/** state -> [screen, options over the defaults, values typed into fields after drawing] */
const DEMO = {
  welcome: ['start'], 'logged-out': ['start', { loggedOut: true }],
  create: ['create', { way: 'passkey' }], 'create-nopasskey': ['create', { pk: false, pkOff: false, way: 'password' }],
  'create-passkey-waiting': ['create', { way: 'passkey', busy: 'Waiting for passkey…' }],
  'create-passkey-cancelled': ['create', { way: 'passkey', problem: 'No passkey made.' }],
  'create-passkey-fallback': ['create', { way: 'password', note: NO_PRF }],
  'create-password': ['create'], 'create-typing': ['create', {}, { '#ob-pw': 'seven77' }], 'create-generated': ['create', {}, { '#ob-pw': 'orbit lantern maple quiet saddle' }],
  'create-error': ['create', { problem: 'The password needs at least 12 characters.' }, { '#ob-pw': 'short' }],
  'create-busy': ['create', { busy: 'Creating…' }, { '#ob-pw': 'a long enough password' }],
  'create-offline': ['create', { problem: 'The hub cannot be reached. Try again.' }, { '#ob-pw': 'a long enough password' }],
  'create-limit': ['create', { problem: 'Too many tries. Wait a moment, then try again.' }, { '#ob-pw': 'a long enough password' }],
  kit: ['kit', W()], 'kit-shown': ['kit', { ...W(), shown: true, saved: true }], 'kit-again': ['kit'], 'kit-wrong': ['kit', { problem: 'Wrong password.' }],
  'kit-passkey': ['kit', { ...W(), ways: { password: false, passkey: true } }], 'kit-noemail': ['kit', { ...W(), shown: true, saved: true, kit: { ...DEMO_KIT, email: null }, ways: { password: false, passkey: true } }],
  'kit-again-passkey': ['kit', { ways: { password: false, passkey: true } }], 'kit-again-both': ['kit', { ways: { password: true, passkey: true } }],
  login: ['login'], 'login-nopasskey': ['login', { pk: false, pkOff: false }], 'login-error': ['login', { problem: 'The e-mail or the password is wrong.' }, { '#ob-pw': 'not my password' }],
  'login-id': ['login', { email: DEMO_ID, problem: ID_NO_PASSWORD }],
  'login-passkey-waiting': ['login', { passkeyBusy: 'Waiting for passkey…' }], 'login-passkey-busy': ['login', { passkeyBusy: 'Logging in…' }],
  'login-passkey-error': ['login', { at: 'passkey', problem: 'This passkey opens no account here.' }], 'login-passkey-noprf': ['login', { at: 'passkey', problem: NO_PRF_HERE }],
  'login-limit': ['login', { problem: 'Too many tries. Wait a moment, then try again.' }], 'login-offline': ['login', { problem: 'The hub cannot be reached. Try again.' }],
  forgot: ['forgot'], 'forgot-passkey': ['forgot', { way: 'passkey' }],
  'forgot-kit': ['forgot', { way: 'passkey', email: DEMO_ID, fromKit: true }], 'forgot-kit-hub': ['forgot', { way: 'passkey', email: DEMO_ID, otherHub: 'hub.example.org', fromKit: true }],
  'forgot-wrong-id': ['forgot', { way: 'passkey', email: DEMO_ID, fromKit: true, problem: 'Wrong account ID or words. A kit that shows an email opens with that email.' }, { textarea: KIT_EMPTY }],
  'new-passkey': ['newway', { way: 'passkey' }], 'new-passkey-fallback': ['newway', { way: 'password', note: NO_PRF }],
  scan: ['scan', { still: 'scanning' }], 'scan-asking': ['scan', { still: 'asking' }], 'scan-denied': ['scan', { still: 'denied' }], 'scan-none': ['scan', { still: 'none' }],
  'scan-tap': ['scan', { still: 'tap' }], 'scan-wrong': ['scan', { still: 'wrong' }], 'scan-paste': ['scan', { still: 'scanning', paste: true }],
  join: ['join'], 'join-emoji': ['join', { emoji: DEMO_EMOJI }], 'join-expired': ['join', { problem: 'The link has ended. Ask for a new one.' }],
  'join-offline': ['joinOffline'], 'join-burned': ['join', { problem: 'The other device said the emoji do not match. Nothing was joined.' }],
  broken: ['broken', { detail: 'no-device: the device keys are not in this browser' }],
}
/** The demo's account screens (app.mjs boot, ?mock=1&onboard=<state>): drawn once, nothing sent. */
export function demoScreen(state) {
  const [name, over = {}, typed = {}] = DEMO[state] ?? DEMO.welcome
  const o = { pk: true, pkOff: false, way: 'password', busy: '', passkeyBusy: '', at: 'form', problem: '', email: 'ada@example.org', loggedOut: false, emoji: [], words: null, shown: false, saved: false, kit: DEMO_KIT, ways: { password: true, passkey: false }, demo: true, ...over }
  document.title = 'Trommi'
  document.body.innerHTML = String(screenMain(name, o))
  const root = document.getElementById('room-screen')
  // (a preview: password managers stay off its fields; nothing is sent, nothing leads elsewhere)
  for (const f of root.querySelectorAll('input, textarea')) { f.autocomplete = 'off'; for (const a of ['data-1p-ignore', 'data-bwignore', 'data-protonpass-ignore']) f.setAttribute(a, ''); f.setAttribute('data-lpignore', 'true'); f.setAttribute('data-form-type', 'other') }
  root.addEventListener('submit', e => { e.preventDefault(); e.stopImmediatePropagation() }, true)
  root.addEventListener('click', e => { if (e.target.closest('a[href], [data-controller~="ob-nav"], #broken-logout, #broken-retry, #join-again, #scan-again')) { e.preventDefault(); e.stopImmediatePropagation() } }, true)
  for (const [sel, value] of Object.entries(typed)) { const f = root.querySelector(sel); if (f) f.value = value }
}

// ---- qr ----
// QR codes for pairing, self-contained (no CDN, no dependency): qrSvg(text) draws the invite link as an SVG (byte mode,
// error correction M, versions 1-40, the mask with the lowest penalty), scanQr(video) reads one from the camera with the browser's
// BarcodeDetector where it exists. The encoder follows ISO/IEC 18004 as Project Nayuki's reference describes it.

const ECC_PER_BLOCK = [-1, 10, 16, 26, 18, 24, 16, 18, 22, 22, 26, 30, 22, 22, 24, 24, 28, 28, 26, 26, 26, 26, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28]
const BLOCKS = [-1, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5, 5, 8, 9, 9, 10, 10, 11, 13, 14, 16, 17, 17, 18, 20, 21, 23, 25, 26, 28, 29, 31, 33, 35, 37, 38, 40, 43, 45, 47, 49]
const FORMAT_M = 0

const rawModules = v => { let r = (16 * v + 128) * v + 64; if (v >= 2) { const n = Math.floor(v / 7) + 2; r -= (25 * n - 10) * n - 55; if (v >= 7) r -= 36 } return r }
const dataCodewords = v => Math.floor(rawModules(v) / 8) - ECC_PER_BLOCK[v] * BLOCKS[v]
const bit = (x, i) => ((x >>> i) & 1) !== 0

function gfMul(x, y) { let z = 0; for (let i = 7; i >= 0; i--) { z = (z << 1) ^ ((z >>> 7) * 0x11d); z ^= ((y >>> i) & 1) * x } return z }
function rsDivisor(degree) {
  const r = new Array(degree).fill(0); r[degree - 1] = 1
  let root = 1
  for (let i = 0; i < degree; i++) {
    for (let j = 0; j < r.length; j++) { r[j] = gfMul(r[j], root); if (j + 1 < r.length) r[j] ^= r[j + 1] }
    root = gfMul(root, 0x02)
  }
  return r
}
function rsRemainder(data, div) {
  const r = div.map(() => 0)
  for (const b of data) { const f = b ^ r.shift(); r.push(0); div.forEach((c, i) => { r[i] ^= gfMul(c, f) }) }
  return r
}

function alignmentPositions(v) {
  if (v === 1) return []
  const n = Math.floor(v / 7) + 2
  const step = v === 32 ? 26 : Math.ceil((v * 4 + 4) / (n * 2 - 2)) * 2
  const r = [6]
  for (let pos = v * 4 + 10; r.length < n; pos -= step) r.splice(1, 0, pos)
  return r
}

/** The modules of a QR code for `text` (UTF-8, byte mode, level M): an array of rows of booleans. */
function qrMatrix(text) {
  const bytes = [...new TextEncoder().encode(text)]
  let v = 1
  for (; v <= 40; v++) if (4 + (v < 10 ? 8 : 16) + bytes.length * 8 <= dataCodewords(v) * 8) break
  if (v > 40) throw new Error('too long for a QR code')
  const cap = dataCodewords(v) * 8
  const bits = []
  const push = (val, len) => { for (let i = len - 1; i >= 0; i--) bits.push((val >>> i) & 1) }
  push(4, 4); push(bytes.length, v < 10 ? 8 : 16); for (const b of bytes) push(b, 8)
  push(0, Math.min(4, cap - bits.length)); push(0, (8 - bits.length % 8) % 8)
  for (let pad = 0xec; bits.length < cap; pad ^= 0xec ^ 0x11) push(pad, 8)
  const data = []
  for (let i = 0; i < bits.length; i += 8) data.push(bits.slice(i, i + 8).reduce((a, b) => (a << 1) | b, 0))

  // Error correction, split into blocks and interleaved.
  const nb = BLOCKS[v], eccLen = ECC_PER_BLOCK[v], raw = Math.floor(rawModules(v) / 8)
  const short = nb - raw % nb, shortLen = Math.floor(raw / nb), div = rsDivisor(eccLen)
  const blocks = []
  for (let i = 0, k = 0; i < nb; i++) {
    const dat = data.slice(k, k + shortLen - eccLen + (i < short ? 0 : 1)); k += dat.length
    const ecc = rsRemainder(dat, div)
    if (i < short) dat.push(0)
    blocks.push(dat.concat(ecc))
  }
  const words = []
  for (let i = 0; i < blocks[0].length; i++) blocks.forEach((b, j) => { if (i !== shortLen - eccLen || j >= short) words.push(b[i]) })

  const size = v * 4 + 17
  const mod = Array.from({ length: size }, () => new Array(size).fill(false))
  const fn = Array.from({ length: size }, () => new Array(size).fill(false))
  const set = (x, y, dark) => { mod[y][x] = dark; fn[y][x] = true }
  for (let i = 0; i < size; i++) { set(6, i, i % 2 === 0); set(i, 6, i % 2 === 0) }
  for (const [cx, cy] of [[3, 3], [size - 4, 3], [3, size - 4]]) {
    for (let dy = -4; dy <= 4; dy++) for (let dx = -4; dx <= 4; dx++) {
      const d = Math.max(Math.abs(dx), Math.abs(dy)), x = cx + dx, y = cy + dy
      if (x >= 0 && x < size && y >= 0 && y < size) set(x, y, d !== 2 && d !== 4)
    }
  }
  const al = alignmentPositions(v), last = al.length - 1
  al.forEach((ax, i) => al.forEach((ay, j) => {
    if ((i === 0 && j === 0) || (i === 0 && j === last) || (i === last && j === 0)) return
    for (let dy = -2; dy <= 2; dy++) for (let dx = -2; dx <= 2; dx++) set(ax + dx, ay + dy, Math.max(Math.abs(dx), Math.abs(dy)) !== 1)
  }))
  const format = mask => {
    const d = (FORMAT_M << 3) | mask
    let r = d; for (let i = 0; i < 10; i++) r = (r << 1) ^ ((r >>> 9) * 0x537)
    const b = ((d << 10) | r) ^ 0x5412
    for (let i = 0; i <= 5; i++) set(8, i, bit(b, i))
    set(8, 7, bit(b, 6)); set(8, 8, bit(b, 7)); set(7, 8, bit(b, 8))
    for (let i = 9; i < 15; i++) set(14 - i, 8, bit(b, i))
    for (let i = 0; i < 8; i++) set(size - 1 - i, 8, bit(b, i))
    for (let i = 8; i < 15; i++) set(8, size - 15 + i, bit(b, i))
    set(8, size - 8, true)
  }
  format(0)
  if (v >= 7) {
    let r = v; for (let i = 0; i < 12; i++) r = (r << 1) ^ ((r >>> 11) * 0x1f25)
    const b = (v << 12) | r
    for (let i = 0; i < 18; i++) { const a = size - 11 + i % 3, c = Math.floor(i / 3); set(a, c, bit(b, i)); set(c, a, bit(b, i)) }
  }
  // The data, in the zigzag of two-module columns.
  for (let right = size - 1, i = 0; right >= 1; right -= 2) {
    if (right === 6) right = 5
    for (let vert = 0; vert < size; vert++) for (let j = 0; j < 2; j++) {
      const x = right - j, y = ((right + 1) & 2) === 0 ? size - 1 - vert : vert
      if (!fn[y][x] && i < words.length * 8) { mod[y][x] = bit(words[i >>> 3], 7 - (i & 7)); i++ }
    }
  }
  const MASKS = [(x, y) => (x + y) % 2 === 0, (x, y) => y % 2 === 0, x => x % 3 === 0, (x, y) => (x + y) % 3 === 0,
    (x, y) => (Math.floor(x / 3) + Math.floor(y / 2)) % 2 === 0, (x, y) => (x * y) % 2 + (x * y) % 3 === 0,
    (x, y) => ((x * y) % 2 + (x * y) % 3) % 2 === 0, (x, y) => ((x + y) % 2 + (x * y) % 3) % 2 === 0]
  const applyMask = m => { for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) if (!fn[y][x] && MASKS[m](x, y)) mod[y][x] = !mod[y][x] }
  // Penalty (runs, 2x2 blocks, balance; the finder-look rule left out: any mask reads, this only picks a calmer one).
  const penalty = () => {
    let p = 0, dark = 0
    for (let y = 0; y < size; y++) {
      for (let x = 0, runX = 0, runY = 0; x < size; x++) {
        runX = x && mod[y][x] === mod[y][x - 1] ? runX + 1 : 1; if (runX === 5) p += 3; else if (runX > 5) p++
        runY = x && mod[x][y] === mod[x - 1][y] ? runY + 1 : 1; if (runY === 5) p += 3; else if (runY > 5) p++
        if (mod[y][x]) dark++
        if (x && y && mod[y][x] === mod[y][x - 1] && mod[y][x] === mod[y - 1][x] && mod[y][x] === mod[y - 1][x - 1]) p += 3
      }
    }
    return p + Math.floor(Math.abs(dark * 20 - size * size * 10) / (size * size)) * 10
  }
  let best = 0, bestP = Infinity
  for (let m = 0; m < 8; m++) { applyMask(m); format(m); const p = penalty(); if (p < bestP) { best = m; bestP = p } applyMask(m) }
  applyMask(best); format(best)
  return mod
}

/** An SVG of the QR code (dark modules on white, with the quiet zone), scalable; `label` for screen readers. */
function qrSvg(text, label = 'QR-Code') {
  const m = qrMatrix(text), n = m.length + 8
  let d = ''
  m.forEach((row, y) => row.forEach((dark, x) => { if (dark) d += `M${x + 4} ${y + 4}h1v1h-1z` }))
  return `<svg class="qr" viewBox="0 0 ${n} ${n}" role="img" aria-label="${label}" shape-rendering="crispEdges"><rect width="${n}" height="${n}" fill="#fff"/><path d="${d}" fill="#000"/></svg>`
}

const SCAN_SAYS = { asking: 'Allow the camera to scan.', tap: '', scanning: 'Point at the code on your other device.', denied: 'Camera is off. Paste the link instead.', none: 'No camera here. Paste the link instead.', wrong: 'Not a Trommi code.' }
/** A join link (an invite's: /join, and after the # `v1.` with its parts); anything else is no code of ours. The core
 *  reads the link itself (Join): this only tells a Trommi code from another one in front of the camera. */
const joinHash = text => {
  let u
  try { u = new URL(String(text).trim()) } catch { return null }
  return u.pathname === '/join' && /^#v1\.[\w.-]+$/.test(u.hash) ? u.hash : null
}
/** The drawn corners over the camera's picture: four pen angles, a little uneven. */
const SCAN_CORNERS = raw('<svg class="scan-corners" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true"><path d="M15.2 31.5 Q14.2 21 15.6 15.8 Q22 14.4 31.8 15.3"/><path d="M68.4 14.9 Q79 14.3 84.5 15.5 Q85.9 22.5 84.8 31.9"/><path d="M85.1 68.2 Q85.8 78.6 84.4 84.6 Q77.5 85.8 68 84.9"/><path d="M31.6 85.2 Q21.4 85.9 15.5 84.3 Q14.2 77 15 68.3"/></svg>')

/** The browser's own reader where it has one for QR codes (Chrome on Android, macOS and ChromeOS; Safari where it is
 *  switched on), else the bundled one (core/qr-decode.mjs, loaded only now: Firefox, Chrome on Linux and Windows,
 *  iPhones): text of the code in `source` (a video for the browser's, the frame's pixels for ours), or null. */
async function qrReaderOf(video) {
  try {
    if ('BarcodeDetector' in window && (await BarcodeDetector.getSupportedFormats()).includes('qr_code')) {
      const detector = new BarcodeDetector({ formats: ['qr_code'] })
      return { native: true, read: async () => (await detector.detect(video))[0]?.rawValue ?? null }
    }
  } catch {}
  const { decodeQR } = await qrReader()
  const canvas = document.createElement('canvas'), ctx = canvas.getContext('2d', { willReadFrequently: true })
  return { native: false, read: async () => {
    // (the frame at no more than 960 px on its long side: enough for a code held to the camera, quick on a phone)
    const k = Math.min(1, 960 / Math.max(video.videoWidth, video.videoHeight))
    const w = Math.round(video.videoWidth * k), h = Math.round(video.videoHeight * k)
    if (!w || !h) return null
    if (canvas.width !== w || canvas.height !== h) { canvas.width = w; canvas.height = h }
    ctx.drawImage(video, 0, 0, w, h)
    try { return decodeQR(ctx.getImageData(0, 0, w, h)) || null } catch { return null }
  } }
}

/**
 * Read QR codes from the camera into `video` until onText returns true or stop() is called: the back camera where there
 * is one (a laptop gives its front one). Frames stay in this page: they are drawn to the video and read here, never
 * sent anywhere. Rejects with the browser's error (NotAllowedError, NotFoundError, …), or { name: 'play' } when the
 * video may not start without a tap.
 */
async function scanQr(video, onText) {
  const stream = await navigator.mediaDevices.getUserMedia({ video: { facingMode: { ideal: 'environment' }, width: { ideal: 1280 }, height: { ideal: 720 } }, audio: false })
  let on = true, timer = 0
  const stop = () => { on = false; clearTimeout(timer); for (const t of stream.getTracks()) t.stop(); video.srcObject = null }
  try {
    video.srcObject = stream
    video.muted = true; video.setAttribute('playsinline', '')
    try { await video.play() } catch (err) { throw Object.assign(new Error(err?.message ?? 'the video did not start'), { name: 'play' }) }
    const reader = await qrReaderOf(video)
    video.dataset.reader = reader.native ? 'browser' : 'bundled'
    const tick = async () => {
      if (!on) return
      const t0 = performance.now()
      let text = null
      try { if (video.readyState >= 2) text = await reader.read() } catch {}
      if (!on) return
      if (text && await onText(text)) return stop()
      // (a wrong code is said once, then the reader rests a moment; otherwise about six looks a second, never back to back)
      timer = setTimeout(tick, text ? 1500 : Math.max(160, (performance.now() - t0) * 1.5))
    }
    tick()
  } catch (err) { stop(); throw err }
  return stop
}

// A stand-in for the code until it is asked for (drawn blurred; it encodes nothing).
const FAKE_QR = (() => {
  const n = 25, cells = [], finder = (x, y) => (x < 7 && y < 7) || (x >= n - 7 && y < 7) || (x < 7 && y >= n - 7)
  let seed = 7
  const rnd = () => { seed = (seed * 1103515245 + 12345) & 0x7fffffff; return seed / 0x7fffffff }
  for (let y = 0; y < n; y++) for (let x = 0; x < n; x++) {
    const ring = finder(x, y) && (() => { const fx = x < 7 ? x : x - (n - 7), fy = y < 7 ? y : y - (n - 7); return fx === 0 || fx === 6 || fy === 0 || fy === 6 || (fx >= 2 && fx <= 4 && fy >= 2 && fy <= 4) })()
    if (finder(x, y) ? ring : rnd() > 0.52) cells.push(`M${x} ${y}h1v1h-1z`)
  }
  return raw(`<svg viewBox="-2 -2 ${n + 4} ${n + 4}" aria-hidden="true"><rect x="-2" y="-2" width="${n + 4}" height="${n + 4}" fill="#fff"/><path d="${cells.join('')}" fill="#14181a"/></svg>`)
})()

// ---- controllers ----
// "room": select a read-only field on focus, copy a link, this device's storage (navigator.storage).
const size = n => (n == null ? '–' : n < 1e3 ? `${n} B` : n < 1e6 ? `${(n / 1e3).toFixed(0)} kB` : n < 1e9 ? `${(n / 1e6).toFixed(1)} MB` : `${(n / 1e9).toFixed(2)} GB`)
controller('room', class extends Controller {
  static targets = ['field', 'label', 'local']
  connect() { if (this.hasLocalTarget) this.usage() }
  disconnect() { clearTimeout(this.timer) }
  select(e) { e.target.select() }
  async copy(e) {
    const ok = await copyText(e.params.text)
    const label = e.currentTarget.querySelector('[data-room-target="label"]')
    if (!label) return
    label.textContent = ok ? 'Copied' : 'Not copied'
    clearTimeout(this.timer)
    this.timer = setTimeout(() => { label.textContent = 'Copy' }, 1800)
  }
  async usage() {
    try { const e = await navigator.storage?.estimate?.(); this.localTarget.textContent = e ? size(e.usage) : 'unknown' } catch { this.localTarget.textContent = 'unknown' }
  }
})

// The agent invite's clipboard: a press on a command copies it and ticks its line.
controller('invite-clip', class extends Controller {
  disconnect() { clearTimeout(this.said) }
  async copy(e) {
    const line = e.currentTarget, word = line.querySelector('.clip-copy-word'), ok = await copyText(e.params.text)
    word.textContent = ok ? 'Copied' : 'Not copied'
    if (ok) line.closest('.clip-step')?.classList.add('is-done')
    clearTimeout(this.said)
    this.said = setTimeout(() => { for (const w of this.element.querySelectorAll('.clip-copy-word')) w.textContent = w.dataset.word }, 1800)
  }
  // (while the name is typed the command below still has the old one: it is not copied until the name is in, at
  //  Enter or when the field is left; then the core writes the command anew and the page is drawn again)
  typed() {
    const line = this.element.querySelector('.clip-copy[data-line="setup"]')
    if (!line || line.disabled) return
    line.disabled = true
    line.querySelector('.clip-copy-word').textContent = 'Press Enter'
  }
  name(e) {
    actCore({ do: 'account', action: { step: 'hermes_name', name: e.target.value } })
    // (the page drawn again keeps what a controller changed: the line is let go here; its text comes with the core)
    const line = this.element.querySelector('.clip-copy[data-line="setup"]')
    if (line) { line.disabled = false; const w = line.querySelector('.clip-copy-word'); w.textContent = w.dataset.word }
  }
})

controller('set-theme', class extends Controller {
  connect() { for (const r of this.element.querySelectorAll('input[name="theme"]')) r.checked = r.value === themeMode() }
  pick(e) { setThemeMode(e.target.value) }
})
// Dictation on this device: the switch is sent at once (Action::Dictate switch); the row follows the core's view.
controller('set-dictation', class extends Controller {
  flip(e) { actCore({ do: 'dictate', step: { step: 'switch', on: e.target.checked } }) }
})
// Push on this device: a level picked is sent at once (the form's handler: Action::Push).
controller('set-push', class extends Controller {
  pick() { this.element.requestSubmit() }
})
// A password field's Show/Hide.
controller('ob-pw', class extends Controller {
  static targets = ['field']
  eye(e) {
    const on = this.fieldTarget.type === 'password'
    this.fieldTarget.type = on ? 'text' : 'password'
    e.currentTarget.setAttribute('aria-pressed', String(on))
    e.currentTarget.textContent = on ? 'Hide' : 'Show'
  }
})
// A screen's first field (or its one button) takes the focus when the screen comes; never inside a frame (/screens).
controller('ob-focus', class extends Controller {
  static values = { at: String }
  connect() {
    const framed = (() => { try { return window.top !== window } catch { return true } })()
    if (!framed && this.atValue) this.element.querySelector(this.atValue)?.focus({ preventScroll: true })
  }
})
// A button that leads to another screen.
controller('ob-nav', class extends Controller {
  static values = { to: String }
  go() { window.Turbo?.visit(this.toValue) }
})
// The kit's sheet: a tap on it is a tap on Show.
controller('kit', class extends Controller {
  flip() { document.getElementById('kit-show')?.click() }
})

// Scan a code: the camera's stream while the screen is in view (and the tab is); a code of this app is joined with
// (posted as a pasted link would be), another one is said for a moment and the camera keeps looking.
controller('scan', class extends Controller {
  static targets = ['video']
  connect() {
    this.alive = true
    this.hidden = () => { if (document.hidden) this.end(); else if (['scanning', 'asking'].includes(this.state)) this.begin() }
    document.addEventListener('visibilitychange', this.hidden)
    this.begin()
  }
  disconnect() { this.alive = false; clearTimeout(this.said); this.end(); document.removeEventListener('visibilitychange', this.hidden) }
  end() { this.stop?.(); this.stop = null }
  set(state, words = SCAN_SAYS[state]) {
    this.state = state
    this.element.dataset.state = state
    const line = document.getElementById('scan-say'), again = document.getElementById('scan-again')?.parentElement, paste = document.getElementById('scan-paste')
    if (line) line.textContent = words
    if (again) again.hidden = state !== 'denied'
    if ((state === 'denied' || state === 'none') && paste && !paste.open) paste.open = true
  }
  async begin() {
    if (this.stop || !this.alive) return
    if (!navigator.mediaDevices?.getUserMedia) return this.set('none')
    document.getElementById('scan-again')?.addEventListener('click', () => this.begin(), { once: true })
    this.set('asking')
    try {
      this.stop = await scanQr(this.videoTarget, async text => {
        if (joinHash(text)) {
          await fetch('/join', { method: 'POST', body: new URLSearchParams({ link: text }) })
          window.Turbo?.visit('/join', { action: 'replace' })
          return true
        }
        const line = document.getElementById('scan-say')
        if (line) line.textContent = SCAN_SAYS.wrong
        clearTimeout(this.said); this.said = setTimeout(() => { if (this.state === 'scanning' && line) line.textContent = SCAN_SAYS.scanning }, 2500)
        return false
      })
      if (!this.alive || document.hidden) return this.end()
      this.set('scanning')
    } catch (err) {
      console.warn('camera:', err?.name ?? '', err?.message ?? err)
      // (iOS: a video that may not start by itself waits for a tap)
      if (err?.name === 'play') return this.set('tap')
      this.set(['NotAllowedError', 'SecurityError'].includes(err?.name) ? 'denied' : 'none')
    }
  }
})
