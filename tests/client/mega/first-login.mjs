// A traced first login into a full account (tests/client/mega/full.rs holds one open): a new device of the web app
// (the app core in its executor, web-observer.mjs: it signs in with the account's e-mail and password) or of the iOS
// app's engine (MegaObserverTests.swift, its test runner on Linux: with the recovery code) follows the live stream and
// keeps every request, every event the core handled (web) or every call into the core (iOS), every timed step and
// samples of what the person would see. Written as JSON; trace-view.mjs draws it.
//
//   node tests/client/mega/first-login.mjs <MEGA_OUT>/full/fixture.json web|ios <out.json> [<tree>]
//
// TRACE_RTT_MS=40: every request of the web client waits so long first, as over a real network.
//
// <tree>: the checkout whose clients sign in (default: this one; it must have been prepared, prepare.sh).
import { spawn } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const [fixtureFile, platform, outFile, treeArg] = process.argv.slice(2)
const here = path.dirname(fileURLToPath(import.meta.url))
const tree = path.resolve(treeArg ?? path.join(here, '../../..'))
const fixture = JSON.parse(fs.readFileSync(fixtureFile, 'utf8'))
const dir = path.join(path.dirname(outFile), `device-${platform}-${Date.now()}`)
fs.mkdirSync(dir, { recursive: true })
const log = fs.openSync(path.join(dir, 'log.txt'), 'w')
const env = { ...process.env, TROMMI_TRACE: '1', MEGA_SEED: '1' }
const child = platform === 'web'
  ? spawn('node', [path.join(here, 'web-observer.mjs'), dir, tree], { env, stdio: ['ignore', log, log] })
  : spawn(path.join(tree, 'tests/ios/.build/out/Products/Debug-linux-x86_64/TrommiCoreLiveTests-test-runner'), ['TrommiCoreLiveTests.MegaObserverTests/testObserve'], { env: { ...env, TROMMI_MEGA_OBSERVER: dir }, stdio: ['ignore', log, log] })
let n = 0
async function call(cmd, limitS = 1800) {
  const at = n++
  fs.writeFileSync(path.join(dir, `cmd-${at}.json.tmp`), JSON.stringify(cmd))
  fs.renameSync(path.join(dir, `cmd-${at}.json.tmp`), path.join(dir, `cmd-${at}.json`))
  const res = path.join(dir, `res-${at}.json`)
  for (let i = 0; i < limitS * 20; i++) {
    if (fs.existsSync(res)) { const v = JSON.parse(fs.readFileSync(res, 'utf8')); if (!v.ok) throw new Error(`${cmd.op}: ${v.error} ${v.message}`); return v.result }
    if (child.exitCode !== null) throw new Error(`the ${platform} device exited at ${cmd.op}`)
    await new Promise(r => setTimeout(r, 50))
  }
  throw new Error(`${cmd.op}: no answer in ${limitS} s`)
}
const t = Date.now()
let signin = null, error = null
try { signin = await call({ op: 'signin', hub: fixture.hub, room_id: fixture.room_id, code: fixture.code, email: fixture.email, password: fixture.password, stream: true, trace: true }) } catch (e) { error = String(e.message ?? e) }
const wall = Date.now() - t
// (a few seconds more: the presence of the live agents on the stream, and what the engine does after its settle)
const trace = await call({ op: 'trace', linger_ms: Number(process.env.TRACE_LINGER_MS ?? 4000) })
await call({ op: 'quit' }).catch(() => {})
child.kill()
fs.writeFileSync(outFile, JSON.stringify({ ...trace, platform, wall_ms: wall, signin, error, fixture: { counts: fixture.counts, built_s: fixture.built_s } }))
console.log(`${platform}: ${wall} ms${error ? ` (error: ${error})` : ''}, ${trace.spans.length} spans, ${trace.samples.length} samples, final ${JSON.stringify(trace.final)}`)
process.exit(0)
