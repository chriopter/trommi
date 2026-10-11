// account-remote.ts: the account (account.ts) as the page uses it. Everything slow or secret runs in the core
// worker: creating an account, logging in, the Emergency Kit's ways back and joining with a link run in a new core
// worker (remote.ts), which then holds the room, and the page gets its RemoteClient; the account's routes of a
// signed-in device (a new kit, a new password, a passkey, …) run in the worker of that device's RemoteClient. The
// page's thread never loads the core: no key is derived on it and no sealed copy is opened on it.
//
// What stays on the page is light and holds no key (passwords.ts, passkey.ts): a form's own checks, a generated
// password, the hub's challenge for a passkey and the prf input. WebAuthn's prompts are the page's too
// (public/auth.mjs); these functions carry what a ceremony returned, the prf output with it, into the worker.
//
// A page without workers cannot run the core: every step that needs it fails with `worker-failed`, which the
// screens word.
import { accountInWorker, joinInWorker, type RemoteClient } from './remote.ts'
import type { AccountStatus, Kit, PasskeyRegistration, Unlock } from './account.ts'
import { PASSKEY_PRF_INPUT, passkeyChallenge as challengeFromHub } from './passkey.ts'

export { PASSWORD_MIN, normaliseEmail, passwordProblem, generatePassword, parseRecoveryWords } from './passwords.ts'
export { accountName, kitAddress, parseKitAddress } from './account-name.ts'

type Args = Record<string, unknown> & { client?: string | null }
type Opened = { client: RemoteClient }
/** A step of account.ts that makes this device's room (worker-protocol.ts ACCOUNT_OPENS). */
type Opens = 'createAccount' | 'createAccountWithPasskey' | 'loginWithPassword' | 'loginWithPasskey' | 'resetPassword' | 'recoverWithKit'

/** A step that makes a room, in a new core worker. `onEvent`: what the worker says before it is done; what it
 *  returns (or throws) is the page's answer to that event. */
async function opening<T extends Opened>(fn: Opens, args: Args, onEvent?: (event: string, data: unknown) => unknown, stall_ms?: number): Promise<T> {
  if (typeof Worker !== 'function') throw Object.assign(new Error('this browser runs no workers'), { code: 'worker-failed' })
  const options = { client: args.client ?? null, ...(onEvent ? { onEvent } : {}), ...(stall_ms ? { stall_ms } : {}) }
  return await accountInWorker(fn, args, options) as unknown as T
}
/** How far a sign-in with the code is (room.ts `SignInStep`), as the worker tells it. */
export type SignInStep = { step: 'reading' } | { step: 'checking'; done: number; total: number } | { step: 'joining' }
/** The words a step says on the sign-in's button: "Catching up 1,234 of 5,000…". */
export function signInWords(p: SignInStep): string {
  if (p.step === 'reading') return 'Reading your account…'
  if (p.step === 'joining') return 'Joining…'
  const n = (v: number): string => v.toLocaleString('en-US')
  return `Catching up ${n(Math.min(p.done, p.total))} of ${n(p.total)}…`
}
/** A sign-in that says nothing for this long is given up (`stalled`): it never spins for ever. */
export const SIGN_IN_STALL_MS = 120_000
/** A step that signs in with the code: its progress goes to `onStep`, and silence for `SIGN_IN_STALL_MS` ends it. */
const signingIn = <T extends Opened>(fn: Opens, args: Args, onStep?: ((step: SignInStep) => void) | null): Promise<T> =>
  opening<T>(fn, args, (event, data) => { if (event === 'signin-progress') onStep?.(data as SignInStep) }, SIGN_IN_STALL_MS)
export const createAccount = (args: Args) => opening<Opened & { kit: Kit }>('createAccount', args)
export const createAccountWithPasskey = (args: Args) => opening<Opened & { kit: Kit }>('createAccountWithPasskey', args)
export const loginWithPassword = (args: Args, onStep?: ((step: SignInStep) => void) | null) => signingIn<Opened>('loginWithPassword', args, onStep)
export const loginWithPasskey = (args: Args, onStep?: ((step: SignInStep) => void) | null) => signingIn<Opened>('loginWithPasskey', args, onStep)
export const recoverWithKit = (args: Args, onStep?: ((step: SignInStep) => void) | null) => signingIn<Opened>('recoverWithKit', args, onStep)
export const resetPassword = (args: Args, onStep?: ((step: SignInStep) => void) | null) => signingIn<Opened & { kit: Kit }>('resetPassword', args, onStep)
// The light parts of a passkey ceremony, on the page beside navigator.credentials (passkey.ts).
export const passkeyChallenge = (args: { hub_url: string; client?: string | null }): Promise<{ challenge: string; account: string; user_handle: string }> => challengeFromHub(args)
export const passkeyPrfInput = async (): Promise<Uint8Array<ArrayBuffer>> => PASSKEY_PRF_INPUT

/** A route of the signed-in device's account, in its worker (worker-protocol.ts ACCOUNT_CALLS). */
const onClient = <T>(fn: string, client: RemoteClient, args?: unknown): Promise<T> => client.call('account', fn, args) as Promise<T>
export const accountStatus = (client: RemoteClient) => onClient<AccountStatus | null>('accountStatus', client)
export const makeEmergencyKit = (client: RemoteClient, args: Unlock) => onClient<Kit>('makeEmergencyKit', client, args)
export const changePassword = (client: RemoteClient, args: { current: string; next: string }) => onClient<{ kit: Kit | null }>('changePassword', client, args)
export const setPassword = (client: RemoteClient, args: { unlock: Unlock; next: string }) => onClient<{ kit: Kit | null }>('setPassword', client, args)
export const checkUnlock = (client: RemoteClient, args: Unlock) => onClient<void>('checkUnlock', client, args)
export const passkeyChallengeFor = (client: RemoteClient) => onClient<string>('passkeyChallengeFor', client)
export const addPasskey = (client: RemoteClient, args: { unlock: Unlock; passkey: PasskeyRegistration }) => onClient<{ credential_id: string; kit: Kit | null }>('addPasskey', client, args)
export const removePasskey = (client: RemoteClient, args: { credential_id: string; unlock: Unlock }) => onClient<void>('removePasskey', client, args)
export const setEmail = (client: RemoteClient, args: { email: string; words: string }) => onClient<Kit>('setEmail', client, args)
export const replaceRecoveryCode = (client: RemoteClient, args: Unlock) => onClient<Kit>('replaceRecoveryCode', client, args)

/** Join with an invite link (room.ts joinRoom's shape), in a new core worker. */
export function joinRoom(args: Args): { check_code: Promise<string>; client: Promise<RemoteClient>; cancel(): void } {
  return joinInWorker(args, { client: args.client ?? null })
}
