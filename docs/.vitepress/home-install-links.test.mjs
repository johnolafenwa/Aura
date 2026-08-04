import assert from 'node:assert/strict'
import fs from 'node:fs'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../..', import.meta.url))

test('the hero links directly to every installation guide', () => {
  const theme = fs.readFileSync(`${root}/docs/.vitepress/theme/index.ts`, 'utf8')
  const component = fs.readFileSync(
    `${root}/docs/.vitepress/theme/HomePlatformLinks.vue`,
    'utf8'
  )

  assert.match(theme, /'home-hero-actions-after': \(\) => h\(HomePlatformLinks\)/)
  assert.match(component, /aria-label="Aura installation guides"/)

  for (const [label, route] of [
    ['Install on macOS', '/install/macos'],
    ['Install on Linux', '/install/linux'],
    ['Install on Windows', '/install/windows-wsl'],
    ['Install VS Code Extension', '/install/vscode']
  ]) {
    assert.match(component, new RegExp(`label: '${label}'`))
    assert.match(component, new RegExp(`withBase\\('${route}'\\)`))
  }
})

test('the installation links retain usable mobile columns', () => {
  const styles = fs.readFileSync(
    `${root}/docs/.vitepress/theme/style.css`,
    'utf8'
  )

  assert.match(styles, /\.aura-hero-platform-links\s*{[^}]*grid-template-columns: repeat\(4, minmax\(0, 1fr\)\)/s)
  assert.match(styles, /@media \(max-width: 767px\)[^{]*{[\s\S]*?\.aura-hero-platform-links\s*{[^}]*grid-template-columns: repeat\(2, minmax\(0, 1fr\)\)/)
  assert.match(styles, /@media \(max-width: 419px\)[^{]*{[\s\S]*?\.aura-hero-platform-links\s*{[^}]*grid-template-columns: minmax\(0, 1fr\)/)
})

test('the Learn installation chapter points to every guide and teaches VS Code setup', () => {
  const learn = fs.readFileSync(
    `${root}/docs/learn/install-and-run.md`,
    'utf8'
  )

  assert.match(learn, /## Choose Your Platform Guide/)
  for (const route of [
    '/install/macos',
    '/install/linux',
    '/install/windows-wsl',
    '/install/vscode'
  ]) {
    assert.match(learn, new RegExp(`\\]\\(${route.replace('/', '\\/')}\\)`))
  }

  assert.match(learn, /## Install The VS Code Extension/)
  assert.match(learn, /code --install-extension JohnOlafenwa\.vscode-aura-lang/)
  assert.match(learn, /aura lsp/)
  assert.match(learn, /Install in WSL: Ubuntu/)
})
