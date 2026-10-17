// Sealed dictation end to end, by hand: the app core (its real binding, core/bindings/wasm/pkg) through the web's
// executor (app/web/core/shell.ts) against a hub with a Tinfoil key, and the real Tinfoil enclave. An account is made
// for the run (a made-up address) and deleted again; each WAV given is "recorded" (the platform op answers with the
// file), Tinfoil's attestation is checked with the vendored verifier (app/web/core/tinfoil-verifier.mjs), and the
// words that come back are printed with the time each took. Nothing of a recording is kept.
//
//   node tests/web/shell/dictation-e2e.mjs https://hub.trommi.com a.wav b.wav ...
//
// Needs the binding built (core/bindings/wasm/build.sh). Not part of any suite: it spends the hub's Tinfoil quota.
import { readFile } from 'node:fs/promises'
import { CoreShell, memorySpaces } from '../../../app/web/core/shell.ts'
import { foldView, emptyView } from '../../../core/bindings/wasm/js/views-fold.js'
import { Verifier } from '../../../app/web/core/tinfoil-verifier.mjs'

const [hub, ...files] = process.argv.slice(2)
if (!hub || !files.length) { console.error('usage: dictation-e2e.mjs <hub url> <file.wav> ...'); process.exit(2) }
const PKG = new URL('../../../core/bindings/wasm/pkg/', import.meta.url)
const b = await import(new URL('trommi-core.js', PKG).href)
await b.init(await readFile(new URL('trommi_core_wasm_bg.wasm', PKG)))

let view = emptyView()
const waiters = []
let wav = null
const platform = async (op, blobs) => {
  switch (op.op) {
    case 'record_start': return { answer: { answer: 'recording' }, blobs: [] }
    case 'record_stop': return { answer: { answer: 'recorded', audio: 0, ms: Math.round((wav.length - 44) / 32) }, blobs: [wav] }
    case 'record_cancel': return { answer: { answer: 'done' }, blobs: [] }
    case 'attest': {
      const bundle = JSON.parse(new TextDecoder().decode(blobs[op.bundle]))
      try {
        const found = await new Verifier({ configRepo: op.repo }).verifyBundle(bundle)
        console.log(`  attested: ${bundle.domain}, measurement ${found.measurement.type.split('/').slice(-2).join('/')}`)
        return { answer: { answer: 'attested', hpke_key: 0, domain: bundle.domain }, blobs: [Uint8Array.from(found.hpkePublicKey.match(/../g), h => parseInt(h, 16))] }
      } catch (e) { console.log('  NOT attested:', e.message); return { answer: { answer: 'not_attested', reason: String(e.message) }, blobs: [] } }
    }
    default: return { answer: { answer: 'refused' }, blobs: [] }
  }
}
const shell = new CoreShell({
  core: new b.AppCore(), ...memorySpaces(),
  host: {
    render: delta => { view = foldView(view, delta); for (const w of [...waiters]) if (w.want(view)) { waiters.splice(waiters.indexOf(w), 1); w.done(view) } },
    log: (level, text) => { if (level === 'error' || level === 'warn') console.error(`  core ${level}:`, text) },
    platform,
  },
})
const until = (what, want, ms = 120_000) => want(view) ? Promise.resolve(view) : new Promise((done, fail) => {
  const t = setTimeout(() => fail(new Error(`no view with ${what}; phase ${view.phase}, dictation ${JSON.stringify(view.dictation)}`)), ms)
  waiters.push({ want, done: v => { clearTimeout(t); done(v) } })
})
const act = action => shell.send({ type: 'action', action })

const email = `dictation-test-${Date.now()}@example.org`
const password = `e2e ${crypto.randomUUID()}`
shell.send({ type: 'start', config: { platform: 'web', version: '2.0.0', hub, seed: Math.floor(Math.random() * 2 ** 40), utc_offset_min: 120, locale: 'de-DE', can: { dictate: true } } })
await until('the sign-in', v => v.phase === 'signed_out')
act({ do: 'account', action: { step: 'found', email, password } })
await until('live', v => v.phase === 'live')
act({ do: 'account', action: { step: 'kit_kept' } })
console.log(`account ${email} made on ${hub}`)
try {
  for (const file of files) {
    wav = new Uint8Array(await readFile(file))
    console.log(`${file} (${(wav.length / 1024).toFixed(0)} KiB)`)
    const t0 = Date.now()
    act({ do: 'dictate', step: { step: 'start', session: 'e2e' } })
    await until('recording', v => v.dictation?.state === 'recording')
    act({ do: 'dictate', step: { step: 'stop' } })
    const v = await until('the words', v => v.dictation?.state === 'done' || v.dictation?.state === 'failed')
    if (v.dictation.state === 'done') {
      console.log(`  ${((Date.now() - t0) / 1000).toFixed(1)} s: ${v.dictation.text.text}`)
      act({ do: 'dictate', step: { step: 'taken', id: v.dictation.text.id } })
    } else {
      console.log(`  FAILED: ${v.dictation.problem?.en}`)
      act({ do: 'dictate', step: { step: 'dismiss' } })
    }
    await until('idle', v => v.dictation?.state === 'idle')
  }
} finally {
  act({ do: 'account', action: { step: 'delete_account', password } })
  await until('deleted', v => v.phase === 'signed_out', 60_000).then(() => console.log('account deleted'), e => console.error('the account was NOT deleted:', e.message))
  process.exit(0)
}
