<script setup lang="ts">
import { withBase } from 'vitepress'

const guides = [
  { label: 'Install on macOS', href: withBase('/install/macos') },
  { label: 'Install on Linux', href: withBase('/install/linux') },
  { label: 'Install on Windows', href: withBase('/install/windows-wsl') },
  { label: 'Install VS Code Extension', href: withBase('/install/vscode') }
]
</script>

<template>
  <nav class="aura-hero-platforms" aria-label="Aura installation guides">
    <span class="aura-hero-platforms-label">Choose your setup</span>
    <div class="aura-hero-platform-links">
      <a v-for="guide in guides" :key="guide.href" :href="guide.href">
        <span>{{ guide.label }}</span>
        <span aria-hidden="true">→</span>
      </a>
    </div>
  </nav>
</template>
