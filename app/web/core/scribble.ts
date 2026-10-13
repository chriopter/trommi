// scribble.ts: the Scribble Board without DOM (spec/v1.md section 10): the shapes the app draws, the merge that
// turns a board's items into shapes, and the snapshot file. The app's Scribble Board (app/web/public/whiteboard.mjs
// openCanvas) drives it with the client; ink.ts packs a stroke's points, palette.ts names its colours, and the end of
// this file knows a board item's body on the wire (the page has no core; see there).
//
// Three forms of one shape, each with one job:
//   wire shape   what an item's body and the snapshot file carry (10.5): whole numbers of 1/16 board unit, base64url
//                ids. reduceBoard merges these and nothing else.
//   entry        the same in the model's numbers and ids (board units, hex): what a timeline item's content holds
//                and what the app writes (entryOf).
//   Shape        the board's own form of one element, its points unpacked: { id, by, tool, pts, z, group, … }.
//     stroke: pts = its points, t, f, az, al, sim (ink.ts), color, width
//     note:   pts = [x, y], text, size, color, wrap        picture: pts = [x0, y0, x1, y1], attachment, nw, nh, mime, name
//
// A shape's id is never read from a body: it is `<sender>/<envelope number>/<index>` from the signed header of the
// item that adds it (10.1), so nobody can name or collide with another writer's shapes.
//
// THE MERGE IS ONE FUNCTION, reduceBoard(snapshot, items): it is trommi-core's `board_items::Board::apply` (10.7) in
// TypeScript, on the wire's own form, and the binding's reducer takes its place there once it is exported
// (core-api.ts ProvisionalStateless.boardReduce). CanvasState is only the adapter between that function and the app: it converts the
// model's items to the wire form, calls reduceBoard, and keeps the Shapes of what changed.

import { idFromHex, idToHex } from './ids.ts'
import { packPoints, unpackPoints, quantum, wrapAdd, Q, STROKE_TOOLS } from './ink.ts'
import type { Ink } from './ink.ts'
import type { AttachmentRef } from './types.ts'
export * from './ink.ts'       // one import for the app: the format, the shape and the palette
export * from './palette.ts'

// ---- which board: one per desk, and one for "All desks" (10.1) ----
// A board is the timeline desk/<32 hex>. A Desk's id is 16 random bytes and is its board's id. The app also names
// two places that are no Desk register: 'main' (a room that has no Desk yet) and "All desks"; deskBoard gives each
// the same board on every client:
//   a desk            deskBoard(desk id): an id of 32 hex is taken as it is; any other name is folded into 16 bytes:
//                     its UTF-8 XORed by position (byte i onto i mod 16), then the length XORed onto the last byte.
//                     'main' -> 6d61696e000000000000000000000004.
//   "All desks"       ALL_BOARD, the id of spec 10.1 (the fold of 'all-desks'), never a desk's.
/** The Scribble Board of a desk: its timeline id (see above). */
export function deskBoard(desk: string | null | undefined): string {
  const id = String(desk || 'main')
  if (/^[0-9a-f]{32}$/.test(id)) return `desk/${id}`
  const bytes = new TextEncoder().encode(id), out = new Uint8Array(16)
  bytes.forEach((v, i) => { out[i % 16]! ^= v })
  out[15]! ^= bytes.length & 0xff
  return `desk/${[...out].map(b => b.toString(16).padStart(2, '0')).join('')}`
}
/** The board of a room without desks. */
export const MAIN_BOARD = 'desk/6d61696e000000000000000000000004'
/** The board of "All desks". */
export const ALL_BOARD = 'desk/616c6c2d6465736b7300000000000009'

// ---- shapes ----
/** A shape on the board (see above): every tool's fields, the ones its tool does not use left out. */
export interface Shape {
  id: string; by: string; tool: string; z: number; group: string | null
  pts: number[]
  t?: number[]; f?: number[]; az?: number[] | null; al?: number[] | null; sim?: boolean
  color?: string | null; width?: number
  text?: string; size?: number; wrap?: number | null
  attachment?: AttachmentRef; nw?: number; nh?: number; mime?: string | null; name?: string | null
  /** A stroke still being drawn by someone else (pieces, spec 7.2): its finished form replaces it. */
  live?: boolean
  [field: string]: unknown
}
/** A shape as a strokes item's content carries it in the model (board units). */
export type Entry = Record<string, any>

const STROKES = new Set<unknown>(STROKE_TOOLS)
const WORDS = new Set<unknown>(['text', 'voice', 'sticky'])   // the tools that carry text and wrap
const TOOLS = new Set<unknown>([...STROKES, ...WORDS, 'image'])
export const WIDTH: Readonly<Record<string, number>> = Object.freeze({ pen: 4, marker: 18 })   // a stroke's default width
const str = (v: unknown, max = 200): string | null => (typeof v === 'string' ? v.slice(0, max) : null)
const num = (v: unknown, d = 0): number => (typeof v === 'number' && Number.isFinite(v) ? v : d)
const r2 = (v: number): number => Math.round(v * 100) / 100
const nums = (v: unknown, n: number): number[] | null => (Array.isArray(v) && v.length === n && v.every(Number.isFinite) ? v as number[] : null)
export const isStroke = (s: { tool?: unknown } | null | undefined): boolean => STROKES.has(s?.tool)
export const inkOf = (s: Shape): Ink => ({ pts: s.pts, t: s.t!, f: s.f!, az: s.az ?? null, al: s.al ?? null, sim: Boolean(s.sim) })

/** A shape -> an entry (for a strokes item's content). */
export function entryOf(s: Shape): Entry {
  const e: Entry = { tool: s.tool }
  if (isStroke(s)) Object.assign(e, { color: s.color ?? (s.tool === 'marker' ? 'yellow' : 'ink'), width: r2(num(s.width, WIDTH[s.tool])), points: packPoints(inkOf(s)) })
  else if (WORDS.has(s.tool)) {
    Object.assign(e, { at: [r2(s.pts[0]!), r2(s.pts[1]!)], text: s.text ?? '', size: num(s.size, 20), color: s.color ?? 'ink' })
    if (s.wrap != null) e.wrap = s.wrap
  } else if (s.tool === 'image') {
    Object.assign(e, { rect: s.pts.slice(0, 4).map(r2), attachment: s.attachment })
    for (const k of ['nw', 'nh', 'mime', 'name']) if (s[k] != null) e[k] = s[k]
  }
  if (s.z != null) e.z = s.z
  if (s.group) e.group = s.group
  return e
}
/** An entry -> a shape (null when it is not one this app can show). */
export function shapeOf(e: Entry | null | undefined, id: string, by: string): Shape | null {
  const tool = e?.['tool']
  if (!e || !TOOLS.has(tool)) return null
  const s: Shape = { id, by, tool, z: num(e['z']), group: str(e['group'], 80), pts: [] }
  if (STROKES.has(tool)) {
    const ink = unpackPoints(e.points)
    if (!ink) return null
    const w = num(e.width, WIDTH[tool])
    return Object.assign(s, ink, { color: str(e.color, 40), width: w > 0 && w <= 1000 ? w : WIDTH[tool] })
  }
  if (WORDS.has(tool)) {
    const at = nums(e.at, 2)
    if (!at) return null
    return Object.assign(s, { pts: [...at], text: String(e.text ?? '').slice(0, 20000), size: num(e.size, 20), color: str(e.color, 40), wrap: Number.isFinite(e.wrap) ? e.wrap : null })
  }
  const rect = nums(e.rect, 4)
  if (!rect || !e.attachment?.attachment_id) return null
  return Object.assign(s, { pts: [...rect], attachment: e.attachment, nw: num(e.nw, 0), nh: num(e.nh, 0), mime: str(e.mime, 80), name: str(e.name) })
}

// ---- the merge (10.7), on the wire's form ----

/** A shape as the wire carries it (10.5), in a board's state with its `id`. */
/** A body's fields. */
export type Fields = Record<string, unknown>
export type WireShape = Fields
/** One board item as the merge takes it: sender (base64url) and number from the signed header, the body's payload
 *  as the core opened it. `unread`: the item stands in its writer's chain but could not be read here. */
export interface BoardItem { sender: string; seq: number; hash: string | null; payload: string | null; unread?: 'newer' | 'no_key' }
/**
 * What a board's items add up to, in the shape of the snapshot file (10.8) so that it is its own input:
 *   shapes    every shape on the board with its id, where its moves have put it, ascending by id
 *   frontier  per writer the number and hash of its last envelope taken into account
 *   gone      ids erased before their shape arrived (beyond their writer's number), ascending
 *   moved     [id, dx, dy]: the summed moves of such ids, ascending
 *   unread    an item could not be read: this state is not all of the board and no snapshot is written from it
 */
export interface BoardState { v: 3; shapes: WireShape[]; frontier: Record<string, [number, string | null]>; gone: string[]; moved: [string, number, number][]; unread?: 'newer' | 'no_key' | null }

const senderHex = new Map<string, string>()
/** A shape id's parts, the sender as hex: shape ids are ordered by sender bytes, number, index. */
function idParts(id: string): [string, number, number] {
  const a = id.indexOf('/'), b = id.lastIndexOf('/')
  const sender = id.slice(0, a)
  let hex = senderHex.get(sender)
  if (hex === undefined) { hex = idToHex(sender); senderHex.set(sender, hex) }
  return [hex, Number(id.slice(a + 1, b)), Number(id.slice(b + 1))]
}
function cmpId(x: string, y: string): number {
  const a = idParts(x), b = idParts(y)
  return (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0) || a[1] - b[1] || a[2] - b[2]
}
/** A wire shape moved by an offset in 1/16 units, modulo 2^32: a new object, the old one untouched. */
function shifted(s: WireShape, dx: number, dy: number): WireShape {
  if (s['points'] !== undefined) {
    const ink = unpackPoints(s['points'])!
    for (let i = 0; i < ink.pts.length; i += 2) { ink.pts[i] = wrapAdd(quantum(ink.pts[i]), dx) / Q; ink.pts[i + 1] = wrapAdd(quantum(ink.pts[i + 1]), dy) / Q }
    return { ...s, points: packPoints(ink) }
  }
  if (s['at'] !== undefined) { const [x, y] = s['at'] as number[]; return { ...s, at: [wrapAdd(x!, dx), wrapAdd(y!, dy)] } }
  const [x0, y0, x1, y1] = s['rect'] as number[]
  return { ...s, rect: [wrapAdd(x0!, dx), wrapAdd(y0!, dy), wrapAdd(x1!, dx), wrapAdd(y1!, dy)] }
}

/**
 * THE MERGE (10.7). `snapshot`: the board so far (null: empty), `items`: more items, each writer's in the order of
 * its chain. Returns the board after them; the input is not changed, and a shape no item touched is the same
 * object in the result. Adding happens once per id, erasing wins for good, moves add up; all three commute, so two
 * devices that applied the same items hold the same board. An item its writer's number already covers is skipped; a
 * body trommi-core would refuse adds nothing and leaves the writer's number where it was.
 */
export function reduceBoard(snapshot: BoardState | null, items: readonly BoardItem[]): BoardState {
  const shapes = new Map<string, WireShape>()
  for (const s of snapshot?.shapes ?? []) shapes.set(s['id'] as string, s)
  const applied = new Map<string, [number, string | null]>(Object.entries(snapshot?.frontier ?? {}))
  const gone = new Set<string>(snapshot?.gone ?? [])
  const moved = new Map<string, [number, number]>((snapshot?.moved ?? []).map(([id, dx, dy]) => [id, [dx, dy]]))
  let unread = snapshot?.unread ?? null
  const added: string[] = []
  const seqOf = (sender: string): number => applied.get(sender)?.[0] ?? 0
  const sender = (id: string): string => id.slice(0, id.indexOf('/'))
  const covers = (id: string): boolean => idParts(id)[1] <= seqOf(sender(id))
  const advance = (by: string, seq: number, hash: string | null): void => {
    applied.set(by, [seq, hash])
    for (const id of gone) if (sender(id) === by && idParts(id)[1] <= seq) gone.delete(id)
    for (const id of moved.keys()) if (sender(id) === by && idParts(id)[1] <= seq) moved.delete(id)
  }
  for (const item of items) {
    const by = item.sender, seq = item.seq
    if (!Number.isSafeInteger(seq) || seq < 1 || seq <= seqOf(by)) continue
    if (item.unread) { advance(by, seq, item.hash); unread = unread === 'newer' || item.unread === 'newer' ? 'newer' : 'no_key'; continue }
    const body = item.payload === null ? 'bad' : parseBoardItem(item.payload)
    if (body === 'newer_schema') { advance(by, seq, item.hash); unread = 'newer'; continue }
    if (body === 'bad') continue
    const here = (id: string): boolean => covers(id) || (sender(id) === by && idParts(id)[1] <= seq)
    if (body['content_type'] === 'strokes') {
      ;(body['strokes'] as WireShape[]).forEach((shape, index) => {
        const id = `${by}/${seq}/${index}`
        const offset = moved.get(id)
        moved.delete(id)
        if (gone.delete(id)) return
        shapes.set(id, offset ? { ...shifted(shape, offset[0], offset[1]), id } : { ...shape, id })
        added.push(id)
      })
    } else if (body['content_type'] === 'move') {
      const [dx, dy] = body['offset'] as [number, number]
      for (const id of body['shape_ids'] as string[]) {
        if (here(id)) { const s = shapes.get(id); if (s) shapes.set(id, shifted(s, dx, dy)) }
        else if (!gone.has(id)) {
          const [x, y] = moved.get(id) ?? [0, 0]
          const sum: [number, number] = [wrapAdd(x, dx), wrapAdd(y, dy)]
          if (sum[0] || sum[1]) moved.set(id, sum); else moved.delete(id)
        }
      }
    } else {
      for (const id of body['shape_ids'] as string[]) {
        if (here(id)) shapes.delete(id)
        else { moved.delete(id); gone.add(id) }
      }
    }
    advance(by, seq, item.hash)
  }
  // Ascending by id: what the snapshot held is in order already, the few new ids are put in their places.
  const order = (snapshot?.shapes ?? []).map(s => s['id'] as string).filter(id => shapes.has(id))
  for (const id of added.sort(cmpId)) {
    if (!shapes.has(id)) continue
    let lo = 0, hi = order.length
    while (lo < hi) { const mid = (lo + hi) >> 1; if (cmpId(order[mid]!, id) < 0) lo = mid + 1; else hi = mid }
    if (order[lo] !== id) order.splice(lo, 0, id)
  }
  return {
    v: 3, shapes: order.map(id => shapes.get(id)!), frontier: Object.fromEntries(applied),
    gone: [...gone].sort(cmpId), moved: [...moved].sort((a, b) => cmpId(a[0], b[0])).map(([id, [dx, dy]]) => [id, dx, dy]), unread,
  }
}

// ---- the app's adapter ----

/** One board item as the app hands it over: a timeline item of the model (ids hex, content in the model's form). */
export interface CanvasItem { sender_device_id: string; sender_sequence: number | null | undefined; envelope_hash?: string | null; envelope_number?: number | null; content?: Record<string, any> | null; item_state?: string | null }
/** A snapshot between CanvasState and packSnapshot / unpackSnapshot: the file's content (10.8) with the frontier in
 *  the model's ids (as the register `scribble_snapshot/<timeline>` names it), and the newest change number seen. */
export interface CanvasSnapshot { v: 3; shapes: WireShape[]; frontier: Record<string, [number, string | null]>; gone: string[]; moved: [string, number, number][]; last_envelope_number: number }

const failure = (code: string, message: string): Error => Object.assign(new Error(message), { code })
const frontierTo = (f: Record<string, [number, string | null]>, id: (text: string) => string): Record<string, [number, string | null]> =>
  Object.fromEntries(Object.entries(f).map(([k, [seq, hash]]) => [id(k), [seq, hash == null ? null : id(hash)]]))
const UNREADABLE: readonly unknown[] = ['newer_schema', 'undecryptable', 'pruned']

export class CanvasState {
  /** Every shape on the board, by its id in the model's form (`<sender hex>/<number>/<index>`). */
  shapes = new Map<string, Shape>()
  last_envelope_number = 0
  /** Items applied since the last snapshot (written or loaded). */
  applied = 0
  #board: BoardState = reduceBoard(null, [])
  #wire = new Map<string, WireShape>()                 // wire id -> the wire shape the Shape was made from
  #refs = new Map<string, AttachmentRef>()             // attachment id -> the reference as an item of the model named it
  covered(sender: string, seq: number): boolean { return (this.#board.frontier[idFromHex(sender)]?.[0] ?? 0) >= seq }
  /** Why this board is not all of the board (an item could not be read), or null. */
  get unread(): 'newer' | 'no_key' | null { return this.#board.unread ?? null }

  /** Apply one item. Returns the ids it changed (added, moved, gone), or null when it was skipped. A stroke piece
   *  (content_type 'stroke_piece') grows the live stroke `live:<sender>/<stroke>` and is no item of the board. */
  apply(item: CanvasItem): Set<string> | null {
    const by = item.sender_device_id, seq = item.sender_sequence, c = item.content ?? null
    if (c?.['content_type'] === 'stroke_piece') return this.#piece(by, c)
    if (!/^[0-9a-f]{64}$/.test(by ?? '') || typeof seq !== 'number' || !Number.isSafeInteger(seq) || seq < 1 || this.covered(by, seq)) return null
    const unread = UNREADABLE.includes(item.item_state)
    // A picture keeps the reference its item named (what the app added to it beside the wire's fields stays).
    for (const e of c?.['content_type'] === 'strokes' && Array.isArray(c['strokes']) ? c['strokes'] : []) if (typeof e?.attachment?.attachment_id === 'string') this.#refs.set(e.attachment.attachment_id, e.attachment)
    let payload: string | null = null
    if (!unread) try { payload = encodeBoardItem(c ?? {}) } catch { payload = null }
    const next = reduceBoard(this.#board, [{ sender: idFromHex(by), seq, hash: item.envelope_hash ? idFromHex(item.envelope_hash) : null, payload, ...(unread ? { unread: item.item_state === 'newer_schema' ? 'newer' as const : 'no_key' as const } : {}) }])
    const changed = this.#take(next)
    // The finished form of a stroke replaces what was shown of its pieces.
    for (const e of c?.['content_type'] === 'strokes' && Array.isArray(c['strokes']) ? c['strokes'] : []) if (e?.live && this.shapes.delete(`live:${by}/${e.live}`)) changed.add(`live:${by}/${e.live}`)
    if (item.envelope_number) this.last_envelope_number = Math.max(this.last_envelope_number, item.envelope_number)
    this.applied++
    return changed
  }
  #piece(by: string, c: Record<string, any>): Set<string> | null {
    const more = unpackPoints(c['points'])
    if (!more || !STROKES.has(c['tool'])) return null
    const id = `live:${by}/${c['stroke']}`
    const head = this.shapes.get(id)
    if (!head) this.shapes.set(id, { id, by, tool: c['tool'], z: 0, group: null, live: true, ...more, color: str(c['color'], 40), width: num(c['width'], WIDTH[c['tool']]) })
    else {
      head.pts.push(...more.pts); head.t!.push(...more.t); head.f!.push(...more.f)
      if (head.az && more.az) { head.az.push(...more.az); head.al!.push(...more.al!) } else head.az = head.al = null
    }
    return new Set([id])
  }
  /** Take a new board state: the Shapes of what differs are made again. Returns the changed ids (the model's form). */
  #take(next: BoardState): Set<string> {
    const changed = new Set<string>()
    const seen = new Set<string>()
    for (const s of next.shapes) {
      const wire = s['id'] as string
      seen.add(wire)
      const had = this.#wire.get(wire)
      if (had === s || (had && JSON.stringify(had) === JSON.stringify(s))) continue
      this.#wire.set(wire, s)
      const id = shapeIdFromWire(wire)!
      const shape = this.#shape(s, id)
      if (shape) this.shapes.set(id, shape); else this.shapes.delete(id)
      changed.add(id)
    }
    if (seen.size !== this.#wire.size) for (const wire of [...this.#wire.keys()]) if (!seen.has(wire)) {
      this.#wire.delete(wire)
      const id = shapeIdFromWire(wire)!
      if (this.shapes.delete(id)) changed.add(id)
    }
    this.#board = next
    return changed
  }
  #shape(s: WireShape, id: string): Shape | null {
    const { id: _id, ...body } = s
    const entry = entryFromWire(body)
    if (entry['attachment']) entry['attachment'] = this.#refs.get((entry['attachment'] as AttachmentRef).attachment_id) ?? entry['attachment']
    return shapeOf(entry, id, id.slice(0, id.indexOf('/')))
  }

  /** The snapshot of the board as it is now (10.2). Throws ('newer-version', 'no-key') when an item could not be
   *  read, and ('bad-format') when a writer's last item has no hash yet. */
  snapshot(): CanvasSnapshot {
    const b = this.#board
    if (b.unread) throw failure(b.unread === 'newer' ? 'newer-version' : 'no-key', 'an item of this board could not be read: no snapshot is written from it')
    if (Object.values(b.frontier).some(([, hash]) => hash == null)) throw failure('bad-format', 'an item of this board has no envelope hash yet')
    return { v: 3, shapes: b.shapes, frontier: frontierTo(b.frontier, idToHex), gone: b.gone, moved: b.moved, last_envelope_number: this.last_envelope_number }
  }
  /** Shapes of a snapshot, ascending as the file has them (in slices, for a big one: load(snap, { shapes: false }) first). */
  addShapes(entries: Iterable<WireShape | null | undefined>): void {
    const more = [...entries].filter((e): e is WireShape => typeof e?.['id'] === 'string' && !this.#wire.has(e['id']))
    this.#take({ ...this.#board, shapes: [...this.#board.shapes, ...more] })
  }
  /** Start from a snapshot (a fresh client). Replaces what is here. */
  load(snap: Partial<CanvasSnapshot> | null | undefined, { shapes = true }: { shapes?: boolean } = {}): void {
    this.shapes.clear(); this.#wire.clear()
    this.#board = { v: 3, shapes: [], frontier: frontierTo(snap?.frontier ?? {}, idFromHex), gone: [...(snap?.gone ?? [])], moved: [...(snap?.moved ?? [])], unread: null }
    if (shapes) this.addShapes(snap?.shapes ?? [])
    this.last_envelope_number = num(snap?.last_envelope_number)
    this.applied = 0
  }
}

// ---- the snapshot file (10.8): gzip'd JSON { v: 3, shapes, frontier, gone, moved } ----
const MAX_SNAPSHOT_BYTES = 64 * 1024 * 1024
async function gzip(bytes: Uint8Array<ArrayBuffer>): Promise<Uint8Array<ArrayBuffer>> {
  return new Uint8Array(await new Response(new Blob([bytes]).stream().pipeThrough(new CompressionStream('gzip'))).arrayBuffer())
}
/** Decompressed, and never more than the largest snapshot: whoever decompresses the file enforces the limit. */
async function gunzip(bytes: Uint8Array<ArrayBuffer>): Promise<Uint8Array> {
  const reader = new Blob([bytes]).stream().pipeThrough(new DecompressionStream('gzip')).getReader()
  const parts: Uint8Array[] = []
  let size = 0
  for (;;) {
    const { done, value } = await reader.read()
    if (done) break
    size += value.length
    if (size > MAX_SNAPSHOT_BYTES) { await reader.cancel(); throw failure('too-large', 'a board snapshot above 64 MiB') }
    parts.push(value)
  }
  const out = new Uint8Array(size)
  let at = 0
  for (const p of parts) { out.set(p, at); at += p.length }
  return out
}
/** A snapshot -> the bytes of its file. */
export function packSnapshot(snap: CanvasSnapshot): Promise<Uint8Array<ArrayBuffer>> {
  const file = { v: 3, shapes: snap.shapes, frontier: frontierTo(snap.frontier, idFromHex), gone: snap.gone, moved: snap.moved }
  return gzip(new TextEncoder().encode(JSON.stringify(file)))
}
/**
 * The bytes of a snapshot file -> the snapshot, checked as 10.8 says: every shape readable and within the frontier,
 * ids ascending and none twice, nothing in `gone` or `moved` that the frontier covers, no move by nothing. Throws
 * ('bad-format'; 'newer-version' for a `v` above 3). `frontier`: the register's (the model's ids); a file whose own
 * differs is refused.
 */
export async function unpackSnapshot(bytes: Uint8Array<ArrayBuffer>, frontier?: Record<string, [number, string | null]> | null): Promise<CanvasSnapshot> {
  const refuse = (what: string): never => { throw failure('bad-format', `board snapshot: ${what}`) }
  const file = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(await gunzip(bytes))) as Fields
  if (!file || typeof file !== 'object' || !Number.isInteger(file['v'])) refuse('not a snapshot')
  if ((file['v'] as number) > 3) throw failure('newer-version', 'a board snapshot of a newer Trommi')
  if (file['v'] !== 3 || !Array.isArray(file['shapes']) || !Array.isArray(file['gone']) || !Array.isArray(file['moved']) || !file['frontier'] || typeof file['frontier'] !== 'object') refuse('not a snapshot')
  const heads = file['frontier'] as Record<string, [number, string | null]>
  for (const [sender, head] of Object.entries(heads)) if (idToHex(sender).length !== 64 || !Array.isArray(head) || !Number.isSafeInteger(head[0]) || head[0] < 1 || typeof head[1] !== 'string' || idToHex(head[1]).length !== 64) refuse('a frontier entry is not readable')
  const covered = (id: string): boolean => idParts(id)[1] <= (heads[id.slice(0, id.indexOf('/'))]?.[0] ?? 0)
  const ascending = (ids: string[]): void => { for (let i = 0; i < ids.length; i++) { if (!shapeIdFromWire(ids[i])) refuse('a shape id is not readable'); if (i && cmpId(ids[i - 1]!, ids[i]!) >= 0) refuse('ids are not ascending') } }
  const shapes = file['shapes'] as WireShape[]
  try { for (const s of shapes) checkWireShape(s, true) } catch { refuse('a shape is not readable') }
  ascending(shapes.map(s => s['id'] as string))
  if (shapes.some(s => !covered(s['id'] as string))) refuse('a shape beyond the frontier')
  const gone = file['gone'] as string[], moved = file['moved'] as [string, number, number][]
  if (moved.some(m => !Array.isArray(m) || m.length !== 3 || !Number.isInteger(m[1]) || !Number.isInteger(m[2]) || (!m[1] && !m[2]))) refuse('a move is not readable')
  ascending(gone); ascending(moved.map(m => m[0]))
  if ([...gone, ...moved.map(m => m[0])].some(covered) || moved.some(m => gone.includes(m[0]))) refuse('an entry the frontier covers')
  const mine = frontierTo(heads, idToHex)
  if (frontier && JSON.stringify(Object.entries(mine).sort()) !== JSON.stringify(Object.entries(frontier).sort())) refuse('its frontier is not the register\'s')
  for (const s of shapes) if (s['attachment']) pictureFromWire(s['attachment'])
  return { v: 3, shapes, frontier: mine, gone, moved, last_envelope_number: 0 }
}

/** Split a list of ids into chunks that keep an item well under its limits (2 000 ids, the payload's size). */
export function chunks<T>(list: T[], size = 400): T[][] {
  const out: T[][] = []
  for (let i = 0; i < list.length; i += size) out.push(list.slice(i, i + size))
  return out
}

// ---- a board item's body on the wire (10.5) ------------------------------------------------------------------------
//
// THE PAGE HAS NO CORE: this module runs in the page that draws the board (gen/vendor/scribble.mjs), where the
// WebAssembly core is not loaded, and the merge above works on the wire's form. So the rules of a board item's body
// stand here once more, for the page: they are the client core's (core/client/src/codec.rs: `check_wire_shape`,
// `entry_from_wire`, `entry_to_wire`, the board items of `decode_body` and `encode_body`), which every other reader
// uses (the worker's model reads board items through the core), and they go when the merge runs in the core
// (core-api.ts boardReduce). A change of the rules is made in both, and the tests of both hold them to each other
// (tests/web/model/scribble.test.mjs, core/client/src/codec_tests.rs).

const bad = (message: string): never => { throw failure('bad-format', message) }
const isObject = (v: unknown): v is Fields => !!v && typeof v === 'object' && !Array.isArray(v)
/** A wire id of exactly `bytes` bytes as hex, or null when the text is not one. */
function hexId(text: unknown, bytes: number): string | null {
  if (typeof text !== 'string' || text.length !== Math.ceil((bytes * 4) / 3)) return null
  try { const h = idToHex(text); return h.length === bytes * 2 ? h : null } catch { return null }
}
/** A model id of exactly `bytes` bytes as the wire writes it, or null when the text is not one. */
const wireId = (text: unknown, bytes: number): string | null => (typeof text === 'string' && text.length === bytes * 2 && /^[0-9a-f]+$/.test(text) ? idFromHex(text) : null)

/** A shape id `<sender>/<envelope number>/<index>` (10.1): the wire's sender is base64url, the model's hex. */
export function shapeIdFromWire(id: unknown): string | null {
  const m = typeof id === 'string' ? /^([A-Za-z0-9_-]{43})\/([1-9][0-9]{0,15})\/(0|[1-9][0-9]{0,9})$/.exec(id) : null
  const sender = m ? hexId(m[1], 32) : null
  return sender && Number(m![3]) <= 0xffffffff ? `${sender}/${m![2]}/${m![3]}` : null
}
export function shapeIdToWire(id: unknown): string | null {
  const m = typeof id === 'string' ? /^([0-9a-f]{64})\/([1-9][0-9]{0,15})\/(0|[1-9][0-9]{0,9})$/.exec(id) : null
  return m && Number(m[3]) <= 0xffffffff ? `${idFromHex(m[1]!)}/${m[2]}/${m[3]}` : null
}

const B64U = /^[A-Za-z0-9_-]+$/
/** A picture's attachment reference on the wire as the model keeps it (9.1.1). */
function pictureFromWire(a: unknown): AttachmentRef {
  if (!isObject(a)) return bad('an attachment is not an object')
  const { file_id, poster_file_id, ...rest } = a
  const attachment_id = hexId(file_id, 16)
  if (!attachment_id || typeof a['file_key'] !== 'string' || !B64U.test(a['file_key']) || typeof a['sha256'] !== 'string' || !B64U.test(a['sha256'])) return bad('an attachment names no file')
  const out: Fields = { attachment_id, ...rest }
  if (poster_file_id != null) out['poster_attachment_id'] = hexId(poster_file_id, 16) ?? bad('a poster names no file')
  return out as AttachmentRef
}
/** A picture's attachment reference of the model as the wire writes it: the fields of a picture alone. */
function pictureToWire(a: unknown): Fields {
  const file_id = isObject(a) ? wireId(a['attachment_id'], 16) : null
  if (!isObject(a) || !file_id || typeof a['file_key'] !== 'string' || typeof a['sha256'] !== 'string') throw failure('bad-argument', 'an attachment needs attachment_id, file_key and sha256')
  const out: Fields = { file_id, file_name: 'file', media_type: 'application/octet-stream', total_size: 0 }
  for (const f of ['file_key', 'sha256', 'file_name', 'media_type', 'total_size', 'width', 'height']) if (a[f] != null) out[f] = a[f]
  if (a['poster_attachment_id'] != null) out['poster_file_id'] = wireId(a['poster_attachment_id'], 16) ?? bad('a poster names no file')
  return out
}

const PENS: readonly unknown[] = ['pen', 'marker'], NOTES: readonly unknown[] = ['text', 'sticky', 'voice']
const MAX_WIDTH = 1000 * Q, MAX_SHAPES = 1000, MAX_SHAPE_IDS = 2000
const utf8 = new TextEncoder()
const bytesOf = (s: string): number => utf8.encode(s).length
const i32 = (v: unknown): v is number => typeof v === 'number' && Number.isInteger(v) && v >= -(2 ** 31) && v < 2 ** 31
const i32s = (v: unknown, n: number): v is number[] => Array.isArray(v) && v.length === n && v.every(i32)
const text = (v: unknown, min: number, max: number): v is string => typeof v === 'string' && bytesOf(v) >= min && bytesOf(v) <= max
const u32 = (v: unknown): v is number => typeof v === 'number' && Number.isInteger(v) && v >= 0 && v <= 0xffffffff
const sizeQ = (v: unknown): v is number => i32(v) && v >= 1 && v <= MAX_WIDTH
/** Fields a shape may carry by its tool; a field of another tool, or one that is null, refuses the shape. */
const SHAPE_FIELDS: readonly string[] = ['color', 'width', 'points', 'live', 'at', 'text', 'size', 'wrap', 'rect', 'attachment']
const TOOL_FIELDS: Readonly<Record<string, readonly string[]>> = { pen: ['color', 'width', 'points', 'live'], note: ['at', 'text', 'size', 'color', 'wrap'], image: ['rect', 'attachment'] }

/** One shape as the wire carries it, checked as trommi-core checks it; returns its family. */
function checkWireShape(e: unknown, withId: boolean): 'pen' | 'note' | 'image' {
  if (!isObject(e)) return bad('a shape is not an object')
  const family = PENS.includes(e['tool']) ? 'pen' : NOTES.includes(e['tool']) ? 'note' : e['tool'] === 'image' ? 'image' : bad('an unknown tool')
  if (('id' in e) !== withId) bad(withId ? 'a shape without its id' : 'a shape names an id')
  for (const f of [...SHAPE_FIELDS, 'group', 'id']) if (f in e && (e[f] === null || (SHAPE_FIELDS.includes(f) && !TOOL_FIELDS[family]!.includes(f)))) bad(`a shape's ${f} does not belong`)
  if ('z' in e && !i32(e['z'])) bad('a layer is not a whole number')
  if ('group' in e && !text(e['group'], 1, 80)) bad('a group name is out of range')
  if (family === 'pen') {
    if (!text(e['color'], 1, 40) || !sizeQ(e['width']) || !unpackPoints(e['points'])) bad('a stroke is not readable')
    if ('live' in e && !hexId(e['live'], 16)) bad('a live stroke id is not 16 bytes')
  } else if (family === 'note') {
    if (!i32s(e['at'], 2) || !text(e['text'], 0, 20000) || !sizeQ(e['size']) || !text(e['color'], 1, 40) || ('wrap' in e && !(i32(e['wrap']) && e['wrap'] > 0))) bad('a note is not readable')
  } else {
    const a = e['attachment']
    if (!i32s(e['rect'], 4) || !isObject(a) || !text(a['file_name'], 1, 255) || !text(a['media_type'], 1, 255) || !(typeof a['total_size'] === 'number' && Number.isSafeInteger(a['total_size']) && a['total_size'] >= 0)) return bad('a picture is not readable')
    for (const f of ['width', 'height']) if (f in a && !u32(a[f])) bad('a picture size is not a whole number')
    if (!hexId(a['file_key'], 32) || !hexId(a['sha256'], 32)) bad('a picture names no file')
    pictureFromWire(a)
  }
  return family
}
const units = (q: number): number => q / Q
/** A wire shape -> the model's entry (board units; a picture's own size, type and name beside its attachment). */
function entryFromWire(e: Fields, withId = false): Fields {
  const family = checkWireShape(e, withId)
  const out: Fields = { tool: e['tool'] }
  if (family === 'pen') {
    Object.assign(out, { color: e['color'], width: units(e['width'] as number), points: e['points'] })
    if ('live' in e) out['live'] = hexId(e['live'], 16)
  } else if (family === 'note') {
    Object.assign(out, { at: (e['at'] as number[]).map(units), text: e['text'], size: units(e['size'] as number), color: e['color'] })
    if ('wrap' in e) out['wrap'] = units(e['wrap'] as number)
  } else {
    const a = pictureFromWire(e['attachment'])
    Object.assign(out, { rect: (e['rect'] as number[]).map(units), attachment: a, nw: a.width ?? 0, nh: a.height ?? 0, mime: a.media_type ?? null, name: a.file_name ?? null })
  }
  if (e['z']) out['z'] = e['z']
  if ('group' in e) out['group'] = e['group']
  return out
}
const lengthQ = (v: unknown, fallback: number): number => Math.max(1, Math.min(MAX_WIDTH, Math.round((typeof v === 'number' && Number.isFinite(v) && v > 0 ? v : fallback) * Q)))
/** A model entry -> the wire's shape. Throws for what no reader would take. */
function entryToWire(e: unknown): Fields {
  if (!isObject(e)) return bad('a shape is not an object')
  const tool = e['tool']
  const out: Fields = { tool }
  if (PENS.includes(tool)) {
    Object.assign(out, { color: e['color'] ?? (tool === 'marker' ? 'yellow' : 'ink'), width: lengthQ(e['width'], tool === 'marker' ? 18 : 4), points: e['points'] })
    if (e['live'] != null) out['live'] = wireId(e['live'], 16) ?? bad('a live stroke id is not 16 bytes')
  } else if (NOTES.includes(tool)) {
    const at = Array.isArray(e['at']) ? e['at'] : []
    Object.assign(out, { at: [quantum(at[0]), quantum(at[1])], text: String(e['text'] ?? ''), size: lengthQ(e['size'], 20), color: e['color'] ?? 'ink' })
    if (e['wrap'] != null) out['wrap'] = lengthQ(e['wrap'], 1)
  } else if (tool === 'image') {
    const rect = Array.isArray(e['rect']) ? e['rect'] : []
    const a = pictureToWire(e['attachment'])
    for (const [k, v] of [['width', e['nw']], ['height', e['nh']]] as const) if (typeof v === 'number' && v > 0) a[k] = Math.round(v)
    if (typeof e['mime'] === 'string' && e['mime']) a['media_type'] = e['mime']
    if (typeof e['name'] === 'string' && e['name']) a['file_name'] = e['name']
    Object.assign(out, { rect: [0, 1, 2, 3].map(i => quantum(rect[i])), attachment: a })
  }
  if (typeof e['z'] === 'number' && e['z']) out['z'] = Math.round(e['z']) | 0
  if (typeof e['group'] === 'string' && e['group']) out['group'] = e['group']
  checkWireShape(out, false)
  return out
}

function idsFromWire(list: unknown): string[] {
  if (!Array.isArray(list) || !list.length || list.length > MAX_SHAPE_IDS || new Set(list).size !== list.length) return bad('shape ids: 1 to 2 000, none twice')
  return list.map(id => shapeIdFromWire(id) ?? bad('a shape id is not readable'))
}
/** A board item's body is one of 10.5's (it throws for anything else); its model form. */
function boardFromWire(b: Fields): Fields {
  const has = (f: string) => { if (b[f] === null) bad(`${f} is null`); return f in b }
  const type = b['content_type']
  if (type === 'strokes' && has('strokes') && !has('shape_ids') && !has('offset')) {
    const list = b['strokes']
    if (!Array.isArray(list) || !list.length || list.length > MAX_SHAPES) return bad('strokes: 1 to 1 000 shapes')
    return { content_type: type, strokes: list.map(e => entryFromWire(e as Fields)) }
  }
  if ((type === 'erase' || type === 'send_away') && !has('strokes') && !has('offset')) return { content_type: type, stroke_ids: idsFromWire(b['shape_ids']) }
  if (type === 'move' && !has('strokes') && has('offset')) {
    if (!i32s(b['offset'], 2)) return bad('an offset is two whole numbers')
    return { content_type: type, stroke_ids: idsFromWire(b['shape_ids']), offset: (b['offset'] as number[]).map(units) }
  }
  return bad('not a board item')
}
/** A board item's model form as the payload the merge takes (with `schema_version` 2). Throws for what no reader would take. */
function encodeBoardItem(c: Fields): string {
  const type = c['content_type']
  const ids = (): string[] => {
    const list = Array.isArray(c['stroke_ids']) ? c['stroke_ids'].map(id => shapeIdToWire(id) ?? bad('a shape id is not readable')) : bad('stroke_ids is not a list')
    if (!list.length || list.length > MAX_SHAPE_IDS || new Set(list).size !== list.length) bad('shape ids: 1 to 2 000, none twice')
    return list
  }
  let body: Fields
  if (type === 'strokes') {
    const list = Array.isArray(c['strokes']) ? c['strokes'] : []
    if (!list.length || list.length > MAX_SHAPES) bad('strokes: 1 to 1 000 shapes')
    body = { content_type: type, strokes: list.map(entryToWire) }
  } else if (type === 'erase' || type === 'send_away') body = { content_type: type, shape_ids: ids() }
  else if (type === 'move') { const o = Array.isArray(c['offset']) ? c['offset'] : []; body = { content_type: type, shape_ids: ids(), offset: [quantum(o[0]), quantum(o[1])] } }
  else throw failure('bad-argument', `unknown content_type ${String(type)}`)
  return JSON.stringify({ schema_version: 2, ...body })
}
/** A board item's payload in the wire's own form, checked: 'newer_schema' for a `schema_version` above 2, 'bad'
 *  for what is no board item. */
function parseBoardItem(payload: string): Fields | 'newer_schema' | 'bad' {
  let parsed: unknown
  try { parsed = JSON.parse(payload) } catch { return 'bad' }
  if (!isObject(parsed)) return 'bad'
  const version = parsed['schema_version']
  if (version !== undefined && !(typeof version === 'number' && Number.isInteger(version) && version >= 2)) return 'bad'
  if (typeof version === 'number' && version > 2) return 'newer_schema'
  try { boardFromWire(parsed); return parsed } catch { return 'bad' }
}
