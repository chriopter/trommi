// platform.mjs: what only the page can do for the app core (core/client/src/app/io.rs PlatformOp): passkeys
// (WebAuthn with the prf extension), Web Push (the service worker's push manager) and dictation's microphone and
// attestation check (dictate.mjs). The core asks; this answers
// with bytes (by index in `blobs`, as the contract carries them), or `{ answer: 'refused' }` when the person said no
// or the browser cannot. Nothing here decides anything: which passkey, which hub, what the prf output opens is the
// core's.
import { dictateOp } from './dictate.mjs'

/** The bytes of what WebAuthn hands back, copied: an ArrayBuffer or a view (also one of another realm: a password
 *  manager's extension answering for the browser), or base64url text some of them give instead. */
const bytesOf = v => {
  if (ArrayBuffer.isView(v)) return new Uint8Array(v.buffer, v.byteOffset, v.byteLength).slice()
  if (v instanceof ArrayBuffer || Object.prototype.toString.call(v) === '[object ArrayBuffer]') return new Uint8Array(v).slice()
  if (typeof v === 'string' && /^[A-Za-z0-9_-]*={0,2}$/.test(v)) return Uint8Array.from(atob(v.replace(/=+$/, '').replace(/-/g, '+').replace(/_/g, '/')), ch => ch.charCodeAt(0))
  return null
}
/** A ceremony's answer read field by field: from its JSON form (`toJSON()`, WebAuthn level 3) where it has one,
 *  else from the objects themselves (whose getters a provider may leave empty). */
function readCredential(cred) {
  let json = null
  try { json = typeof cred?.toJSON === 'function' ? cred.toJSON() : null } catch { json = null }
  const raw = cred?.response ?? {}, jr = json?.response ?? {}
  const pick = (...vs) => { for (const v of vs) { if (v == null || v === '') continue; const b = bytesOf(v); if (b) return b } return null }
  let ext = null
  try { ext = cred?.getClientExtensionResults?.() ?? null } catch { ext = null }
  const prfOf = e => e?.prf?.results?.first
  return {
    field: name => pick(jr[name], raw[name]),
    id: () => pick(json?.rawId ?? json?.id, cred?.rawId),
    prf: () => pick(prfOf(json?.clientExtensionResults), prfOf(ext)),
    prfEnabled: () => (json?.clientExtensionResults?.prf ?? ext?.prf)?.enabled,
  }
}
const REFUSED = { answer: { answer: 'refused' }, blobs: [] }
const refused = err => (['NotAllowedError', 'AbortError', 'InvalidStateError', 'SecurityError'].includes(err?.name) ? REFUSED : Promise.reject(err))

async function passkeyGet(op, blobs) {
  const cred = await navigator.credentials.get({ publicKey: {
    challenge: blobs[op.challenge], rpId: op.rp_id, userVerification: 'required',
    allowCredentials: (op.credential_ids ?? []).map(i => ({ type: 'public-key', id: blobs[i] })),
    extensions: { prf: { eval: { first: blobs[op.prf_input] } } },
  } }).catch(refused)
  if (cred === REFUSED || !cred) return REFUSED
  const a = readCredential(cred)
  const out = [a.id(), a.field('authenticatorData'), a.field('clientDataJSON'), a.field('signature'), a.prf()]
  if (out.some(b => !b)) return REFUSED   // (no prf: this passkey cannot open the account)
  const handle = a.field('userHandle')
  if (handle?.length) out.push(handle)
  return { answer: { answer: 'passkey_asserted', credential_id: 0, authenticator_data: 1, client_data_json: 2, signature: 3, prf: 4, ...(handle?.length ? { user_handle: 5 } : {}) }, blobs: out }
}

async function passkeyCreate(op, blobs) {
  const cred = await navigator.credentials.create({ publicKey: {
    challenge: blobs[op.challenge], rp: { id: op.rp_id, name: 'Trommi' },
    user: { id: blobs[op.user_handle], name: op.user_name, displayName: op.user_name },
    pubKeyCredParams: [{ type: 'public-key', alg: -7 }, { type: 'public-key', alg: -8 }],
    authenticatorSelection: { residentKey: 'required', requireResidentKey: true, userVerification: 'required' },
    attestation: 'none', extensions: { prf: { eval: { first: blobs[op.prf_input] } } },
  } }).catch(refused)
  if (cred === REFUSED || !cred) return REFUSED
  const a = readCredential(cred)
  const id = a.id()
  let prf = a.prf()
  // (a provider that gives the prf output only at get(): asked at once, for this credential, a second prompt)
  if (!prf && id && a.prfEnabled() !== false) {
    const got = await passkeyGet({ rp_id: op.rp_id, challenge: 0, credential_ids: [1], prf_input: 2 }, [crypto.getRandomValues(new Uint8Array(32)), id, blobs[op.prf_input]]).catch(() => REFUSED)
    prf = got === REFUSED ? null : got.blobs[got.answer.prf]
  }
  const out = [id, a.field('attestationObject'), a.field('clientDataJSON'), prf]
  if (out.some(b => !b)) {
    // (a passkey that cannot open the account is of no use: its store is told to drop it, where the browser can)
    try { Promise.resolve(window.PublicKeyCredential?.signalUnknownCredential?.({ rpId: op.rp_id, credentialId: b64u(id) })).catch(() => {}) } catch {}
    return REFUSED
  }
  return { answer: { answer: 'passkey_created', credential_id: 0, attestation_object: 1, client_data_json: 2, prf: 3 }, blobs: out }
}
const b64u = b => btoa(String.fromCharCode(...b)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')

// ---- push ----
const apple = () => /iPhone|iPad|iPod/.test(navigator.userAgent) || (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1)
const installed = () => navigator.standalone === true || matchMedia('(display-mode: standalone)').matches
/** Why push cannot be switched on here, in words; '' when it can. */
export function pushObstacle() {
  if (!globalThis.isSecureContext) return 'Push needs the app at its https address.'
  if (apple() && !installed()) return 'On the iPhone, add the app to the Home Screen first and open it from there.'
  if (!('serviceWorker' in navigator && 'PushManager' in window && 'Notification' in window)) return 'This browser has no push.'
  if (Notification.permission === 'denied') return 'Notifications are blocked for the app (browser settings).'
  return ''
}
const subscription = async () => (await navigator.serviceWorker?.getRegistration('/'))?.pushManager.getSubscription() ?? null

async function pushRegister(op, blobs) {
  if (pushObstacle()) return REFUSED
  let sub = await subscription()
  if (!sub) {
    if ((await Notification.requestPermission()) !== 'granted') return REFUSED
    const reg = await navigator.serviceWorker.register('/sw.js')
    await navigator.serviceWorker.ready
    if (op.server_key == null) return REFUSED
    sub = await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: blobs[op.server_key] })
  }
  return { answer: { answer: 'push', kind: 'web_push', token: 0 }, blobs: [new TextEncoder().encode(JSON.stringify(sub.toJSON()))] }
}
async function pushUnregister() {
  await (await subscription())?.unsubscribe()
  return { answer: { answer: 'done' }, blobs: [] }
}

/** Answers one platform op of the core. */
export function platform(op, blobs) {
  const dictated = dictateOp(op, blobs)
  if (dictated) return dictated
  switch (op.op) {
    case 'passkey_get': return navigator.credentials?.get ? passkeyGet(op, blobs) : REFUSED
    case 'passkey_create': return navigator.credentials?.create ? passkeyCreate(op, blobs) : REFUSED
    case 'push_register': return pushRegister(op, blobs)
    case 'push_unregister': return pushUnregister()
    default: return REFUSED
  }
}

/** Passkeys are switched off for the launch (the owner's call, 2026-10-10): on for a tab with `?passkeys=1` (kept for
 *  the tab session), which the tests use. */
export function passkeysOn() {
  try {
    const asked = new URLSearchParams(location.search).get('passkeys')
    if (asked !== null) { if (asked === '1') sessionStorage.setItem('trommi-passkeys', '1'); else sessionStorage.removeItem('trommi-passkeys') }
    return sessionStorage.getItem('trommi-passkeys') === '1' && typeof window.PublicKeyCredential === 'function'
  } catch { return false }
}
/** Whether this browser has Web Push at all (the core then offers the level). */
export const pushable = () => typeof window !== 'undefined' && 'serviceWorker' in navigator && 'PushManager' in window
