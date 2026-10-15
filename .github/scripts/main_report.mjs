// Report each part independently after every delivery stage has finished.
import fs from 'node:fs'
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { fileURLToPath } from 'node:url'
import { checkNames } from './main_plan.mjs'
export function outcomes(config, jobs) {
  return config.checks.map(({part}) => {
    const pipelines=config.pipelines.filter(row=>row.part===part)
    const checked=config.checks.find(row=>row.part===part)
    const expected=[checked.delivering ? `${part} / ${checkNames[part]}` : checkNames[part],...pipelines.flatMap(row=>[row.prepare_job,row.publish_job])]
    if(part!=='site') expected.push('Plan and shared checks')
    const failed=expected.filter(name=>!jobs.some(job=>job.name===name && job.conclusion==='success'))
    let state=failed.length ? 'failure' : 'success'
    if(state==='success' && pipelines.length && ['web','hub','connector','site'].includes(part)) {
      const marker=part==='site'?'Site delivered':'Release published'
      if(!jobs.find(job=>job.name===pipelines[0].publish_job)?.steps?.some(step=>step.name===marker && step.conclusion==='success')) state='overtaken'
    }
    return { part, state, delivering:pipelines.length>0, reason:failed.join(', ') }
  })
}
if (process.argv[1] && fileURLToPath(import.meta.url)===process.argv[1]) {
  const temp=process.env.RUNNER_TEMP
  const bytes=fs.readFileSync(`${temp}/main-plan/main-plan.json`)
  if(createHash('sha256').update(bytes).digest('hex')!==process.env.MAIN_PLAN_DIGEST) throw Error('report plan was replaced')
  const config=JSON.parse(bytes)
  if (config.commit!==process.env.GITHUB_SHA) throw Error('report source mismatch')
  const pages=JSON.parse(execFileSync('gh',['api','--paginate','--slurp',`repos/${process.env.GITHUB_REPOSITORY}/actions/runs/${process.env.GITHUB_RUN_ID}/jobs?filter=latest&per_page=100`],{encoding:'utf8'}))
  const titles={web:'Web',hub:'Hub',connector:'Connector',apple:'iOS'}
  let failed=false
  for(const result of outcomes(config,pages.flatMap(page=>page.jobs))) {
    if(result.state==='failure') failed=true
    fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, `- ${result.part}: ${result.state}${result.reason ? ` (${result.reason})` : ''}\n`)
    if(!result.delivering || !titles[result.part] || result.state==='overtaken') continue
    const build=['connector','apple'].includes(result.part)
    try { execFileSync('sh',['.github/scripts/deploy_issue.sh',titles[result.part]],{stdio:'inherit',env:{...process.env,
      GH_REPO:process.env.GITHUB_REPOSITORY,TAG:result.part==='apple'?config.version:`${result.part}-v${config.version}`,
      WHAT:build?'build':'update',LABEL:build?'build-failed':'deploy-failed',RESULT:result.state,REASON:result.reason,
      RUNNING:build?`the commit built, ${config.commit}`:'',
      RUN_URL:`https://github.com/${process.env.GITHUB_REPOSITORY}/actions/runs/${process.env.GITHUB_RUN_ID}`,
      HEALTH_URL:result.part==='hub'?'https://hub.trommi.com/healthz':result.part==='web'?'https://app.trommi.com/gen/build.txt':'',
    }}) } catch(error) { failed=true; console.error(error.message) }
  }
  if(failed) process.exitCode=1
}
