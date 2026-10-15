import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { test } from 'node:test'
import { coreWasm } from '../../../app/web/dev/build.mjs'

test('a wire-only source or dependency change invalidates the WASM output', () => {
  const repo = fs.mkdtempSync(path.join(os.tmpdir(), 'trommi-wire-freshness-'))
  const saved = { PATH: process.env.PATH, CARGO_HOME: process.env.CARGO_HOME }
  try {
    const write = (file, text = '') => {
      const full = path.join(repo, file)
      fs.mkdirSync(path.dirname(full), { recursive: true })
      fs.writeFileSync(full, text)
      fs.utimesSync(full, 1, 1)
    }
    for (const file of ['Cargo.toml', 'Cargo.lock', 'core/protocol/Cargo.toml', 'core/wire/Cargo.toml', 'core/client/Cargo.toml', 'core/bindings/swift/Cargo.toml', 'core/bindings/wasm/build.sh']) write(file)
    for (const dir of ['protocol/src', 'wire/src', 'client/src', 'bindings/swift/src', 'bindings/wasm/src', 'bindings/wasm/js']) write(`core/${dir}/fixture.rs`)
    write('rust-toolchain.toml', 'channel = "1.99.0"')
    write('core/bindings/wasm/Cargo.toml', 'wasm-bindgen = "=0.2.129"')
    for (const file of ['trommi_core_wasm_bg.wasm', 'trommi-core.js', 'trommi_core_wasm.js', 'idb-store.js', 'views-fold.js']) {
      write(`core/bindings/wasm/pkg/${file}`)
      fs.utimesSync(path.join(repo, 'core/bindings/wasm/pkg', file), 2, 2)
    }
    // No compiler can start: a stale result must reach the explicit missing-tool diagnostic.
    process.env.PATH = path.join(repo, 'no-tools')
    process.env.CARGO_HOME = path.join(repo, 'no-cargo')
    for (const file of ['core/wire/src/fixture.rs', 'core/wire/Cargo.toml']) {
      fs.utimesSync(path.join(repo, file), 3, 3)
      assert.throws(() => coreWasm(repo), /older than the core's sources, and cargo is not installed/)
      fs.utimesSync(path.join(repo, file), 1, 1)
    }
  } finally {
    for (const [key, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[key]
      else process.env[key] = value
    }
    fs.rmSync(repo, { recursive: true, force: true })
  }
})
