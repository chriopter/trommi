// Composite actions cannot evaluate the workflow's vars context. Bind it at their call sites.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
const read = path => readFileSync(new URL('../' + path, import.meta.url), 'utf8')
test('Apple delivery configuration is passed from the workflow into action inputs', () => {
  const workflow = read('workflows/main-delivery.yml')
  for (const [name, keys] of Object.entries({
    'main-upload-ios': ['non-exempt-encryption', 'export-compliance-code', 'uploader'],
    'main-upload-mac': ['uploader'], 'main-process-ios': ['tester'], 'main-process-mac': ['tester'],
  })) {
    const action = read('actions/' + name + '/action.yml')
    assert.doesNotMatch(action, /\$\{\{[^}]*\bvars\./, name)
    const call = workflow.split('uses: ./.github/actions/' + name + '\n')[1]?.split('    - ')[0]
    assert.ok(call, name)
    for (const key of keys) {
      assert.ok(action.includes('inputs.' + key), name + ': ' + key)
      assert.ok(call.includes(key + ': ${{ vars.'), name + ': ' + key)
    }
  }
})
