// Fails when a test exists that no workflow runs. Run by pr-check.yml (job "Which parts changed") on every pull request:
//   node .github/scripts/ci_coverage.mjs
// 1. every crate of the workspace (Cargo.toml members) is named with `-p <crate>` in a workflow, or is listed in NOT_TESTED;
// 2. every test target of tests/Cargo.toml is run: its name stands in a workflow, or the workflow picks the targets of
//    its prefix by a `sed 's/^name = "\(prefix_...` selection;
// 3. every file that the web app's test scripts (app/web/package.json, test:*) name stands in a workflow, or is in
//    NOT_RUN (with its reason). A new crate, test target or browser test that nobody wired in fails here.
// 4. every job name that a workflow or script looks up in a run's jobs (`select(.name == "…")`, `conclusion '…'` of
//    ios_release_gate.sh, `grep -F… "…" …jobs.txt`, the job argument of ios_delivered.sh's `runs`) is the name of a
//    job of that workflow (the file's own, or the one it names by `.path == ".github/workflows/…"`): a renamed job
//    must not silently stop a delivery (issue #224: "Checks / iOS client (check)" after the checks were inlined).
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
  // 'dir/file.mjs': 'why it is not run (and what would let it run)'
}

const files = [
  ...fs.readdirSync(path.join(repo, '.github/workflows')).filter(f => f.endsWith('.yml')).map(f => `.github/workflows/${f}`),
  ...fs.readdirSync(path.join(repo, '.github/actions')).map(d => `.github/actions/${d}/action.yml`),   // the composite actions hold the steps
]
const workflows = files.map(read).join('\n')
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

// 4. job names looked up in a run
// the display names of a workflow's jobs; a name with ${{ … }} (a matrix) matches anything there; a job that calls a
// reusable workflow of this repository has the names "<its name> / <a job name of that workflow>"
const jobNames = (file, seen = new Set()) => {
  if (seen.has(file) || !fs.existsSync(path.join(repo, file))) return []
  seen.add(file)
  const lines = read(file).split('\n')
  const names = []
  let inJobs = false, job = null
  const done = () => {
    if (!job) return
    const own = (job.name ?? job.id).replace(/^(['"])(.*)\1$/, '$2')
    if (job.uses) for (const inner of jobNames(job.uses.replace(/^\.\//, ''), seen)) names.push(`${own} / ${inner}`)
    else names.push(own)
  }
  for (const line of lines) {
    if (/^\S/.test(line)) { done(); job = null; inJobs = /^jobs:\s*$/.test(line); continue }
    if (!inJobs) continue
    const id = line.match(/^  ([\w-]+):\s*$/)
    if (id) { done(); job = { id: id[1] }; continue }
    const field = line.match(/^    (name|uses):\s*(.+?)\s*$/)
    if (job && field) job[field[1]] = field[2]
  }
  done()
  return names
}
const matches = (names, wanted) => names.some(n => n === wanted ||
  (n.includes('${{') && new RegExp(`^${n.split(/\$\{\{.*?\}\}/).map(p => p.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('.+')}$`).test(wanted)))
const lookups = [
  /select\(\.name == "([^"$']+)"/g,
  /\bconclusion '([^']+)'/g,
  /grep -Fx?q "([^"$]+)" "[^"]*jobs\.txt"/g,
]
const sources = [
  ...files.filter(f => f.startsWith('.github/workflows/')),
  ...fs.readdirSync(path.join(repo, '.github/scripts')).filter(f => f !== 'ci_coverage.mjs').map(f => `.github/scripts/${f}`),
  ...fs.readdirSync(path.join(repo, 'dev')).filter(f => fs.statSync(path.join(repo, 'dev', f)).isFile()).map(f => `dev/${f}`),
]
let looked = 0
for (const file of sources) {
  const text = read(file)
  const own = text.match(/\.path == "\.github\/workflows\/([\w.-]+)"/)?.[1]
  const workflow = file.startsWith('.github/workflows/') ? file : own ? `.github/workflows/${own}` : null
  const refs = []
  for (const re of lookups) for (const m of text.matchAll(re)) refs.push([workflow, m[1]])
  // ios_delivered.sh: runs <workflow file> <event> '<job>' (a workflow no longer here: its old runs, not checked)
  for (const m of text.matchAll(/\bruns ([\w.-]+\.yml) \w+ '([^']+)'/g)) refs.push([`.github/workflows/${m[1]}`, m[2], true])
  for (const [wf, name, gone] of refs) {
    if (gone && !fs.existsSync(path.join(repo, wf))) continue
    looked++
    const names = wf ? jobNames(wf) : files.filter(f => f.startsWith('.github/workflows/')).flatMap(f => jobNames(f))
    if (!matches(names, name)) problems.push(`${file} looks for the job "${name}" in a run of ${wf ?? 'any workflow'}, which has no job of that name`)
  }
}

if (problems.length) {
  for (const p of problems) console.log(`::error::${p}`)
  process.exit(1)
}
console.log(`ci_coverage: ${members.length} crates, ${targets.length} test targets, ${Object.keys(scripts).filter(k => k.startsWith('test:')).length} web test scripts: every one is run by a workflow; ${looked} job names looked up in runs: every one exists`)
