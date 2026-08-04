import fs from 'node:fs'
import { fileURLToPath } from 'node:url'

const grammarUrl = new URL(
  '../../tools/vscode-aura/syntaxes/aura.tmLanguage.json',
  import.meta.url
)
const grammar = JSON.parse(
  fs.readFileSync(fileURLToPath(grammarUrl), 'utf8')
)

export const AURA_LANGUAGE_LABEL = 'Aura'

export const AURA_LANGUAGE = Object.freeze({
  ...grammar,
  name: 'aura',
  displayName: AURA_LANGUAGE_LABEL
})
