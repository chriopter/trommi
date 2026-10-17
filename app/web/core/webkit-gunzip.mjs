// webkit-gunzip.mjs: WebKit's DecompressionStream (Safari, the iPhone app's web view) refuses the chunks a Response's
// body gives when that body is piped into it ("Invalid type should be ArrayBuffer"), and Tinfoil's verifier gunzips its
// attestation that way. This wraps the stream so every chunk reaches it as a fresh Uint8Array: the same bytes, no
// other change. Other browsers take it as it is. Called before the verifier runs (public/dictate.mjs, and the iPhone's
// copy of the verifier: app/ios/dev/verifier.mjs).
export function fixDecompression() {
  const DS = globalThis.DecompressionStream
  if (!DS || DS.trommiWrapped) return
  class Wrapped {
    constructor(format) {
      const ds = new DS(format), writer = ds.writable.getWriter()
      const copy = c => new Uint8Array(ArrayBuffer.isView(c) ? c.buffer.slice(c.byteOffset, c.byteOffset + c.byteLength) : c)
      this.writable = new WritableStream({ write: c => writer.write(copy(c)), close: () => writer.close(), abort: r => writer.abort(r) })
      this.readable = ds.readable
    }
  }
  Wrapped.trommiWrapped = true
  globalThis.DecompressionStream = Wrapped
}
