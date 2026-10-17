// dictate.mjs: what only the page can do for sealed dictation (spec/v1.md 18; the core's PlatformOp record_start,
// record_stop, record_cancel, attest): the microphone, the recording turned into 16 kHz mono WAV, and the check of
// Tinfoil's attestation bundle with Tinfoil's own verifier (core/tinfoil-verifier.mjs, loaded on the first dictation
// only). Nothing here decides: when to record, whether to send, what to send is the core's. The recording never
// leaves this page but to the core, which seals it to the verified enclave.

const REFUSED = { answer: { answer: 'refused' }, blobs: [] }
const RATE = 16000

let rec = null   // { stream, recorder, chunks, started, ended, timer, analyser, ctx }
let asking = 0   // the start whose microphone prompt is open; a cancel or a new start makes it stale

/** Whether this browser can record and check attestations at all. */
export const canDictate = () => typeof window !== 'undefined' && !!navigator.mediaDevices?.getUserMedia && typeof MediaRecorder === 'function'
  && typeof OfflineAudioContext === 'function' && typeof DecompressionStream === 'function' && globalThis.isSecureContext === true

/** The microphone's level now, 0..1, for the bars while recording; 0 when none. */
export function dictateLevel() {
  const a = rec?.analyser
  if (!a || rec.recorder.state !== 'recording') return 0
  const data = new Uint8Array(a.fftSize)
  a.getByteTimeDomainData(data)
  let peak = 0
  for (const v of data) peak = Math.max(peak, Math.abs(v - 128))
  return Math.min(1, peak / 64)
}

/** The microphone off: its tracks stopped, the level's context closed (what was recorded stays in `r.chunks`). */
function mute(r) {
  clearTimeout(r.timer)
  for (const t of r.stream.getTracks()) t.stop()
  r.ctx?.close().catch(() => {})
  r.ctx = null
  r.analyser = null
}

function release() {
  const r = rec
  rec = null
  if (r) mute(r)
}

async function recordStart(op) {
  if (!canDictate()) return REFUSED
  release()
  const mine = ++asking
  let stream
  try { stream = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true } }) } catch { return REFUSED }
  // (cancelled, or started again, while the browser asked: this microphone is not wanted any more)
  if (mine !== asking) { for (const t of stream.getTracks()) t.stop(); return REFUSED }
  let recorder
  const chunks = []
  try {
    recorder = new MediaRecorder(stream)
    recorder.addEventListener('dataavailable', e => { if (e.data?.size) chunks.push(e.data) })
  } catch { for (const t of stream.getTracks()) t.stop(); return REFUSED }
  const ended = new Promise(resolve => recorder.addEventListener('stop', resolve, { once: true }))
  let ctx = null, analyser = null
  try {
    ctx = new AudioContext()
    analyser = ctx.createAnalyser()
    analyser.fftSize = 512
    ctx.createMediaStreamSource(stream).connect(analyser)
  } catch { ctx?.close().catch(() => {}); ctx = null; analyser = null }
  const r = { stream, recorder, chunks, started: performance.now(), ended, timer: 0, analyser, ctx }
  try { recorder.start(1000) } catch { rec = r; release(); return REFUSED }
  // (the core stops it at max_ms; this only ends one the core forgot: the microphone off, what was recorded kept)
  r.timer = setTimeout(() => { if (recorder.state !== 'inactive') recorder.stop(); mute(r) }, Math.max(1000, Number(op.max_ms) || 300000))
  rec = r
  return { answer: { answer: 'recording' }, blobs: [] }
}

async function recordStop() {
  const r = rec
  if (!r) return REFUSED
  const ms = Math.round(performance.now() - r.started)
  if (r.recorder.state !== 'inactive') r.recorder.stop()
  await r.ended
  // (only this recording's microphone: another may have begun while it ended)
  mute(r)
  if (rec === r) rec = null
  try {
    const wav = await toWav(await new Blob(r.chunks, { type: r.recorder.mimeType }).arrayBuffer())
    return { answer: { answer: 'recorded', audio: 0, ms }, blobs: [wav] }
  } catch { return REFUSED }
}

function recordCancel() {
  asking++
  const r = rec
  if (r && r.recorder.state !== 'inactive') { try { r.recorder.stop() } catch {} }
  if (r) r.chunks.length = 0
  release()
  return { answer: { answer: 'done' }, blobs: [] }
}

/** Whatever the browser recorded (webm/opus, mp4/aac) as 16 kHz mono 16-bit PCM WAV, which the enclave's models take. */
async function toWav(bytes) {
  const ctx = new AudioContext()
  let decoded
  try { decoded = await ctx.decodeAudioData(bytes) } finally { ctx.close().catch(() => {}) }
  const frames = Math.max(1, Math.ceil(decoded.duration * RATE))
  const off = new OfflineAudioContext(1, frames, RATE)
  const src = off.createBufferSource()
  src.buffer = decoded
  src.connect(off.destination)
  src.start()
  const pcm = (await off.startRendering()).getChannelData(0)
  const out = new DataView(new ArrayBuffer(44 + pcm.length * 2))
  const text = (at, s) => { for (let i = 0; i < s.length; i++) out.setUint8(at + i, s.charCodeAt(i)) }
  text(0, 'RIFF'); out.setUint32(4, 36 + pcm.length * 2, true); text(8, 'WAVE')
  text(12, 'fmt '); out.setUint32(16, 16, true); out.setUint16(20, 1, true); out.setUint16(22, 1, true)
  out.setUint32(24, RATE, true); out.setUint32(28, RATE * 2, true); out.setUint16(32, 2, true); out.setUint16(34, 16, true)
  text(36, 'data'); out.setUint32(40, pcm.length * 2, true)
  for (let i = 0; i < pcm.length; i++) out.setInt16(44 + i * 2, Math.max(-1, Math.min(1, pcm[i])) * 0x7fff, true)
  return new Uint8Array(out.buffer)
}

/** Tinfoil's verifier on the bundle the hub passed on, pinned to the core's repo: the enclave's HPKE key and host. */
async function attest(op, blobs) {
  let bundle
  try { bundle = JSON.parse(new TextDecoder().decode(blobs[op.bundle])) } catch { return { answer: { answer: 'not_attested', reason: 'not a bundle' }, blobs: [] } }
  try {
    const { Verifier } = await import('./gen/vendor/tinfoil-verifier.mjs')
    const found = await new Verifier({ configRepo: op.repo }).verifyBundle(bundle)
    const hex = found?.hpkePublicKey ?? ''
    if (!/^[0-9a-f]{64}$/.test(hex) || typeof bundle?.domain !== 'string') return { answer: { answer: 'not_attested', reason: 'no HPKE key' }, blobs: [] }
    const key = Uint8Array.from(hex.match(/../g), h => parseInt(h, 16))
    return { answer: { answer: 'attested', hpke_key: 0, domain: bundle.domain }, blobs: [key] }
  } catch (err) {
    return { answer: { answer: 'not_attested', reason: String(err?.message ?? err).slice(0, 200) }, blobs: [] }
  }
}

/** Answers one of dictation's platform ops; null for another op. */
export function dictateOp(op, blobs) {
  switch (op.op) {
    case 'record_start': return recordStart(op)
    case 'record_stop': return recordStop()
    case 'record_cancel': return recordCancel()
    case 'attest': return attest(op, blobs)
    default: return null
  }
}
