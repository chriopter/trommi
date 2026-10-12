// core-sessions.mjs: the session list of a model built by hand, as the client core makes it (client/src/sessions.rs,
// through the WASM binding in core/wasm/pkg): the page draws `model.session_list` and has no rules of its own.
//   import { withSessions } from './core-sessions.mjs'   … withSessions(model) before BoardState.update()
import fs from 'node:fs'
import { snapshotForCore } from '../../../app/web/core/board.ts'

const PKG = new URL('../../../core/wasm/pkg/trommi-core.js', import.meta.url)
if (!fs.existsSync(PKG)) throw new Error('core/wasm/pkg is not built (sh core/wasm/build.sh): the session list is the core\'s')
const binding = await import(PKG.href)
await binding.init(fs.readFileSync(new URL('../../../core/wasm/pkg/trommi_core_wasm_bg.wasm', import.meta.url)))

/** Puts the core's session list of `m` into `m.session_list` (the settings and the crown a test set by hand count as
 *  the registers they stand for). Returns `m`. */
export function withSessions(m) {
  const raw = new Map(m.human.raw)
  for (const [id, v] of m.human.session_settings ?? []) if (!raw.has(`session/${id}`)) raw.set(`session/${id}`, { value: v, envelope_number: 1, by_device_id: null, pending: false, causal: null })
  if (m.human.crown && !raw.has('crown')) raw.set('crown', { value: m.human.crown, envelope_number: 1, by_device_id: null, pending: false, causal: null })
  const cm = new binding.ClientModel(m.room.my_device_id ?? '')
  try {
    cm.load(snapshotForCore({ ...m, human: { ...m.human, raw } }))
    m.session_list = JSON.parse(cm.sessionList(Boolean(m.room.reading)))
  } finally { cm.free?.() }
  return m
}
