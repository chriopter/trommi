// room.ts: how a device comes to hold a room, and the `Client` over it: founding one, opening the stored one, joining
// by an invite link (spec/v1.md 12.1), signing in with the recovery code (8.4) and recovering with it (8.7). What
// account.ts and core-worker.ts call; they hold no key and no device themselves.
//
// Every function here returns a client that is NOT started (`client.start()` is the caller's). A step that fails
// leaves nothing stored: the device it made is closed and its stores are deleted, so the same step can be taken
// again. The recovery code is copied on the way in and the copy is overwritten when the step is over.
//
// Where the core, the device's store and the cache come from is one small record (`RoomEnv`). The product's is
// below: core-wasm.ts and store-idb.ts, loaded when first needed. Tests hand in another (the stand-in core over
// the real binding, stores in memory) through `roomsOn`; the product has no switch for that.
//
// The hub's address is not part of a device's state. It is kept in the cache (`client/room`, written durably when
// the room is made); a caller that knows it may pass `hub_url` to `openRoom` for a cache that was lost.
import { Client, ClientError, engineMeta } from './client.ts'
import type { Core, Device, PlacedCommit, ServedCommit, ServedEnvelope, Store } from './core-api.ts'
import { Engine } from './engine.ts'
import type { EngineTiming } from './engine.ts'
import { accountCopiesBytes, Hub, HubError } from './hub.ts'
import type { AccountCopies, NewAccount, RoomInSteps, RoomOnlyInSteps } from './hub.ts'
import { unhex } from './ids.ts'
import { halving, inSlices, SLICE } from './slices.ts'
import type { Cache } from './store-idb.ts'

export interface DeviceOptions {
  storage: { name: string }
  client?: string | null; device_name?: string; device_info?: unknown; fetch?: typeof fetch | null
  /** The store on `storage.name`, loaded: for a caller that holds its lock already (tabs.ts `adoptInTabs`). The
   *  device made over it is then not opened again in place when it closes itself: that caller takes over. */
  store?: Store
}
/** Where a room's parts come from. */
export interface RoomEnv {
  core(): Promise<Core>
  /** A new store object on the device state `name`; `load()` takes its lock. */
  store(name: string): Store
  cache(name: string): Promise<Cache>
  /** Deletes everything stored under `name`. */
  destroy(name: string): Promise<void>
  /** Left out in the product. Tests: another hub client (one that also hands an agent device the room group's
   *  Commits, which the fake hub's catch-up leaves out), and shorter waits. */
  hub?(opts: { hub_url: string; client_name: string | null; fetch?: typeof fetch }): Hub
  timing?: Partial<EngineTiming>
}
export interface Joining { check_code: Promise<string>; client: Promise<Client>; cancel(): void }
export interface Rooms {
  foundRoom(o: DeviceOptions & { hub_url: string; recovery_code?: Uint8Array; found_token?: string | null; account?: ((room_id: Uint8Array) => Record<string, unknown> | Promise<Record<string, unknown>>) | null }): Promise<{ client: Client; recovery_code: Uint8Array }>
  /** null: nothing is stored. */
  openRoom(o: DeviceOptions & { hub_url?: string }): Promise<Client | null>
  /** The client over a store that is loaded already (tabs.ts holds its lock): null when it holds no room. Its
   *  device is not opened again in place when it closes itself; the client says `device-closed` and tabs.ts takes over. */
  openRoomOver(o: DeviceOptions & { hub_url?: string }, store: Store): Promise<Client | null>
  /** `check_code`: the six numbers both sides compare, 'nn-nn-nn-nn-nn-nn'. */
  joinRoom(o: DeviceOptions & { link: string; poll_ms?: number; timeout_ms?: number }): Joining
  /** Signs in on a new device with the code (8.4). With `recover` the recovery of 8.7: every other human device is
   *  removed and the code replaced; `account` is called exactly once with the new code, before anything is posted,
   *  and gives the account's new sealed copies (null: a room without an account). */
  joinWithCode(o: DeviceOptions & { hub_url: string; room_id: string; code: Uint8Array; recover?: boolean; account?: ((new_code: Uint8Array) => Record<string, unknown> | null | Promise<Record<string, unknown> | null>) | null; progress?: ((step: SignInStep) => void) | null }): Promise<{ client: Client; sessions: Promise<SessionJoins> }>
}
/** How far a sign-in with the code is, for the screen that waits on it (a big account's check takes a while). */
export type SignInStep = { step: 'reading'; done?: number; total?: number } | { step: 'checking'; done: number; total: number } | { step: 'joining' }
/** The session groups a sign-in with the code joined behind it (`joinWithCode`'s `sessions`). */
export interface SessionJoins { joined: number; not_joined: { group: Uint8Array; code: string }[] }
/** How many session groups' Commits are read from the hub at a time while they are joined behind a sign-in. */
const SESSION_READS = 6
/** How often a session join that met `busy` is tried again (about half a minute in all). */
const BUSY_TRIES = 20

const sleep = (ms: number): Promise<void> => new Promise(r => setTimeout(r, ms))
/** A joining device's sign-in while it is no leaf yet: asked again after these waits, then every LEAF_WAIT_EVERY_MS, LEAF_WAIT_MS in all. */
const LEAF_WAITS_MS = [1000, 2000, 4000], LEAF_WAIT_EVERY_MS = 5000, LEAF_WAIT_MS = 5 * 60_000
/** A step of a join that did not reach the hub is asked again after these waits, then the join gives up (`offline`). */
const JOIN_RETRY_MS = [1000, 2000, 4000]
const sameBytes = (a: Uint8Array, b: Uint8Array): boolean => a.length === b.length && a.every((v, i) => v === b[i])
const codeText = (numbers: readonly number[]): string => numbers.map(n => String(n).padStart(2, '0')).join('-')

export function roomsOn(env: RoomEnv): Rooms {
  const hubOf = (o: DeviceOptions, hub_url: string): Hub => {
    const opts = { hub_url, client_name: o.client ?? null, ...(o.fetch ? { fetch: o.fetch } : {}) }
    return env.hub ? env.hub(opts) : new Hub(opts)
  }
  const timing = env.timing ? { timing: env.timing } : {}
  /** Where a device that closed itself is opened again: here, unless the caller holds the store's lock itself. */
  const again = (o: DeviceOptions): { store?: () => Store } => (o.store ? {} : { store: () => env.store(o.storage.name) })

  /** A new device in an empty store. A store that holds something is a room already: `room-exists`, nothing touched. */
  async function fresh(o: DeviceOptions, core: Core): Promise<Device> {
    const store = o.store ?? env.store(o.storage.name)
    let stored
    try { stored = await store.load() } catch (e) { await store.close?.(); throw e }
    if (stored.revision !== 0 || stored.entries.length) { await store.close?.(); throw new ClientError('room-exists', 'this browser holds an account already') }
    return core.createDevice(store)
  }
  /** Undoes a step that failed: the device it made holds nothing anyone else knows of. */
  async function discard(o: DeviceOptions, device: Device, engine: Engine | null): Promise<void> {
    if (engine) await engine.stop().catch(() => {}); else await device.close().catch(() => {})
    await env.destroy(o.storage.name).catch(() => {})
  }
  async function clientOf(o: DeviceOptions, core: Core, hub: Hub, engine: Engine, cache: Cache): Promise<Client> {
    return new Client({ core, hub, engine, cache, ...(o.device_name ? { device_name: o.device_name } : {}), ...(o.device_info !== undefined ? { device_info: o.device_info } : {}) }).open()
  }
  /** Posts everything in the outbox now, in order: a step's own posts, whose failure is the step's. An entry the
   *  hub did not answer is the same bytes again, a few times (the hub answers a repeated post like the first). */
  async function postAll(engine: Engine): Promise<void> {
    for (let tries = 0; (await engine.outbox()).length;) {
      try { await engine.postHead(); tries = 0 } catch (e) {
        if (!(e instanceof HubError) || !e.transient || ++tries > 4) throw e
        await sleep(300 * 2 ** tries)
      }
    }
  }
  /** Whether a failed post may have reached the hub all the same: its answer was lost, or the hub was busy. */
  const uncertain = (e: unknown): boolean => e instanceof HubError && e.transient

  async function foundRoom(o: Parameters<Rooms['foundRoom']>[0]): Promise<{ client: Client; recovery_code: Uint8Array }> {
    const core = await env.core()
    const hub = hubOf(o, o.hub_url)
    const device = await fresh(o, core)
    let engine: Engine | null = null, posting = false
    const code = o.recovery_code ? o.recovery_code.slice() : core.generateRecoveryCode()
    try {
      const room_id = await device.foundRoom(code, Date.now())
      // the account's sealed copies bind the room id: they are made now, before the founding is posted
      const account = o.account ? await o.account(room_id) as unknown as NewAccount : null
      const cache = await env.cache(o.storage.name)
      await cache.set('client/room', { hub_url: hub.hub_url }, { durable: true })
      engine = new Engine({ core, hub, meta: engineMeta(cache), ...again(o), ...timing }, device)
      const founding = (await engine.outbox()).find(e => e.kind === 'roomFounding') ?? (() => { throw new ClientError('internal', 'the core made no founding') })()
      await engine.attach(founding.id, { account, found_token: o.found_token ?? null })
      posting = true
      await postAll(engine)
      await engine.load()
      return { client: await clientOf(o, core, hub, engine, cache), recovery_code: code }
    } catch (e) {
      code.fill(0)
      // A founding the hub may have taken (its answers were lost) is NOT thrown away: the device and its outbox
      // stay, with the account that goes with them, and the next start of this stored room posts the same bytes
      // again. Everything else leaves nothing stored.
      if (posting && uncertain(e)) { if (engine) await engine.stop().catch(() => {}); else await device.close().catch(() => {}) }
      else await discard(o, device, engine)
      throw e
    }
  }

  async function over(o: DeviceOptions & { hub_url?: string }, store: Store, reopen: boolean): Promise<Client | null> {
    let stored
    try { stored = await store.load() } catch (e) { await store.close?.(); throw e }
    if (stored.revision === 0 && !stored.entries.length) { await store.close?.(); return null }
    const core = await env.core()
    const device = await core.openDevice(store)
    try {
      // a device that never came to hold a room (a join that was given up) is nothing to open
      if (!(await device.room()) && !(await device.groups()).length) { await device.close(); return null }
      const cache = await env.cache(o.storage.name)
      const kept = (await cache.get('client/room')) as { hub_url?: string } | undefined
      const hub_url = kept?.hub_url ?? o.hub_url ?? (() => { throw new ClientError('no-hub', 'this device no longer knows its hub\'s address') })()
      const hub = hubOf(o, hub_url)
      const engine = new Engine({ core, hub, meta: engineMeta(cache), ...(reopen ? { store: () => env.store(o.storage.name) } : {}), ...timing }, device)
      await engine.load()
      return await clientOf(o, core, hub, engine, cache)
    } catch (e) { await device.close().catch(() => {}); throw e }
  }

  function joinRoom(o: Parameters<Rooms['joinRoom']>[0]): Joining {
    const poll = o.poll_ms ?? 800, until = Date.now() + (o.timeout_ms ?? 15 * 60_000)
    let cancelled = false
    let tellCode: (code: string) => void = () => {}, failCode: (e: unknown) => void = () => {}
    const check_code = new Promise<string>((resolve, reject) => { tellCode = resolve; failCode = reject })
    check_code.catch(() => {})
    const wait = async (): Promise<void> => {
      if (cancelled) throw new ClientError('cancelled', 'joining was given up')
      if (Date.now() > until) throw new ClientError('invite-expired', 'nobody confirmed this device in time')
      await sleep(poll)
    }
    // The sign-in of a device that is no leaf yet is refused `not-member` until the inviter's Commit is in. It is
    // asked again after 1 s, 2 s and 4 s, then every 5 s, for five minutes in all (the hub's rule for this wait).
    // A caller that names `poll_ms` or `timeout_ms` (tests) gets its own pace and end for this wait as for the others.
    const paced = o.poll_ms !== undefined || o.timeout_ms !== undefined
    let asked = 0, leafUntil = 0
    const waitForLeaf = async (): Promise<void> => {
      if (paced) return wait()
      leafUntil ||= Date.now() + LEAF_WAIT_MS
      if (cancelled) throw new ClientError('cancelled', 'joining was given up')
      if (Date.now() > leafUntil) throw new ClientError('invite-expired', 'nobody confirmed this device in time')
      await sleep(LEAF_WAITS_MS[asked++] ?? LEAF_WAIT_EVERY_MS)
    }
    // A step that did not reach the hub (or met a busy one) is asked again, the same bytes, a few times before the
    // join gives up: a network blip never burns the invite. Posting the same Request twice is safe (the hub answers it
    // as the first time). What is still `offline` after that says so: the same link may be tried again.
    const steady = async <T>(step: () => Promise<T>): Promise<T> => {
      for (let tried = 0; ; tried++) {
        try { return await step() } catch (e) {
          const wait = paced ? poll : JOIN_RETRY_MS[tried]
          if (!(e instanceof HubError) || !e.transient || wait === undefined || tried >= JOIN_RETRY_MS.length) throw e
          if (cancelled) throw new ClientError('cancelled', 'joining was given up')
          if (Date.now() > until) throw e
          await sleep(e.retry_after !== null ? Math.min(e.retry_after * 1000, 10_000) : wait)
        }
      }
    }
    const client = (async (): Promise<Client> => {
      const core = await env.core()
      // the link held against this clock before anything is fetched: an expired one is refused here
      const checked = core.inviteLinkCheck(o.link, Date.now())
      const hub = hubOf(o, checked.hub)
      const device = await fresh(o, core)
      let engine: Engine | null = null
      try {
        const link = await device.joinLink(o.link, Date.now())
        const invite = await steady(() => hub.getInvite(link.inviteId))
        const request = await device.joinRequest(o.link, { offer: invite.offer, signature: invite.signature, mac: invite.mac }, Date.now())
        // (an earlier post whose answer was lost may have got through, and the inviter revealed it meanwhile: then the
        // hub calls the invite used. The Reveal below says whose Request it took; the core takes only its own.)
        let maybeIn = false
        await steady(async () => {
          try { return await hub.postInviteRequest(link.inviteId, { request: request.request, mac: request.mac, signature: request.signature }) } catch (e) {
            if (maybeIn && e instanceof HubError && e.code === 'invite-used') return null
            if (e instanceof HubError && e.transient) maybeIn = true
            throw e
          }
        })
        // the inviter's Reveal: with it both sides hold the same six numbers
        let reveal = null
        while (!reveal) {
          try { reveal = await hub.getReveal(link.inviteId) } catch (e) { if (!(e instanceof HubError) || (e.code !== 'not-found' && !e.transient)) throw e; await wait() }
        }
        tellCode(codeText([...(await device.joinReveal(reveal)).numbers]))
        // Let in once the person confirmed on the inviter's side: from then on the hub knows this device's key.
        hub.useSigner(link.roomId, (address, challenge) => device.hubSignIn(address, challenge))
        for (;;) {
          try { await hub.signIn(); break } catch (e) {
            if (!(e instanceof HubError) || (e.code !== 'not-member' && !e.transient)) throw e
            // "They don't match" burns the invite: the hub says so on the invite's own route
            try { await hub.getReveal(link.inviteId) } catch (burned) { if (burned instanceof HubError && burned.code === 'invite-burned') throw new ClientError('code-mismatch', 'the other device said the codes do not match') }
            await waitForLeaf()
          }
        }
        if (request.role === 'agent') {
          // an agent device follows the room group from the epoch the Offer names (12.1.6)
          const info = await steady(() => hub.groupInfo(core.roomGroupId(link.roomId), request.roomEpoch))
          await device.joinObserve(info.group_info)
        }
        const cache = await env.cache(o.storage.name)
        await cache.set('client/room', { hub_url: hub.hub_url }, { durable: true })
        engine = new Engine({ core, hub, meta: engineMeta(cache), ...again(o), ...timing }, device)
        await engine.load(link.roomId)
        // The Welcome: into the room group for a human device, into its session for an agent; committed by the inviter.
        const inside = (): boolean => (request.role === 'human' ? engine!.is_human : engine!.groups.some(g => g.session !== null))
        while (!inside()) { if (!(await steady(() => engine!.joinNow(request.inviter))) || !inside()) await wait() }
        return await clientOf(o, core, hub, engine, cache)
      } catch (e) {
        failCode(e)
        await discard(o, device, engine)
        throw e
      }
    })()
    client.catch(() => {})
    return { check_code, client, cancel() { cancelled = true } }
  }

  async function joinWithCode(o: Parameters<Rooms['joinWithCode']>[0]): Promise<{ client: Client; sessions: Promise<SessionJoins> }> {
    const core = await env.core()
    const room = unhex(o.room_id), code = o.code.slice()
    const hub = hubOf(o, o.hub_url)
    const tell = (p: SignInStep): void => { try { o.progress?.(p) } catch {} }
    let device: Device | null = null, engine: Engine | null = null
    // Once the hub took the join of the room group the device is a leaf of the room: it is never thrown away after
    // that (its leaf would stay behind at the hub, a device of the account that never comes). The stored room is
    // opened by the next start, which finishes what is left.
    let inRoom = false
    let sessions: Promise<SessionJoins> = Promise.resolve({ joined: 0, not_joined: [] })
    try {
      device = await fresh(o, core)
      const held = device
      // under the recovery key's token: it reads what a join needs and posts the join's Commits (12.3.2)
      hub.useSigner(room, async (address, challenge) => core.recoverySignIn(code, room, address, challenge))
      const cache = await env.cache(o.storage.name)
      await cache.set('client/room', { hub_url: hub.hub_url }, { durable: true })
      engine = new Engine({ core, hub, meta: engineMeta(cache), ...again(o), ...timing }, device)
      const tooLarge = (e: unknown): boolean => core.errorCode(e) === 'too-large'
      const size = (c: PlacedCommit): number => c.commit.commit.length
      // The room in steps: everything but the Commits at once, the Commits read from the hub's logs while the core
      // takes them in slices (a room's whole history may be more than one call carries).
      tell({ step: 'reading' })
      const anchorOf = (rows: Uint8Array[]): { group: Uint8Array; epoch: number } => core.recoveryAnchor(code, room, rows)
      // A recovery checks the room with its sessions (8.7 removes devices from them); a sign-in only the room group,
      // and each live session group after it is in (8.5): the person waits for the room group's history alone.
      let later: RoomOnlyInSteps | null = null
      if (o.recover) {
        const steps = await hub.roomInSteps({ anchorOf, read: (done, total) => tell({ step: 'reading', done, total }) })
        // The new code is made, and the account's copies of it (or the person's own copy: the caller's `account`
        // resolves once the code is on their screen), BEFORE the recovery is opened: the hub locks the room for
        // ten minutes from then on, and nothing of a recovery is posted for a code nobody holds.
        // The plan and the check the recovery builds on are two walks of the core; one reading of the logs feeds both.
        await held.recoveryPlanStart(code, steps.start)
        await held.codeCheckStart(code, steps.start)
        const planSlice = halving(async (slice: PlacedCommit[]) => { await held.recoveryPlanSlice(slice); return false }, tooLarge)
        const checkSlice = halving(async (slice: PlacedCommit[]) => { await held.codeCheckSlice(slice); return false }, tooLarge)
        await inSlices(steps.placed().next, size, async slice => { await planSlice(slice); await checkSlice(slice); return false })
        const plan = await held.recoveryPlanFinish(code, steps.end)
        let copies: Uint8Array
        try { copies = accountCopiesBytes(o.account ? await o.account(plan.newCode) as AccountCopies | null : null) } finally { plan.newCode.fill(0) }
        // A removed device's chain ends at its Cut for everyone (9.0.10). The core takes the Cuts itself, from
        // the chains as the hub serves them, each verified from number 1.
        const chains: ServedEnvelope[] = []
        for (const removal of plan.removals) for (const gone of removal.devices) {
          for (let after = 0, more = true; more;) {
            const page = await hub.chain(removal.group, gone, { after, limit: 500 })
            for (const link of page.items) { chains.push({ bytes: link.envelope, change: link.change, voidCode: link.void_code === null ? null : core.errorCodeFromText(link.void_code) ?? 'hub-voided-other' }); after = link.seq }
            more = page.more && page.items.length > 0
          }
        }
        // In the hub's ONE order across the removed devices, not chain after chain: the core judges each envelope
        // against what it was handed before, and a version one device wrote on another's needs that one first
        // (handed over chain by chain, such a version was `forbidden` on the recovered device for good).
        chains.sort((x, y) => x.change - y.change)
        const recovery = await hub.openRecovery()
        try {
          const done = await held.recoverChecked(code, steps.end, chains, copies, Date.now())
          for (const id of done.outbox) await engine.attach(id, { recovery_id: recovery.recovery_id })
          await postAll(engine)
          inRoom = true
        } catch (e) { await hub.dropRecovery(recovery.recovery_id).catch(() => {}); throw e }
      } else {
        const steps = later = await hub.roomOnlyInSteps({ anchorOf })
        // what is to be read is known from the room group's current epoch: its Commits up to it
        const total = steps.epoch
        let done = 0
        tell({ step: 'checking', done, total })
        await held.codeCheckStart(code, steps.start)
        await held.codeCheckDefer(steps.later)
        // (progress is told as the Commits are read, every few, not only once a slice of up to 256 was checked: a
        //  phone that reads slowly says it moves, and the sign-in's watch for silence is not tripped)
        const placed = steps.placed()
        let read = 0
        const next = async (): Promise<PlacedCommit | null> => { const c = await placed.next(); if (c && ++read % 16 === 0) tell({ step: 'checking', done: Math.max(done, read), total }); return c }
        await inSlices(next, size, halving(async slice => { await held.codeCheckSlice(slice); done += slice.length; tell({ step: 'checking', done: Math.max(done, read), total }); return false }, tooLarge))
        tell({ step: 'joining' })
        await held.joinRoomChecked(code, steps.end, Date.now())
        await postAll(engine)
        inRoom = true
      }
      await engine.load()
      // Its KeyPackages at once, under its own token now: a founding of a session claims one of every human device
      // of the room, and is refused (`not-found`) while one of them has none. Upkeep: the engine's timed run tries again.
      if (await held.keyPackagesToUpload(0, Date.now()).catch(() => null) !== null) await postAll(engine).catch(() => {})
      // what the room holds from before is read once the hub's order was taken: a recovery took the removed
      // devices' chains already, and their envelopes are then read back
      await engine.wantRescan()
      const client = await clientOf(o, core, hub, engine, cache)
      // The live session groups are joined behind the sign-in: a big account has many, each read and checked from
      // its founding, and the person waits for none of them. A session that is not joined here (the page went,
      // the hub refused) is added by another human device of the person when it sees it (engine `heal`).
      if (later && later.later.length) {
        const own = code.slice(), running = engine
        sessions = joinSessions(running, own, later).finally(() => own.fill(0))
        sessions.catch(() => {})
      }
      return { client, sessions }
    } catch (e) {
      if (device && inRoom) { if (engine) await engine.stop().catch(() => {}); else await device.close().catch(() => {}) }
      else if (device) await discard(o, device, engine)
      throw e
    } finally { code.fill(0) }
  }

  /**
   * The live session groups of a sign-in with the code, joined from outside after the device is in the room: each
   * session's Commits are read from the hub, several sessions at a time (`SESSION_READS`), and handed to the core
   * session by session in one job of the engine (its check, then its join), whose Commit the engine's pump posts.
   * Main sessions come first. Never throws: what was not joined is listed.
   */
  async function joinSessions(engine: Engine, code: Uint8Array, steps: RoomOnlyInSteps): Promise<SessionJoins> {
    const out: SessionJoins = { joined: 0, not_joined: [] }
    // each session: its GroupInfos and its Commits, read from the hub
    const read = async (at: number): Promise<{ founding: Uint8Array; current: Uint8Array; commits: ServedCommit[] }> => {
      const group = steps.later[at]!, infos = await engine.hub.sessionInSteps(group)
      const reader = engine.hub.groupCommits(group, infos.epoch), all: ServedCommit[] = []
      for (let c = await reader.next(); c; c = await reader.next()) all.push(c)
      return { founding: infos.founding, current: infos.current, commits: all }
    }
    const ahead: Promise<{ founding: Uint8Array; current: Uint8Array; commits: ServedCommit[] }>[] = []
    const fetch = (at: number): void => { if (at < steps.later.length && !ahead[at]) { ahead[at] = read(at); ahead[at]!.catch(() => {}) } }
    for (let at = 0; at < SESSION_READS; at++) fetch(at)
    for (const [at, group] of steps.later.entries()) {
      const session = { group }
      fetch(at + SESSION_READS)
      try {
        const { founding, current, commits } = await ahead[at]!
        delete ahead[at]
        const join = (): Promise<void> => engine.land(async d => {
          // (already a member: an earlier try went through)
          if ((await d.groups()).some(g => sameBytes(g.group, session.group))) return null
          const checked = await d.sessionCheckStart(founding)
          // (a founding GroupInfo of another group than the one listed: not this session)
          if (!sameBytes(checked, session.group)) throw new ClientError('bad-group', 'the hub served another group\'s founding')
          for (let i = 0; i < commits.length; i += SLICE.commits) {
            await halving(async (slice: ServedCommit[]) => { await d.sessionCheckSlice(checked, slice); return false }, e => engine.core.errorCode(e) === 'too-large')(commits.slice(i, i + SLICE.commits))
          }
          // its keys are selected now, under the room epochs its own verified Commits name (8.5)
          return await d.joinSessionCheckedKeys(code, checked, current, steps.end.rows, steps.end.links, Date.now())
        })
        // `busy`: a join of this group is on its way already (an earlier try, posted and waiting for its place in
        // the log), or another human device added this one meanwhile (5.2.7: its Welcome waits). The log and the
        // Welcomes are taken, and the join is tried again while the device is not in the group, a while.
        const member = async (): Promise<boolean> => (await engine.do(d => d.groups())).some(g => sameBytes(g.group, session.group))
        for (let tries = 0; ; tries++) {
          try { await join(); break } catch (e) {
            if (engine.core.errorCode(e) !== 'busy' || tries >= BUSY_TRIES) throw e
            await engine.catchUp().catch(() => {})
            await engine.joinNow(null).catch(() => false)
            if (await member()) break
            await sleep(Math.min(50 * 2 ** tries, 1000))
          }
        }
        out.joined++
      } catch (e) {
        const why = engine.core.errorCode(e) ?? (e instanceof HubError ? e.code : 'failed')
        out.not_joined.push({ group: session.group, code: String(why) })
      }
    }
    // the sessions' keys came with their joins, after the room's history was read: what it could not open then
    // is read again now
    if (out.joined) await engine.wantRescan().catch(() => {})
    return out
  }

  return {
    foundRoom, joinRoom, joinWithCode,
    openRoom: o => over(o, env.store(o.storage.name), true),
    openRoomOver: (o, store) => over(o, store, false),
  }
}

/** The product's parts: the WASM core and IndexedDB, loaded when a room function first needs them. */
const product: RoomEnv = (() => {
  const wasm = () => import('./core-wasm.ts')
  const idb = () => import('./store-idb.ts')
  return {
    core: async () => (await wasm()).loadCore(),
    store(name: string): Store {
      // the store's modules are loaded by `load()`, the first thing anyone calls on a store
      let inner: Promise<Store> | null = null
      const open = (): Promise<Store> => inner ??= (async () => { const [{ IdbStore }, { openDeviceStore }] = await Promise.all([wasm(), idb()]); return openDeviceStore({ name, IdbStore }) })()
      return { load: async () => (await open()).load(), apply: async write => (await open()).apply(write), close: async () => { if (inner) await (await inner).close?.() } }
    },
    cache: async name => (await idb()).openCache(name),
    destroy: async name => { const [{ IdbStore }, { deleteStores }] = await Promise.all([wasm(), idb()]); await deleteStores({ name, IdbStore }) },
  }
})()
export const { foundRoom, openRoom, openRoomOver, joinRoom, joinWithCode } = roomsOn(product)
