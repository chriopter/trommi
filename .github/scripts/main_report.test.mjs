import assert from 'node:assert/strict'
import {plan,checkNames} from './main_plan.mjs'
import {outcomes} from './main_report.mjs'
const config={...plan(['app/web/public/desk.css'],{web:true,hub:true}),version:'2001',commit:'a'.repeat(40)}
const jobs=[{name:'Plan and shared checks',conclusion:'success'},...config.checks.map(row=>({name:`${row.part} / ${checkNames[row.part]}`,conclusion:'success'})),...config.pipelines.flatMap(row=>[{name:row.prepare_job,conclusion:'success'},{name:row.publish_job,conclusion:'success',steps:[{name:'Release published',conclusion:'success'}]}])]
assert.ok(outcomes(config,jobs).every(part=>part.state==='success'))
const failed=jobs.map(job=>job.name===config.pipelines[0].prepare_job?{...job,conclusion:'failure'}:job)
assert.deepEqual(outcomes(config,failed).map(x=>[x.part,x.state]),[['web','failure'],['hub','success']])
assert.equal(outcomes(config,jobs.filter(job=>job.name!==config.pipelines[0].prepare_job))[0].state,'failure')
assert.equal(outcomes(config,jobs.map(job=>job.name===config.pipelines[0].publish_job?{...job,steps:[{name:'Release published',conclusion:'skipped'}]}:job))[0].state,'overtaken')
console.log('main reports: actual per-part job/step conclusions; independent failures and overtaken releases pass')
