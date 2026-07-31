<script setup lang="ts">
declare const __AURORA_RELEASE_VERSION__: string
declare const __AURORA_RELEASE_CHANNEL__: string
declare const __AURORA_IMPLEMENTATION_COMMIT__: string

const version = __AURORA_RELEASE_VERSION__
const channel = __AURORA_RELEASE_CHANNEL__
const implementationCommit = __AURORA_IMPLEMENTATION_COMMIT__
</script>

<template>
  <aside class="aurora-release-stamp" aria-label="Aurora Manual release stamp">
    <strong>Aurora {{ version }} ({{ channel }})</strong>
    <span>
      Implementation baseline commit:
      <code>{{ implementationCommit }}</code>
    </span>
  </aside>
</template>
