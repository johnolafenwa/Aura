import assert from 'node:assert/strict'
import test from 'node:test'

import { createHighlighter } from 'shiki'

import { AURA_LANGUAGE, AURA_LANGUAGE_LABEL } from './aura-language.mjs'

test('Aura uses the canonical source.aura TextMate grammar', () => {
  assert.equal(AURA_LANGUAGE.name, 'aura')
  assert.equal(AURA_LANGUAGE.scopeName, 'source.aura')
  assert.equal(AURA_LANGUAGE_LABEL, 'Aura')
  assert.ok(AURA_LANGUAGE.patterns.length > 0)
  assert.ok(AURA_LANGUAGE.repository.keywords)
})

test('Shiki tokenizes Aura source with the custom grammar', async () => {
  const highlighter = await createHighlighter({
    themes: ['github-dark'],
    langs: [AURA_LANGUAGE]
  })
  const html = highlighter.codeToHtml(
    'def main() -> int32:\n    print("hello from aura")\n',
    { lang: 'aura', theme: 'github-dark' }
  )

  assert.equal(highlighter.getLoadedLanguages().includes('aura'), true)
  assert.match(html, /<span style="color:/)
  assert.match(html, /main/)
  assert.match(html, /hello from aura/)
})
