// A slow/failed delivery must not make another part repeat checks it already passed.
import { execFileSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { checkNames,changedFor } from './main_plan.mjs'
export function baselines(runs) {
  const found={}
  for(const run of runs) {
    if(!/^[0-9a-f]{40}$/.test(run.head_sha)) continue
    for(const part of Object.keys(checkNames)) if(!found[part] && run.jobs.some(job=>[checkNames[part],`${part} / ${checkNames[part]}`].includes(job.name) && job.conclusion==='success')) found[part]=run.head_sha
    if(!found.core && run.jobs.some(job=>job.name==='Plan and shared checks' && job.conclusion==='success' && job.steps?.some(step=>step.name==='Shared core passed' && step.conclusion==='success'))) found.core=run.head_sha
  }
  return found
}
if(process.argv[1] && fileURLToPath(import.meta.url)===process.argv[1]) {
  const api=(path,pages=false)=>JSON.parse(execFileSync('gh',['api',...(pages?['--paginate','--slurp']:[]),path],{encoding:'utf8'}))
  let runs=[]
  try { runs=api(`repos/${process.env.GITHUB_REPOSITORY}/actions/workflows/main.yml/runs?branch=main&event=push&per_page=50`).workflow_runs } catch { /* first main run: check everything */ }
  const seen=[]
  for(const run of runs) {
    if(String(run.id)===process.env.GITHUB_RUN_ID) continue
    if(run.head_branch!=='main' || run.repository?.full_name!==process.env.GITHUB_REPOSITORY || run.head_repository?.full_name!==process.env.GITHUB_REPOSITORY) continue
    const jobs=api(`repos/${process.env.GITHUB_REPOSITORY}/actions/runs/${run.id}/jobs?filter=latest&per_page=100`,true).flatMap(page=>page.jobs)
    seen.push({...run,jobs})
    if(Object.keys(baselines(seen)).length===7) break
  }
  const bases=baselines(seen),changed={},files=new Set()
  for(const part of ['core',...Object.keys(checkNames)]) {
    let paths=null
    if(bases[part]) try { paths=execFileSync('git',['diff','--name-only',bases[part],'HEAD'],{encoding:'utf8',stdio:['ignore','pipe','ignore']}).trim().split('\n').filter(Boolean) } catch { /* missing history: check it */ }
    changed[part]=paths===null || changedFor(part,paths)
    for(const path of paths??[]) files.add(path)
  }
  console.log(JSON.stringify({changed,files:[...files]}))
}
