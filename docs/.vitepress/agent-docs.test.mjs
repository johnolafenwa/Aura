import assert from 'node:assert/strict'
import fs from 'node:fs'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../..', import.meta.url))

test('the homepage gives AI agents prominent machine-readable entry points', () => {
  const landing = fs.readFileSync(`${root}/docs/index.md`, 'utf8')
  const theme = fs.readFileSync(`${root}/docs/.vitepress/theme/index.ts`, 'utf8')
  const component = fs.readFileSync(
    `${root}/docs/.vitepress/theme/AgentDocs.vue`,
    'utf8'
  )

  assert.match(landing, /text: AI Agent Docs\s+link: \/#ai-agents/)
  assert.match(landing, /<AgentDocs \/>/)
  assert.match(theme, /app\.component\('AgentDocs', AgentDocs\)/)
  assert.match(component, /id="ai-agents"/)
  assert.match(component, /withBase\('\/llms\.txt'\)/)
  assert.match(component, /withBase\('\/llms-full\.txt'\)/)
  assert.match(component, /Read .*llms\.txt.* first/s)
  assert.match(component, /Treat the linked Manual as normative/)
})

test('the agent briefing contains long URLs at mobile widths', () => {
  const styles = fs.readFileSync(
    `${root}/docs/.vitepress/theme/style.css`,
    'utf8'
  )

  assert.match(styles, /\.aura-agent-docs-intro,\s*\.aura-agent-docs-instruction\s*{[^}]*min-width: 0/s)
  assert.match(styles, /\.aura-agent-docs-instruction > p\s*{[^}]*overflow-wrap: anywhere/s)
  assert.match(styles, /@media \(max-width: 767px\)[^{]*{[\s\S]*?\.aura-agent-docs\s*{[^}]*grid-template-columns: minmax\(0, 1fr\)/)
})

test('the machine-readable homepage summary remains descriptive', () => {
  const llms = fs.readFileSync(`${root}/docs/public/llms.txt`, 'utf8')

  assert.doesNotMatch(llms, /<AgentDocs \/>/)
  assert.match(llms, /Systems programming should not require a systems background/)
})
