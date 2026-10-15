// Cache input contract: documentation is ignored; source, executable mode and helper changes invalidate it.
import assert from 'node:assert/strict'
import { execFileSync, spawnSync } from 'node:child_process'
import { mkdtempSync, mkdirSync, writeFileSync, copyFileSync, chmodSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
const dir = mkdtempSync(join(tmpdir(), 'trommi-input-tree-'))
try {
  mkdirSync(join(dir, '.github/scripts'), { recursive: true })
  mkdirSync(join(dir, 'core'))
  copyFileSync(fileURLToPath(new URL('./source_tree.sh', import.meta.url)), join(dir, '.github/scripts/source_tree.sh'))
  const git = (...args) => execFileSync('git', args, { cwd: dir, stdio: 'pipe' }).toString()
  git('init', '-q')
  const commit = () => {
    git('add', '.')
    git('-c', 'commit.gpgsign=false', '-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '-qm', 'Updated: test fixture')
  }
  const tree = () => execFileSync('sh', ['.github/scripts/source_tree.sh', 'HEAD', 'core/'], { cwd: dir }).toString()
  writeFileSync(join(dir, 'core/README.md'), 'First documentation\n')
  writeFileSync(join(dir, 'core/source.rs'), 'fn first() {}\n')
  commit()
  const first = tree()
  assert.ok(first.includes('core/source.rs'))
  assert.ok(first.includes('.github/scripts/source_tree.sh'))
  assert.ok(!first.includes('README.md'))
  writeFileSync(join(dir, 'core/README.md'), 'Different documentation\n')
  commit()
  assert.equal(tree(), first)
  writeFileSync(join(dir, 'core/source.rs'), 'fn second() {}\n')
  commit()
  const source = tree()
  assert.notEqual(source, first)
  chmodSync(join(dir, 'core/source.rs'), 0o755)
  commit()
  const mode = tree()
  assert.notEqual(mode, source)
  writeFileSync(join(dir, '.github/scripts/source_tree.sh'), '# changed helper\n', { flag: 'a' })
  commit()
  assert.notEqual(tree(), mode)
  const invalid = spawnSync('sh', ['.github/scripts/source_tree.sh', 'missing-revision', 'core/'], { cwd: dir, encoding: 'utf8' })
  assert.notEqual(invalid.status, 0)
  assert.equal(invalid.stdout, '')
  console.log('input trees: documentation stable; source, mode and helper invalidate; invalid revision fails closed')
} finally { rmSync(dir, { recursive: true, force: true }) }
