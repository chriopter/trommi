// Writes the iOS app's Tinfoil verifier (TrommiApp/Sources/TrommiApp/Resources/tinfoil-verifier.js) from the web's
// vendored one (app/web/core/tinfoil-verifier.mjs, see app/web/THIRD-PARTY.md): the same code, as a classic script that
// sets `TinfoilVerifier.Verifier` (NativeDictation.swift runs it in a hidden web view). esbuild only rewraps it.
//   node app/ios/dev/verifier.mjs          write it
//   node app/ios/dev/verifier.mjs --check  exit 1 when the file is not what the web's gives
import fs from 'node:fs'
import path from 'node:path'
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'

const repo = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../..')
const esbuild = createRequire(path.join(repo, 'app/web/package.json'))('esbuild')
const src = path.join(repo, 'app/web/core/tinfoil-verifier.mjs')
const out = path.join(repo, 'app/ios/TrommiApp/Sources/TrommiApp/Resources/tinfoil-verifier.js')
const head = '// Written by app/ios/dev/verifier.mjs from app/web/core/tinfoil-verifier.mjs (Tinfoil\'s verifier, Apache-2.0 and MIT:\n// app/web/THIRD-PARTY.md) and app/web/core/webkit-gunzip.mjs. Do not edit.\n'
const entry = `import { fixDecompression } from ${JSON.stringify(path.join(repo, 'app/web/core/webkit-gunzip.mjs'))}\nfixDecompression()\nexport { Verifier } from ${JSON.stringify(src)}\n`
const built = await esbuild.build({ stdin: { contents: entry, resolveDir: repo, loader: 'js' }, bundle: true, format: 'iife', globalName: 'TinfoilVerifier', footer: { js: 'globalThis.TinfoilVerifier = TinfoilVerifier;' }, legalComments: 'eof', write: false, logLevel: 'silent' })
const text = head + built.outputFiles[0].text
if (process.argv.includes('--check')) {
  if (!fs.existsSync(out) || fs.readFileSync(out, 'utf8') !== text) { console.error(`${path.relative(repo, out)} is not what the web's verifier gives: node app/ios/dev/verifier.mjs`); process.exit(1) }
  console.log('tinfoil-verifier.js is current')
} else { fs.writeFileSync(out, text); console.log(`wrote ${path.relative(repo, out)}`) }
