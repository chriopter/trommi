// Actual browser startup after the old ten-second cutoff; early exits retain their diagnostic.
import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { launchChromium } from '../../../app/web/dev/cdp.mjs'
const folder = await fs.mkdtemp(path.join(os.tmpdir(), 'trommi-browser-start-'))
const saved = process.env.CHROMIUM
const program = saved || 'chromium'
const savedProgram = process.env.TROMMI_BROWSER_START_PROGRAM
process.env.TROMMI_BROWSER_START_PROGRAM = program
let browser
try {
  const delayed = path.join(folder, 'delayed')
  await fs.writeFile(delayed, '#!/bin/sh\nsleep 11\nexec "$TROMMI_BROWSER_START_PROGRAM" "$@"\n', { mode: 0o700 })
  process.env.CHROMIUM = delayed
  const began = performance.now()
  browser = await launchChromium()
  const page = await browser.page()
  const value = await page.send('Runtime.evaluate', { expression: '1 + 1', returnByValue: true })
  assert.equal(value.result.value, 2)
  console.log(`browser-start: delayed real browser ready in ${Math.round(performance.now() - began)} ms (one launch, beyond the old 10000 ms cutoff)`)
  await browser.close(); browser = null
  const failed = path.join(folder, 'failed')
  await fs.writeFile(failed, '#!/bin/sh\necho browser-start-fixture-diagnostic >&2\nexit 7\n', { mode: 0o700 })
  process.env.CHROMIUM = failed
  await assert.rejects(launchChromium(), error => /exit 7/.test(error.message) && /browser-start-fixture-diagnostic/.test(error.message))
  console.log('browser-start: early exit reports its code and stderr')
} finally {
  await browser?.close()
  if (saved === undefined) delete process.env.CHROMIUM
  else process.env.CHROMIUM = saved
  if (savedProgram === undefined) delete process.env.TROMMI_BROWSER_START_PROGRAM
  else process.env.TROMMI_BROWSER_START_PROGRAM = savedProgram
  await fs.rm(folder, { recursive: true, force: true })
}
