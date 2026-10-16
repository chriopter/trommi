// Writes the session marks of the iOS app's pen.json (TrommiApp/Sources/TrommiApp/Resources/pen.json, keys "draw:<name>"):
// the faces and the two device drawings, as app/web/public/ui.mjs doodleSvg draws them. The other keys stay as they are.
//   node app/ios/dev/pen.mjs          write them
//   node app/ios/dev/pen.mjs --check  exit 1 when pen.json does not hold what ui.mjs gives
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const ui = await import(path.join(here, '../../web/public/ui.mjs'))
const file = path.join(here, '../TrommiApp/Sources/TrommiApp/Resources/pen.json')
const names = JSON.parse(fs.readFileSync(path.join(here, '../../web/public/drawings.json'), 'utf8')).map(d => d.name)
const before = fs.readFileSync(file, 'utf8')
const pen = JSON.parse(before)
const out = {}
for (const [k, v] of Object.entries(pen)) if (!k.startsWith('draw:')) out[k] = v
for (const n of [...names, 'phone', 'house']) out[`draw:${n}`] = ui.doodleSvg(`draw:${n}`)
const text = JSON.stringify(Object.fromEntries(Object.keys(out).sort().map(k => [k, out[k]]))) + (before.endsWith('\n') ? '\n' : '')
if (process.argv.includes('--check')) {
  if (text !== before) { console.error('pen.json is not what ui.mjs gives: node app/ios/dev/pen.mjs'); process.exit(1) }
  console.log('pen.json is current')
} else {
  fs.writeFileSync(file, text)
  console.log(`wrote ${path.relative(process.cwd(), file)}`)
}
