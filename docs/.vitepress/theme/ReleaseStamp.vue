<script setup lang="ts">
declare const __AURA_RELEASE_VERSION__: string
declare const __AURA_RELEASE_CHANNEL__: string
declare const __AURA_IMPLEMENTATION_COMMIT__: string

const version = __AURA_RELEASE_VERSION__
const channel = __AURA_RELEASE_CHANNEL__
const implementationCommit = __AURA_IMPLEMENTATION_COMMIT__
</script>

<template>
  <aside class="aura-release-stamp" aria-label="Aura Manual release stamp">
    <strong>Aura {{ version }} ({{ channel }})</strong>
    <span>
      Implementation baseline commit:
      <code>{{ implementationCommit }}</code>
    </span>
  </aside>
</template>
