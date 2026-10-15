// Execute the real file gates with a source release/ directory beside the downloaded artifact.
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'
const sha = bytes => createHash('sha256').update(bytes).digest('hex')
const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'main-web-paths-'))
const root = path.resolve('.')
try {
  const temp = path.join(dir, 'temp'), artifact = path.join(temp, 'main-web-release')
  fs.mkdirSync(artifact, { recursive:true })
  fs.mkdirSync(path.join(dir, 'release'))
  fs.writeFileSync(path.join(dir, 'release/manifest.sh'), 'source script, not a web artifact')
  const commit = 'a'.repeat(40), file = 'checked app bytes'
  const manifest = JSON.stringify({ product:'trommi-web', repository:'chriopter/trommi', worker:'trommi-app', host:'app.trommi.com', commit, version:'2001', files:[{name:'app.js',sha256:sha(file)}] })
  fs.writeFileSync(path.join(artifact, 'app.js'), file)
  fs.writeFileSync(path.join(artifact, 'manifest.json'), manifest)
  fs.writeFileSync(path.join(artifact, 'checksums.txt'), `${sha(file)}  app.js\n${sha(manifest)}  manifest.json\n`)
  const gates = [['main-attest-web','The files are the ones the build named'],['main-deliver-web','The files are the attested ones']]
  for (const [action, step] of gates) {
    const source = fs.readFileSync(`${root}/.github/actions/${action}/action.yml`, 'utf8')
    const block = source.split(`  - name: ${step}\n`)[1]
    const run = block.match(/    run: \|\n((?:      .*\n|\n)+)/)[1].replace(/^      /gm,'')
    execFileSync('bash', ['-c',run], { cwd:dir, env:{...process.env,RUNNER_TEMP:temp,GITHUB_OUTPUT:path.join(dir,'outputs'),GITHUB_REPOSITORY:'chriopter/trommi',COMMIT:commit,VERSION:'2001',MANIFEST:sha(manifest)},stdio:'pipe' })
  }
  console.log('main web paths: source release scripts cannot contaminate downloaded artifact gates')
} finally { fs.rmSync(dir, {recursive:true,force:true}) }
