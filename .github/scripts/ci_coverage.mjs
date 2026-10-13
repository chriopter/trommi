// Fails when a test exists that no workflow runs. Run by pr-check.yml (job "Which parts changed") on every pull request:
//   node .github/scripts/ci_coverage.mjs
// 1. every crate of the workspace (Cargo.toml members) is named with `-p <crate>` in a workflow, or is listed in NOT_TESTED;
// 2. every test target of tests/Cargo.toml is run: its name stands in a workflow, or the workflow picks the targets of
//    its prefix by a `sed 's/^name = "\(prefix_...` selection;
// 3. every file that the web app's test scripts (app/web/package.json, test:*) name stands in a workflow, or is in
//    NOT_RUN (with its reason). A new crate, test target or browser test that nobody wired in fails here.
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = path.join(path.dirname(fileURLToPath(import.meta.url)), '../..')
const read = file => fs.readFileSync(path.join(repo, file), 'utf8')

// crates with nothing to test, and files known not to pass: each with the reason
const NOT_TESTED = {
  'trommi-uniffi-bindgen': 'a build tool for the Swift binding (built by core/bindings/swift/build.sh), no tests',
  'trommi-hub-updater': 'clippy only; its behaviour is tested by tests/deploy (the hub job)',
  'trommi-hub': 'has no unit tests; tests/hub and tests/deploy run against its binary',
}
const NOT_RUN = {
  'build/wasm-load.mjs': 'fails on main since the binding gained the call it expected to lack; to be repaired, then wired in',
}

const workflows = fs.readdirSync(path.join(repo, '.github/workflows')).filter(f => f.endsWith('.yml'))
  .map(f => read(`.github/workflows/${f}`)).join('\n')
const problems = []

// 1. crates
const members = [...read('Cargo.toml').matchAll(/^members = \[(.*?)\]/gms)][0][1].match(/"[^"]+"/g).map(s => s.slice(1, -1))
for (const dir of members) {
  const name = read(`${dir}/Cargo.toml`).match(/^name = "([^"]+)"/m)[1]
  const named = new RegExp(`-p ${name}(?![\\w-])`).test(workflows) || /--workspace/.test(workflows)
  if (!named && !NOT_TESTED[name]) problems.push(`crate ${name} (${dir}) is named by no workflow (-p ${name}); add it to a test or clippy step, or to NOT_TESTED with the reason`)
}

// 2. test targets of tests/Cargo.toml
const prefixes = new Set()
for (const line of workflows.split('\n')) {
  const m = line.match(/name = "(.*?)_\[a-z0-9_\]\*/)
  if (m) for (const p of m[1].replace(/\\\(|\\\)/g, '').split('\\|')) prefixes.add(p)
}
const targets = [...read('tests/Cargo.toml').matchAll(/^\[\[test\]\]\s*\nname = "([^"]+)"/gm)].map(m => m[1])
for (const t of targets) {
  const prefix = t.split('_')[0]
  const literal = new RegExp(`(--test |test\\b.*)${t}(?![\\w])`).test(workflows)
  if (!prefixes.has(prefix) && !literal) problems.push(`test target ${t} (tests/Cargo.toml) is run by no workflow (no --test ${t}, no selection of ${prefix}_*)`)
}

// 3. the web app's test scripts
const scripts = JSON.parse(read('app/web/package.json')).scripts
for (const [key, command] of Object.entries(scripts)) {
  if (!key.startsWith('test:') || key.includes(' ')) continue
  for (const file of command.match(/\.\.\/\.\.\/(?:tests\/web|demo)\/[\w./-]+\.(?:mjs|js)/g) ?? []) {
    const rest = file.replace('../../', '').replace('tests/web/', '')
    if (NOT_RUN[rest]) continue
    if (!workflows.includes(rest) && !workflows.includes(`npm run ${key}`)) problems.push(`${file} (npm run ${key}) is run by no workflow`)
  }
}

if (problems.length) {
  for (const p of problems) console.log(`::error::${p}`)
  process.exit(1)
}
console.log(`ci_coverage: ${members.length} crates, ${targets.length} test targets, ${Object.keys(scripts).filter(k => k.startsWith('test:')).length} web test scripts: every one is run by a workflow`)
