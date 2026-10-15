import assert from 'node:assert/strict'
import {baselines} from './main_baselines.mjs'
const newest='b'.repeat(40),older='a'.repeat(40)
const old={head_sha:older,jobs:[{name:'Plan and shared checks',conclusion:'success',steps:[{name:'Shared core passed',conclusion:'success'}]},{name:'apple / iOS client (check)',conclusion:'success'}]}
const busy={head_sha:newest,jobs:[{name:'Plan and shared checks',conclusion:'success',steps:[{name:'Shared core passed',conclusion:'success'}]},{name:'web / Web app (check)',conclusion:'success'},{name:'apple / iOS client (check)',conclusion:'in_progress'}]}
assert.deepEqual(baselines([busy,old]),{web:newest,apple:older,core:newest})
const site={head_sha:newest,jobs:[{name:'Plan and shared checks',conclusion:'success',steps:[{name:'Shared core passed',conclusion:'skipped'}]},{name:'site / Site (test)',conclusion:'success'}]}
assert.deepEqual(baselines([site,old]),{site:newest,core:older,apple:older})
assert.equal(baselines([{head_sha:newest,jobs:[{name:'Web app (check)',conclusion:'failure'}]}]).web,undefined)
console.log('main baselines: passing checks reused during slow/failed deliveries; skipped core cannot advance its baseline')
