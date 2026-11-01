import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { bookSidebar, pageTitle } from './book-sidebar.mjs'

function book(files) {
  const directory = mkdtempSync(join(tmpdir(), 'aura-book-'))
  for (const [name, source] of Object.entries(files)) writeFileSync(join(directory, name), source)
  return directory
}

test('pageTitle reads the first level-one heading', () => {
  assert.equal(pageTitle('intro\n# Hello, world\n\n## Later'), 'Hello, world')
  assert.equal(pageTitle('no heading'), null)
})

test('chapters are numbered and ordered from file names', () => {
  const directory = book({
    'index.md': '# The Aura Programming Language\n',
    'foreword.md': '# Foreword\n',
    'ch10-00-generics.md': '# Generics\n',
    'ch02-00-word-counter.md': '# Programming a word counter\n',
    'ch01-02-hello-world.md': '# Hello, world\n',
    'ch01-00-getting-started.md': '# Getting started\n',
    'ch01-01-installation.md': '# Installation\n',
    'appendix-00-appendices.md': '# Appendices\n',
    'appendix-02-operators.md': '# Operators\n',
    'appendix-01-keywords.md': '# Keywords\n',
    'STYLE.md': '# Writing the Aura book\n'
  })
  const sidebar = bookSidebar(directory)
  assert.deepEqual(sidebar[0].items, [
    { text: 'Introduction', link: '/book/' },
    { text: 'Foreword', link: '/book/foreword' }
  ])
  assert.equal(sidebar[1].text, '1. Getting started')
  assert.equal(sidebar[1].link, '/book/ch01-00-getting-started')
  assert.deepEqual(
    sidebar[1].items.map((item) => item.text),
    ['1.1. Installation', '1.2. Hello, world']
  )
  assert.equal(sidebar[2].text, '2. Programming a word counter')
  assert.deepEqual(sidebar[2].items, [])
  assert.equal(sidebar[3].text, '10. Generics')
  assert.equal(sidebar[4].text, 'Appendices')
  assert.equal(sidebar[4].link, '/book/appendix-00-appendices')
  assert.deepEqual(
    sidebar[4].items.map((item) => item.text),
    ['A. Keywords', 'B. Operators']
  )
  assert.equal(sidebar.length, 5)
})
