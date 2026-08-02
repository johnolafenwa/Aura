<script setup lang="ts">
import { ref } from 'vue'

const command = 'curl -fsSL https://johnolafenwa.github.io/Aura/install.sh | sh'
const copied = ref(false)

async function copyCommand() {
  await navigator.clipboard.writeText(command)
  copied.value = true
  window.setTimeout(() => {
    copied.value = false
  }, 1600)
}
</script>

<template>
  <div class="aura-hero-install" aria-label="Install Aura">
    <span class="aura-hero-install-label">Install Aura</span>
    <code>{{ command }}</code>
    <button type="button" :aria-label="copied ? 'Aura install command copied' : 'Copy Aura install command'" @click="copyCommand">
      {{ copied ? 'Copied' : 'Copy' }}
    </button>
  </div>
</template>
