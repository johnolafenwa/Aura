import assert from 'node:assert/strict'
import fs from 'node:fs'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../..', import.meta.url))

test('the landing page leads with Aura\'s compiled and statically typed identity', () => {
  const landing = fs.readFileSync(`${root}/docs/index.md`, 'utf8')

  for (const statement of [
    'Compiled. Statically typed. Familiar.',
    'Python-inspired syntax, deterministic ownership, structured concurrency, and native executables for reliable software.',
    'Native Compilation',
    'Static Types',
    'Ownership-Based Reliability',
    'Aura brings familiar source code to a compiled, statically typed language.'
  ]) {
    assert.match(landing, new RegExp(statement.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')))
  }

  assert.doesNotMatch(landing, /Simple, safe systems programming\./)
  assert.doesNotMatch(landing, /Aura is (?:a|the)[^.]*systems(?: programming)? language/i)
})

test('the site metadata carries the same positioning', () => {
  const config = fs.readFileSync(`${root}/docs/.vitepress/config.mts`, 'utf8')

  assert.match(config, /compiled, statically typed programming language/)
  assert.match(config, /Python-inspired syntax/)
  assert.match(config, /native executables for reliable software/)
  assert.doesNotMatch(config, /compiled systems language/)
})

test('the systems-language ambition is explicitly long-term', () => {
  const landing = fs.readFileSync(`${root}/docs/index.md`, 'utf8')
  const positioning = fs.readFileSync(`${root}/docs/positioning.md`, 'utf8')
  const combined = `${landing}\n${positioning}`

  assert.match(
    combined,
    /long-term goal[^.]*general-purpose systems language[^.]*every type of software/is
  )
  assert.match(combined, /operating systems/i)
  assert.match(combined, /device drivers/i)
})
