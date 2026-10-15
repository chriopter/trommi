import test from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, writeFileSync, copyFileSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { spawnSync } from 'node:child_process'
const root = mkdtempSync(join(tmpdir(), 'trommi-native-gate-'))
const scripts = join(root,'.github/scripts'), bin=join(root,'bin')
mkdirSync(scripts,{recursive:true});mkdirSync(bin)
const script=(file,body)=>writeFileSync(file,'#!/bin/bash\nset -eu\n'+body,{mode:0o755})
for(const file of ['main_native_pending.sh','ios_release_gate.sh']) copyFileSync(new URL(file,import.meta.url),join(scripts,file))
script(join(bin,'gh'),`printf '%s\\n' '{"path":".github/workflows/main.yml"}'\n`)
script(join(scripts,'main_release_gate.sh'),'[ "${CHECKS:-green}" = green ]\n')
script(join(scripts,'inputs.sh'),`printf 'ios %064d\\n' 1\n`)
script(join(scripts,'ios_delivered.sh'),'[ "${DELIVERED:-}" != "${1:-ios}" ] || printf "%064d\\n" 1\n')
script(join(scripts,'mac_release.sh'),'echo "mode=${MAC_MODE:-none}" >> "$GITHUB_OUTPUT"; echo "mode: ${MAC_MODE:-none}"\n')
const env={...process.env,PATH:bin+':'+process.env.PATH,GH_TOKEN:'test-only',GITHUB_REPOSITORY:'trommi/trommi',RUN_ID:'42',COMMIT:'a'.repeat(40),RUNNER_TEMP:root}
const run=(file,args=[],extra={})=>spawnSync('bash',[join(scripts,file),...args],{cwd:root,env:{...env,...extra},encoding:'utf8'})
test('queued TestFlight destinations recheck their own delivery independently',()=>{
  for(const [pipeline,destination,other] of [['ios','ios','mac'],['mac-testflight','mac','ios']]) {
    const done=run('main_native_pending.sh',[pipeline],{DELIVERED:destination})
    assert.equal(done.status,0,done.stderr);assert.equal(done.stdout,'deliver=false\n')
    const pending=run('main_native_pending.sh',[pipeline],{DELIVERED:other})
    assert.equal(pending.status,0,pending.stderr);assert.equal(pending.stdout,'deliver=true\n')
  }
})
test('a late duplicate has a distinct no-op status; failed checks still refuse',()=>{
  const done=run('ios_release_gate.sh',['wait'],{DELIVERED:'ios'})
  assert.equal(done.status,3);assert.match(done.stdout,/already/)
  assert.equal(run('ios_release_gate.sh',['wait'],{DELIVERED:'ios',CHECKS:'failed'}).status,1)
  assert.equal(run('ios_release_gate.sh',['wait'],{DELIVERED:'mac'}).status,0)
  assert.equal(run('ios_release_gate.sh',['wait'],{DELIVERED:'mac',TF_DESTINATION:'mac'}).status,3)
})
test('direct Mac recheck emits only a valid output flag and rejects unknown destinations',()=>{
  for(const [mode,flag] of [['none','false'],['build','true']]) {
    const result=run('main_native_pending.sh',['mac-direct'],{MAC_MODE:mode})
    assert.equal(result.status,0,result.stderr);assert.equal(result.stdout,`deliver=${flag}\n`)
  }
  assert.equal(run('main_native_pending.sh',['unknown']).status,2)
})
test('a no-op never processes an absent TestFlight build or attests an absent DMG',()=>{
  const workflow=readFileSync(new URL('../workflows/main-delivery.yml',import.meta.url),'utf8')
  for(const name of ['main-process-ios','main-process-mac','main-notes-ios']) {
    const call=workflow.split('uses: ./.github/actions/'+name+'\n')[1]?.split('    - ')[0]
    assert.ok(call?.includes("env.MAIN_READY != 'already'"),name)
  }
  const publisher=readFileSync(new URL('../actions/main-publish-mac-direct/action.yml',import.meta.url),'utf8')
  assert.equal((publisher.match(/if: steps.pending.outputs.mode == 'build'/g)||[]).length,5)
})
process.on('exit',()=>rmSync(root,{recursive:true,force:true}))
