// codec.ts: the bodies of stored content (spec/v1.md 9.1: UTF-8 JSON, `schema_version` 2) between the wire and the
// model, as the app calls them. The rules are the client core's (core/client/src/codec.rs), the same for every app; this
// module only hands the fields across (as JSON text) and gives the answers the shapes the model and the views name
// (types.ts). What the two forms are, and what is converted, is said in codec.rs.
//
// Reading: what the core received the client core's model reads itself (core/client/src/model); `decodeBody` reads a
// payload in hand. Writing: `encodeBody` turns a draft's fields into the text the core seals.
//
// The core's codec is reached through the binding, which this module is handed once it is loaded (`useCodec`:
// core-wasm.ts, and the tests' stand-in). The page that draws the Scribble Board has no core: scribble.ts keeps the
// board item's rules for it.
import type { FileRef } from './core-api.ts'
import { b64u, unb64u, unhex } from './ids.ts'
import type { AttachmentRef } from './types.ts'

export const SCHEMA_VERSION = 2
/** What every client says (or shows) when it meets something only a newer version understands. */
export const UPDATE_MESSAGE = 'This needs a newer version of Trommi. Update to see it.'

/** Which body: a Chat message, a board item, the version of a card, Note or Artifact, an answer, a take back, a
 *  permission request, a verdict, a register value. */
export type BodyKind = 'message' | 'board_item' | 'card' | 'note' | 'artifact' | 'answer' | 'take_back' | 'request' | 'verdict' | 'register'
/** A body as plain fields. */
export type Fields = Record<string, unknown>

/** A card's own fields: everything of its body but the version's number and predecessor (codec.rs `card_content_fields`). */
export const CARD_CONTENT_FIELDS: readonly string[] = Object.freeze(['card_type', 'title', 'teaser', 'body', 'options', 'sections', 'html', 'allows_multiple', 'recommended', 'urgency_reason', 'attachments',
  'change_note', 'close_summary', 'withdraw_reason', 'merged_into_object_id', 'merged_from_object_ids', 'working'])
/** A card's card_type. */
export const CARD_TYPES: readonly string[] = Object.freeze(['decision', 'info'])
/** An answer's answer_action. */
export const ANSWER_ACTIONS: readonly string[] = Object.freeze(['answer', 'read', 'shred'])
/** A board item's content types (10.5). */
export const BOARD_CONTENT_TYPES: readonly string[] = Object.freeze(['strokes', 'erase', 'move', 'send_away'])

/** A body that cannot be written (encodeBody): `code` is 'bad-argument' or 'bad-format'. */
export class BodyError extends Error {
  code: string
  constructor(code: string, message: string) { super(message); this.name = 'BodyError'; this.code = code }
}

// ---- the core's codec ----------------------------------------------------------------------------------------------

/** The calls of the binding this module uses (core/bindings/wasm/js/trommi-core.d.ts, "the body codec"). */
export interface CodecCalls {
  bodyFromWire(kindName: string, payload: Uint8Array): string
  bodyToWire(kindName: string, fieldsJson: string): string
  registerToWire(key: string, valueJson: string): { name: string; value: Uint8Array | null }
  registerKeyOf(name: string): string
  registerNameOf(key: string): string
  bodyFileIds(kindName: string, fieldsJson: string): Uint8Array[]
  attachmentFromWire(attachmentJson: string): string
  attachmentToWire(attachmentJson: string): string
  pieceFromWire(piece: Uint8Array): string | null
  pieceToWire(pieceJson: string): Uint8Array
}
let calls: CodecCalls | null = null
/** Hands this module the loaded binding. */
export function useCodec(binding: CodecCalls): void { calls = binding }
function core(): CodecCalls {
  if (!calls) throw new BodyError('core-missing', 'codec: the core is not loaded (useCodec)')
  return calls
}
/** A call whose refusal is the codec's: its code stands at the head of the core's message ('bad-argument: …'). */
function written<T>(call: () => T): T {
  try { return call() } catch (e) {
    const m = /^(bad-argument|bad-format): (.*)$/s.exec((e as Error)?.message ?? '')
    if ((e as { code?: unknown })?.code === 'bad-format' && m) throw new BodyError(m[1]!, m[2]!)
    throw e
  }
}
const utf8 = new TextEncoder()
const utf8Text = new TextDecoder()
const json = (v: unknown): string => JSON.stringify(v ?? null)

// ---- reading -------------------------------------------------------------------------------------------------------

/** A body as the core read it (ReceivedEnvelope `body`, or bodyFromWire): the model's fields, 'newer_schema' for a
 *  `schema_version` above 2 (shown as "needs a newer Trommi"), 'bad' for what is no body of its kind. */
export function bodyFromText(text: string): Fields | 'newer_schema' | 'bad' {
  return text === 'newer_schema' || text === 'bad' ? text : JSON.parse(text) as Fields
}
/**
 * A body's payload (the JSON the core opened, as its bytes or as text) as the model's fields; 'newer_schema' for a
 * `schema_version` above 2, 'bad' for what is no body of its kind. A register gives { name, key, value, lamport }:
 * `key` is the model's key for the wire's `name`.
 */
export function decodeBody(kind: BodyKind, payload: string | Uint8Array): Fields | 'newer_schema' | 'bad' {
  return bodyFromText(core().bodyFromWire(kind, typeof payload === 'string' ? utf8.encode(payload) : payload))
}
/** A stroke piece as the model shows it: a stroke still being drawn, its new points (7.2). */
export interface StrokePiece { content_type: 'stroke_piece'; stroke: string; number: number; tool: string; color: string; width: number; points: string }
/** The `piece` of a stroke piece message -> the model's form, or null for one trommi-core would refuse. */
export function decodePiece(piece: string): StrokePiece | null {
  const text = core().pieceFromWire(utf8.encode(piece))
  return text === null ? null : JSON.parse(text) as StrokePiece
}
/** A wire attachment reference as the model keeps it; throws BodyError for one that names no file. */
export function attachmentFromWire(a: unknown): AttachmentRef {
  return JSON.parse(written(() => core().attachmentFromWire(json(a)))) as AttachmentRef
}
/** A register's name on the wire -> its key in the model ('board_snapshot/<board>' is 'scribble_snapshot/desk/<board>'). */
export const registerKeyOf = (name: string): string => core().registerKeyOf(name)

// ---- writing -------------------------------------------------------------------------------------------------------

/** A draft's fields (the model's names) as the payload the core seals: one JSON object with `schema_version` 2.
 *  Throws BodyError for fields no reader would take. */
export function encodeBody(kind: Exclude<BodyKind, 'register'>, fields: Fields): string {
  return written(() => core().bodyToWire(kind, json(fields)))
}
/** The same as the bytes a draft carries (core-api.ts Draft `payload`). */
export const encodeBodyBytes = (kind: Exclude<BodyKind, 'register'>, fields: Fields): Uint8Array => utf8.encode(encodeBody(kind, fields))
/** A register write as the core's draft takes it (core-api.ts Draft `register`): the wire's name for the model's
 *  key, and the value as JSON (null deletes). The core adds the lamport and makes the body. */
export function encodeRegister(key: string, value: unknown): { name: string; value: Uint8Array | null } {
  const w = written(() => core().registerToWire(key, json(value)))
  return { name: w.name, value: w.value }
}
/** A register's key in the model -> its name on the wire. */
export const registerNameOf = (key: string): string => core().registerNameOf(key)
/** The model's piece -> the `piece` text a stroke piece message carries. */
export function encodePiece(p: { stroke: string; number: number; tool: string; color?: string | null; width?: number | null; points: string }): string {
  return utf8Text.decode(written(() => core().pieceToWire(json(p))))
}
/** A model attachment reference as a body carries it. */
export function attachmentToWire(a: unknown): Fields {
  return JSON.parse(written(() => core().attachmentToWire(json(a)))) as Fields
}
/** The files a body names, as sealing takes them for the envelope's header (core-api.ts `seal`, fileIds): none twice. */
export function fileIdsOf(kind: BodyKind, fields: Fields): Uint8Array[] {
  return core().bodyFileIds(kind, json(fields))
}

// ---- files: the model's reference and the core's FileRef -----------------------------------------------------------

/** The model's reference for a file the core encrypted (core-api.ts FileRef), with what the app knows of it. */
export function attachmentRef(file: FileRef, meta: Fields = {}): AttachmentRef {
  const ref = attachmentFromWire({ file_id: b64u(file.fileId), file_key: b64u(file.fileKey), sha256: b64u(file.sha256), total_size: 0 }) as Fields
  for (const f of ['total_size', 'file_name', 'media_type', 'width', 'height', 'caption', 'page', 'poster_attachment_id', 'marks']) if (meta[f] != null) ref[f] = meta[f]
  return ref as AttachmentRef
}
/** What the core needs to fetch and open a model attachment (core-api.ts FileRef). `fileKey` is a secret. */
export function fileRefOf(a: AttachmentRef): FileRef {
  if (!/^[0-9a-f]{32}$/.test(a.attachment_id ?? '')) throw new BodyError('bad-format', 'an attachment names no file')
  return { fileId: unhex(a.attachment_id), fileKey: unb64u(a.file_key), sha256: unb64u(a.sha256) }
}
