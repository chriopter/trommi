import assert from 'node:assert/strict'
import { plan } from './main_plan.mjs'
// Actual check runs: common plan/report, the part checks, two jobs per delivery destination,
// and at most one skipped aggregate when either check-only or releasing parts are absent.
const count=p=>2+p.checks.length+2*p.pipelines.length+(!p.check_only.length || !p.releases.length ? 1:0)
const web=plan(['app/web/public/desk.css'],{web:true})
assert.equal(web.core,false);assert.deepEqual(web.checks.map(x=>x.part),['web']);assert.equal(count(web),6)
const core=plan(['core/client/src/app.rs'],{web:true,hub:true,connector:true,ios:true,mac:true})
assert.equal(core.core,true);assert.deepEqual(core.checks.map(x=>x.part),['web','hub','connector','apple','linux']);assert.equal(count(core),19)
assert.equal(core.pipelines.length,6)
assert.deepEqual(core.check_only.map(x=>x.part),['linux'])
assert.deepEqual(core.pipelines.filter(x=>x.part==='apple').map(x=>x.pipeline),['ios','mac-testflight','mac-direct'])
for(const scenario of [web,core,plan(['app/linux/src/main.rs']),plan(['tests/web/foo.test.mjs']),plan(['core/README.md']),plan([],{hub:true}),plan([],{mac:true}),plan(['site/public/index.html'])]) {
  assert.equal(new Set(scenario.checks.map(x=>x.part)).size,scenario.checks.length)
  assert.equal(new Set(scenario.pipelines.map(x=>x.prepare_job)).size,scenario.pipelines.length)
  for(const pipeline of scenario.pipelines) assert.ok(scenario.releases.some(row=>row.part===pipeline.part))
  assert.ok(scenario.check_only.every(row=>!row.build))
}
assert.deepEqual(plan(['core/README.md']).checks,[])
assert.equal(plan(['tests/web/views/foo.browser.mjs']).deliver,false)
assert.deepEqual(plan(['app/web/public/desk.css'],{hub:true}).checks.map(x=>x.part),['web','hub'])
for(const file of ['.github/actions/check-site/action.yml','.github/workflows/main.yml','.github/scripts/main_plan.test.mjs']) {
  const ci=plan([file]);assert.ok(ci.core);assert.ok(ci.checks.some(row=>row.part==='site'))
}
const manual=plan([],{web:true,hub:true,connector:true,ios:true,mac:true},false,'connector')
assert.deepEqual(manual.checks.map(x=>x.part),['connector']);assert.ok(manual.core);assert.ok(manual.pipelines.every(row=>row.part==='connector'));assert.equal(manual.delivery.hub,false)
assert.equal(plan([],{},false,'site').core,false);assert.throws(()=>plan([],{},false,'arbitrary'))
console.log('main plan: web6/core19 checks; affected-only, retries, independent native destinations and manual selection pass')
