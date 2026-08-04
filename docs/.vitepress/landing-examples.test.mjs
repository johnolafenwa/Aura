import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'

const root = path.resolve(import.meta.dirname, '../..')

// The Manual's code blocks are executed by scripts/reference_integrity.py.
// The landing page is outside that gate, yet its sample is the first Aura
// code most people ever read, so it gets executed here.
test('every Aura example on the landing page runs', () => {
  const page = fs.readFileSync(`${root}/docs/index.md`, 'utf8')
  const blocks = [...page.matchAll(/^```aura\n([\s\S]*?)^```$/gm)].map((match) => match[1])

  assert.ok(blocks.length > 0, 'docs/index.md declares no ```aura example')

  // scripts/reference_integrity.py runs earlier in the gate and leaves a
  // debug binary behind; a local release build is preferred when present.
  const candidates = [
    process.env.AURA_BIN,
    `${root}/target/release/aura`,
    `${root}/target/debug/aura`,
  ].filter(Boolean)
  const binary = candidates.find((candidate) => fs.existsSync(candidate))
  if (!binary) {
    assert.fail(
      `Aura binary not found. Looked in ${candidates.join(', ')}. ` +
        'Build it with `cargo build -p aura` or set AURA_BIN.'
    )
  }

  const workdir = fs.mkdtempSync(path.join(os.tmpdir(), 'aura-landing-'))
  try {
    blocks.forEach((source, index) => {
      const file = path.join(workdir, `landing_${index}.au`)
      fs.writeFileSync(file, source)
      execFileSync(binary, ['run', file], { stdio: 'pipe' })
    })
  } finally {
    fs.rmSync(workdir, { recursive: true, force: true })
  }
})
