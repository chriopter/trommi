// Pure planning for main: changed checks plus independently undelivered inputs.
// The caller obtains delivery flags using the existing signed-release/TestFlight gates.
import fs from 'node:fs'
import { fileURLToPath } from 'node:url'
const changed = fs.readFileSync(new URL('./changed_parts.sh', import.meta.url), 'utf8')
const shared = changed.match(/^shared='([^']+)'/m)[1]
const patterns = Object.fromEntries([...changed.matchAll(/^part (\w+) '([^']+)'/gm)].map(([, part, own]) => [part, new RegExp(String.raw`^(${own}|${shared}|\.github/workflows/main(?:-delivery|-part)?\.yml$|\.github/actions/main-)`)]))
const siteOwn = changed.match(/^part site '([^']+)'/m)[1]
patterns.site = new RegExp(String.raw`^(${siteOwn}|\.github/actions/|\.github/workflows/(main(?:-delivery|-part)?|pr-check)\.yml$|\.github/scripts/main_)` )
export const checkNames = { web: 'Web app (check)', hub: 'Hub (check)', connector: 'Connector (check)', apple: 'iOS client (check)', linux: 'Linux app (check)', site: 'Site (test)' }
export const changedFor = (part, files) => files.filter(file => !/(^|\/)README\.md$/.test(file)).some(file => patterns[part].test(file))
export function plan(files, delivery = {}, all = false, requested = '', observed = null) {
  delivery = { ...delivery, mac_testflight: delivery.mac_testflight ?? delivery.ios ?? false }
  const source = files.filter(file => !/(^|\/)README\.md$/.test(file))
  const checks = new Set(Object.keys(checkNames).filter(part => all || source.some(file => patterns[part].test(file))))
  let core = all || source.some(file => patterns.core.test(file))
  if (checks.size === 0 && source.length > 0) { core = true; for (const part of Object.keys(checkNames)) checks.add(part) }
  if (observed) { checks.clear(); for (const part of Object.keys(checkNames)) if (observed[part]) checks.add(part); core = !!observed.core }
  for (const part of ['web', 'hub', 'connector']) if (delivery[part]) checks.add(part)
  if (delivery.ios || delivery.mac_testflight || delivery.mac) checks.add('apple')
  if (requested) {
    if (!['all','connector','linux','site'].includes(requested)) throw Error('unknown manual part')
    checks.clear()
    for (const part of requested === 'all' ? Object.keys(checkNames) : [requested]) checks.add(part)
    core = requested !== 'site'
    if (requested !== 'all') for (const part of Object.keys(delivery)) if (part !== requested) delivery[part] = false
  }
  const pipelines = []
  const add = (part, pipeline, prepareEnv, publishEnv, prepareLock, publishLock, prepareRunner = 'ubuntu-24.04') => {
    pipelines.push({ part, pipeline, prepare_env:prepareEnv, publish_env:publishEnv, prepare_lock:prepareLock, publish_lock:publishLock, prepare_runner:prepareRunner,
      prepare_role:`${pipeline}-prepared`, final_role:`${pipeline}-delivered`,
      prepare_job:`${part} / ${pipeline} / Sign and prepare`, publish_job:`${part} / ${pipeline} / Publish and deliver`,
      prepare_timeout:part==='apple'?(pipeline==='mac-direct'?120:90):part==='connector'?15:30,
      publish_timeout:part==='apple'?(pipeline==='ios'?65:pipeline==='mac-testflight'?50:15):part==='site'?10:30,
      prepare_deployment:part==='hub' || (part==='apple' && pipeline!=='mac-direct'),
      publish_deployment:['web','connector','site'].includes(part) || (part==='apple' && pipeline!=='mac-direct') })
  }
  for (const part of ['web','hub','connector']) if (delivery[part]) add(part,'release','connector',part==='connector'?'connector':part,part==='hub'?'publish-hub':`main-sign-${part}`,part==='web'?'deploy_web':part==='hub'?'ship-hub':'publish-connector')
  if (delivery.ios) {
    add('apple','ios','ios','ios','ship-apple-ios','main-process-ios','xcode-27')
  }
  if (delivery.mac_testflight) {
    add('apple','mac-testflight','ios','ios','ship-apple-mac','main-process-mac','xcode-27')
  }
  if (delivery.mac) add('apple','mac-direct','mac','mac','ship-apple-mac-direct','main-publish-mac-direct','xcode-27')
  if (checks.has('site')) add('site','release','site','site','main-attest-site','deploy_site')
  const matrix=[...checks].map(part=>{
    const releases=pipelines.filter(row=>row.part===part)
    return { part,name:checkNames[part],runner:part==='apple'?'xcode-27':'ubuntu-24.04',build:releases.length>0,delivering:releases.length>0,pipelines:JSON.stringify({include:releases}) }
  })
  return { core,delivery,checks:matrix,check_only:matrix.filter(row=>!row.delivering),releases:matrix.filter(row=>row.delivering),pipelines,deliver:pipelines.length>0 }

}
if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const input = JSON.parse(fs.readFileSync(0, 'utf8'))
  console.log(JSON.stringify(plan(input.files, input.delivery, input.all, input.requested)))
}
