// Reject a forged/stale artifact even when it exists in a main run. No network or credentials.
import assert from 'node:assert/strict'
import fs from 'node:fs'
import {createHash} from 'node:crypto'
import { join, resolve } from 'node:path'
import { tmpdir } from 'node:os'
import { spawnSync } from 'node:child_process'
const dir = fs.mkdtempSync(join(tmpdir(), 'trommi-main-gates-'))
try {
  fs.mkdirSync(join(dir, 'bin'))
  const gh = join(dir, 'bin/gh')
  fs.writeFileSync(gh, `#!/usr/bin/env python3
import sys,json,os
f=json.load(open(os.environ['MAIN_FIXTURE']))
if any('/jobs?' in arg for arg in sys.argv):
 if '--slurp' in sys.argv: print(json.dumps([{'jobs':f['jobs']}]))
 else:
  for job in f['jobs']: print(job['name']+'\\t'+job['conclusion'])
else: print(json.dumps(f['run']))
`)
  fs.chmodSync(gh, 0o755)
  const sha = 'a'.repeat(40), repo = 'test/repository'
  const good = { run:{ path:'.github/workflows/main.yml', head_branch:'main', event:'push', head_sha:sha, repository:{full_name:repo}, head_repository:{full_name:repo} }, jobs:[{name:'Plan and shared checks',conclusion:'success'},{name:'web / Web app (check)',conclusion:'success'},{name:'site / Site (test)',conclusion:'success'},{name:'web / release / Sign and prepare',conclusion:'success'}] }
  const env = { ...process.env, PATH:join(dir,'bin')+':'+process.env.PATH, MAIN_FIXTURE:join(dir,'fixture.json'), GH_TOKEN:'test-only', GITHUB_REPOSITORY:repo, RUN_ID:'1', GITHUB_RUN_ID:'1', COMMIT:sha, GITHUB_SHA:sha, RUNNER_TEMP:dir, GITHUB_ENV:join(dir,'env') }
  const run = (fixture, part='web') => {
    fs.writeFileSync(env.MAIN_FIXTURE, JSON.stringify(fixture))
    return spawnSync('bash', ['.github/scripts/main_release_gate.sh',part], {env,encoding:'utf8'})
  }
  assert.equal(run(good).status,0)
  const manual=structuredClone(good);manual.run.event='workflow_dispatch';assert.equal(run(manual).status,0)
  for (const mutate of [
    f => { f.run.repository.full_name='other/repo' },
    f => { f.run.head_repository.full_name='other/repo' },
    f => { f.run.head_branch='release/test' },
    f => { f.run.event='pull_request' },
    f => { f.run.head_sha='b'.repeat(40) },
    f => { f.run.path='.github/workflows/pr-check.yml' },
    f => { f.jobs[0].conclusion='failure' },
    f => { f.jobs[0].conclusion='skipped' },
    f => { f.jobs[1].conclusion='failure' },
    f => { f.jobs[1].name='A renamed check' },
  ]) { const f=structuredClone(good); mutate(f); assert.notEqual(run(f).status,0) }
  const site=structuredClone(good);site.jobs[0].conclusion='failure';assert.equal(run(site,'site').status,0)
  assert.notEqual(run(good,'apple').status,0)
  fs.mkdirSync(join(dir,'main-plan'));fs.mkdirSync(join(dir,'main-previous'))
  const plan=JSON.stringify({commit:sha,version:'2001',checks:[{part:'web'}],pipelines:[{part:'web',pipeline:'release',prepare_role:'release-prepared',prepare_job:'web / release / Sign and prepare'}]})
  fs.writeFileSync(join(dir,'main-plan/main-plan.json'),plan)
  env.MAIN_PLAN_DIGEST=createHash('sha256').update(plan).digest('hex')
  const proof={commit:sha,version:'2001',part:'web',role:'release-prepared',data:{manifest:'c'.repeat(64),tag:'web-v2001'}}
  env.MAIN_PREVIOUS_DIGEST=createHash('sha256').update(JSON.stringify(proof)).digest('hex')
  const previous = (fixture, value) => {
    fs.writeFileSync(env.MAIN_FIXTURE,JSON.stringify(fixture))
    fs.writeFileSync(join(dir,'main-previous/main-result.json'),JSON.stringify(value))
    return spawnSync(process.execPath,[resolve('.github/scripts/main_metadata.mjs'),'previous','web','release-prepared'],{env,encoding:'utf8'})
  }
  assert.equal(previous(good,proof).status,0)
  const failed=structuredClone(good);failed.jobs.at(-1).conclusion='failure'
  assert.notEqual(previous(failed,proof).status,0,'an existing proof cannot override a failed signing job')
  assert.notEqual(previous(good,{...proof,commit:'b'.repeat(40)}).status,0)
  assert.notEqual(previous(good,{...proof,data:{secret_key:'forbidden'}}).status,0)
  assert.notEqual(previous(good,{...proof,data:{manifest:'injected\nVALUE=yes'}}).status,0)
  const file=join(dir,'artifact');fs.writeFileSync(file,'checked bytes')
  const hash=createHash('sha256').update('checked bytes').digest('hex')
  assert.equal(spawnSync('sh',['.github/scripts/main_artifact.sh',hash,file],{env}).status,0)
  fs.writeFileSync(file,'replacement bytes')
  assert.notEqual(spawnSync('sh',['.github/scripts/main_artifact.sh',hash,file],{env}).status,0)
  fs.writeFileSync(join(dir,'main-plan/main-plan.json'),plan+' ')
  assert.notEqual(previous(good,proof).status,0,'plan replacement must fail')
  console.log('main gates: source/fork/branch/event/check failures and forged/stale proofs refused; valid gates pass')
} finally { fs.rmSync(dir,{recursive:true,force:true}) }
