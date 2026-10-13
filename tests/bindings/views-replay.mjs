// The web's fold (core/bindings/wasm/js/views-fold.js) replays every recorded view scene
// (core/client/tests/conformance/views/*.json, written by core/client/tests/view_scenes.rs) and must reach the view
// model the core reached, exactly; a delta out of order must be refused.
//   node tests/bindings/views-replay.mjs
import fs from 'node:fs'
import path from 'node:path'
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
import { emptyView, foldView } from '../../core/bindings/wasm/js/views-fold.js'

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../core/client/tests/conformance/views')
const names = fs.readdirSync(dir).filter(n => n.endsWith('.json')).sort()
assert.ok(names.length >= 5, 'the recorded scenes')
for (const name of names) {
  const { scene, deltas, view } = JSON.parse(fs.readFileSync(path.join(dir, name), 'utf8'))
  let v = emptyView()
  for (const d of deltas) v = foldView(v, d)
  assert.deepStrictEqual(v, view, `${scene}: the folded view is the core's`)
  if (deltas.length > 1) assert.throws(() => foldView(v, deltas[1]), /out of order/, `${scene}: an old delta is refused`)
}
console.log(`${names.length} recorded scenes replayed by the web's fold`)
