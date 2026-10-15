// Only public build identity and gate results cross jobs. Never include an environment dump.
import fs from 'node:fs'
import { execFileSync } from 'node:child_process'
import { randomUUID, createHash } from 'node:crypto'
const temp = process.env.RUNNER_TEMP
const read = path => JSON.parse(fs.readFileSync(path, 'utf8'))
const digest = bytes => createHash('sha256').update(bytes).digest('hex')
const planBytes = fs.readFileSync(`${temp}/main-plan/main-plan.json`)
if (digest(planBytes) !== process.env.MAIN_PLAN_DIGEST) throw Error('plan is not the one its producing job exported')
const config = JSON.parse(planBytes)
if (config.commit !== process.env.GITHUB_SHA || !/^[0-9a-f]{40}$/.test(config.commit) || !/^[1-9][0-9]{0,11}$/.test(config.version)) throw Error('invalid main build identity')
const writeEnv = (name, value) => {
  const marker = `INPUT_${randomUUID()}`
  fs.appendFileSync(process.env.GITHUB_ENV, `${name}<<${marker}\n${value}\n${marker}\n`)
}
const [command, part, role] = process.argv.slice(2)
if (!config.checks.some(row => row.part === part)) throw Error('part was not planned')
const keys = ['manifest','tag','published','app_version','app_build','mac_version','ready','web_manifest','mac_dmg_sha']
const proof = (path, expectedRole) => {
  const value = read(path)
  if (value.commit !== config.commit || value.version !== config.version || value.part !== part || (expectedRole && value.role !== expectedRole)) throw Error('gate proof is not of this build')
  for (const [key, field] of Object.entries(value.data ?? {})) if (!keys.includes(key) || typeof field !== 'string' || field.includes('\n')) throw Error('invalid public gate field')
  return value
}
if (command === 'configure') {
  writeEnv('MAIN_PARTS', config.inputs)
  writeEnv('MAIN_VERSION', config.version)
  writeEnv('MAIN_HUB_CHANGED', String(config.delivery.hub))
} else if (command === 'checked') {
  fs.writeFileSync(`${temp}/main-checked.json`, JSON.stringify({ commit:config.commit, version:config.version, part }))
} else if (command === 'verify-check') {
  proof(`${temp}/main-checked/main-checked.json`)
} else if (command === 'built') {
  const paths = part === 'web' ? {release_digest:'webrel/manifest.json',web_digest:'out/manifest.json'} : ['hub','connector'].includes(part) ? {release_digest:'out/manifest.json'} : part === 'apple' ? {native_digest:'out/checksums.txt'} : part === 'site' ? {site_digest:`${temp}/site-inputs.tar.gz`} : {}
  for (const [name,path] of Object.entries(paths)) fs.appendFileSync(process.env.GITHUB_OUTPUT, `${name}=${digest(fs.readFileSync(path))}\n`)
} else if (command === 'previous') {
  if (!/^[a-z-]+$/.test(role) || !/^[1-9][0-9]*$/.test(process.env.GITHUB_RUN_ID ?? '')) throw Error('invalid preceding job identity')
  const pages = JSON.parse(execFileSync('gh', ['api', '--paginate', '--slurp', `repos/${process.env.GITHUB_REPOSITORY}/actions/runs/${process.env.GITHUB_RUN_ID}/jobs?filter=latest&per_page=100`], { encoding:'utf8' }))
  const pipeline = config.pipelines.find(row => row.part === part && row.prepare_role === role)
  if (!pipeline || !pages.flatMap(page => page.jobs).some(job => job.name === pipeline.prepare_job && job.conclusion === 'success')) throw Error('the preceding job did not pass')
  if (digest(fs.readFileSync(`${temp}/main-previous/main-result.json`)) !== process.env.MAIN_PREVIOUS_DIGEST) throw Error('preceding proof was replaced after its job')
  const previous = proof(`${temp}/main-previous/main-result.json`, role)
  for (const [key, value] of Object.entries(previous.data ?? {})) writeEnv(`MAIN_${key.toUpperCase()}`, value)
} else if (command === 'result') {
  const previousPath = `${temp}/main-previous/main-result.json`
  const data = fs.existsSync(previousPath) ? proof(previousPath).data : {}
  const fields = Object.fromEntries(keys.filter(key => process.env[`MAIN_FIELD_${key}`] !== undefined).map(key => [key, process.env[`MAIN_FIELD_${key}`]]))
  for (const [key, value] of Object.entries(fields)) {
    if (!keys.includes(key) || typeof value !== 'string' || value.includes('\n')) throw Error('invalid result field')
    if (value !== '') data[key] = value
  }
  if (part === 'apple' && role === 'mac-direct-prepared' && data.ready === 'yes') data.mac_dmg_sha = digest(fs.readFileSync(`${temp}/mac-direct/Trommi-macOS.dmg`))
  const result = JSON.stringify({ commit:config.commit, version:config.version, part, role, data })
  fs.writeFileSync(`${temp}/main-result.json`, result)
  fs.appendFileSync(process.env.GITHUB_OUTPUT, `digest=${digest(result)}\n`)
} else throw Error('unknown metadata command')
