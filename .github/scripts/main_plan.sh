#!/bin/bash
# Main only. Retry each independently undelivered part, including changes from a failed earlier run.
set -euo pipefail
node .github/scripts/main_baselines.mjs > "$RUNNER_TEMP/main-baselines.json"
sh .github/scripts/inputs.sh > "$RUNNER_TEMP/main-inputs"
sh .github/scripts/deployed.sh "$RUNNER_TEMP/main-inputs" > "$RUNNER_TEMP/main-delivery"
ios=$(sed -n 's/^ios //p' "$RUNNER_TEMP/main-inputs")
delivered=$(sh .github/scripts/ios_delivered.sh) || delivered=
if [ -n "$delivered" ] && [ "$delivered" = "$ios" ]; then tf=false; else tf=true; fi
export VERSION=$((2000 + GITHUB_RUN_NUMBER)) COMMIT=$GITHUB_SHA
GITHUB_OUTPUT="$RUNNER_TEMP/main-mac" bash .github/scripts/mac_release.sh check
mac=false
if grep -q '^mode=build$' "$RUNNER_TEMP/main-mac"; then mac=true; fi
export MAIN_TF=$tf MAIN_MAC=$mac
node --input-type=module <<'JS'
import fs from 'node:fs'
import { createHash } from 'node:crypto'
import { plan } from './.github/scripts/main_plan.mjs'
const temp = process.env.RUNNER_TEMP
const delivery = Object.fromEntries(fs.readFileSync(`${temp}/main-delivery`, 'utf8').trim().split('\n').map(line => { const [part, flag] = line.split('='); return [part, flag === 'true'] }))
delivery.updater = delivery.hub
delivery.ios = process.env.MAIN_TF === 'true'; delivery.mac = process.env.MAIN_MAC === 'true'
const observed=JSON.parse(fs.readFileSync(`${temp}/main-baselines.json`,'utf8'))
const result=plan(observed.files,delivery,false,process.env.MAIN_REQUESTED??'',observed.changed)
const inputs = fs.readFileSync(`${temp}/main-inputs`, 'utf8').trim().split('\n').map(line => `${line} ${result.delivery[line.split(' ')[0]] || false}`).join('\n')
fs.writeFileSync(`${temp}/main-plan.json`, JSON.stringify({ ...result, inputs, version: process.env.VERSION, commit: process.env.GITHUB_SHA }))
for (const [key, value] of Object.entries({ core:result.core, checks:JSON.stringify({include:result.check_only}), parts:JSON.stringify({include:result.releases}), deliver:result.deliver, version:process.env.VERSION, digest:createHash('sha256').update(fs.readFileSync(`${temp}/main-plan.json`)).digest('hex') })) fs.appendFileSync(process.env.GITHUB_OUTPUT, `${key}=${value}\n`)
fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, `Checks: ${result.checks.map(row => row.part).join(', ') || 'none'}; shared core: ${result.core}. Release ${process.env.VERSION}.\n`)
JS
