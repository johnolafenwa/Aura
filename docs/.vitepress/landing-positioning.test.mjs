import assert from 'node:assert/strict'
import fs from 'node:fs'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../..', import.meta.url))

test('the landing page states Aura\'s three familiar foundations and purpose', () => {
  const landing = fs.readFileSync(`${root}/docs/index.md`, 'utf8')

  for (const statement of [
    'Simple, safe systems programming.',
    'A compiled systems language for agents and frontier ML systems',
    'Python-like syntax',
    'Rust-like ownership',
    'Go-like task-based concurrency',
    'simple and safe systems language'
  ]) {
    assert.match(landing, new RegExp(statement.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')))
  }
})

test('the site metadata carries the same positioning', () => {
  const config = fs.readFileSync(`${root}/docs/.vitepress/config.mts`, 'utf8')

  assert.match(config, /compiled systems language with Python-like syntax/)
  assert.match(config, /Rust-like ownership/)
  assert.match(config, /Go-like tasks for agents and frontier ML systems/)
})
