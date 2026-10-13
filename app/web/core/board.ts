// board.ts: the seam between the client core's board model (core/client/src/model, the binding's `ClientModel`) and the
// page's copy of it. The core builds the model; it hands, once per batch, the patch of what changed (JSON in the
// shape of mirror.ts `ModelPatch`), which this module turns into the copy's own objects, and the copy back into the
// core's snapshot for a start from the cache.
import type { ModelPatch } from './mirror.ts'
import { devregsOf } from './model.ts'
import type { Model, Session, Timeline } from './types.ts'

type Obj = Record<string, unknown>

/** A patch as the core wrote it, with the Maps of the web model put back; `devregs`: the devices' own registers, when
 *  they changed (kept beside the model, model.ts `devregsOf`). */
export function revivePatch(text: string): ModelPatch & { devregs?: [string, Obj][] } {
  const p = JSON.parse(text) as ModelPatch & { sessions?: [string, (Obj & { registers: [string, unknown][] }) | null][]; timelines?: { meta: Obj | null }[] }
  for (const [, s] of p.sessions ?? []) if (s) (s as unknown as Session).registers = new Map(s.registers as unknown as [string, never][])
  for (const t of p.timelines ?? []) if (t.meta && t.meta['loaded_down_to'] == null) t.meta['loaded_down_to'] = Infinity
  return p
}

/** The whole model as the core reads it (`ClientModel.load`): Maps as pairs, a window's items as pairs. */
export function snapshotForCore(m: Model): string {
  const sessions = [...m.sessions].map(([k, s]) => [k, { ...s, registers: [...s.registers] }])
  const timelines = [...m.timelines].map(([k, t]: [string, Timeline]) => [k, { ...t, loaded_down_to: Number.isFinite(t.loaded_down_to) ? t.loaded_down_to : null, items: [...t.items] }])
  return JSON.stringify({
    room: { key_epoch: m.room.key_epoch, recovery_key: m.room.recovery_key, my_device_id: m.room.my_device_id },
    members: [...m.members], sessions, cards: [...m.cards], permissions: [...m.permissions], notes: [...m.notes], published: [...m.published],
    timelines, human: [...m.human.raw], alerts: m.alerts, newer: m.newer, devregs: [...devregsOf(m)],
  })
}

/** What the web model held as one object in two places, after a patch: a card's answer in force is the one of its
 *  answers; a session's settings are its register's value. */
export function relink(m: Model, p: ModelPatch): void {
  for (const [id] of p.cards ?? []) {
    const c = m.cards.get(id)
    const a = c?.answer
    if (c && a && !a.pending && a.envelope_hash) { const same = c.answers.find(x => x.envelope_hash === a.envelope_hash); if (same) c.answer = same }
  }
  // settings an older web kept under a session's agent device are that session's (no session of the device's key)
  for (const { key } of p.human ?? []) {
    if (!key.startsWith('session/')) continue
    const id = key.slice('session/'.length)
    if (m.sessions.has(id)) continue
    const owner = [...m.sessions.values()].find(x => x.agent_device_id === id || x.agent_device_ids.includes(id))
    if (!owner) continue
    m.human.session_settings.delete(id)
    if (owner.settings != null) m.human.session_settings.set(owner.session_id, owner.settings)
  }
  for (const [id] of p.sessions ?? []) {
    const s = m.sessions.get(id)
    const v = m.human.session_settings.get(id)
    if (s && v && s.settings && JSON.stringify(v) === JSON.stringify(s.settings)) s.settings = v
  }
}
