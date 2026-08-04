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

test('Shiki colors Aura f-string interpolation separately from string text', async () => {
  const highlighter = await createHighlighter({
    themes: ['dark-plus'],
    langs: [AURA_LANGUAGE]
  })
  const [line] = highlighter.codeToTokens('print(f"Lang: {lang}")', {
    lang: 'aura',
    theme: 'dark-plus'
  }).tokens
  const stringText = line.find((token) => token.content.includes('Lang: '))
  const openingBrace = line.find((token) => token.content === '{')
  const expression = line.find((token) => token.content === 'lang')
  const closingBrace = line.find((token) => token.content === '}')

  assert.ok(stringText, 'the f-string text should be tokenized')
  assert.ok(openingBrace, 'the opening interpolation brace should be its own token')
  assert.ok(expression, 'the interpolation expression should be its own token')
  assert.ok(closingBrace, 'the closing interpolation brace should be its own token')
  assert.notEqual(openingBrace.color, stringText.color)
  assert.notEqual(expression.color, stringText.color)
  assert.notEqual(closingBrace.color, stringText.color)
})
