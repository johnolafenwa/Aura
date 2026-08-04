<script setup lang="ts">
import { ref } from 'vue'
import { withBase } from 'vitepress'

const indexHref = withBase('/llms.txt')
const fullHref = withBase('/llms-full.txt')
const instruction =
  'Read https://johnolafenwa.github.io/Aura/llms.txt first. Use https://johnolafenwa.github.io/Aura/llms-full.txt when you need complete language and tooling context. Treat the linked Manual as normative.'
const copied = ref(false)

async function copyInstruction() {
  await navigator.clipboard.writeText(instruction)
  copied.value = true
  window.setTimeout(() => {
    copied.value = false
  }, 1600)
}
</script>

<template>
  <section id="ai-agents" class="aura-agent-docs" aria-labelledby="aura-agent-docs-title">
    <div class="aura-agent-docs-intro">
      <span class="aura-agent-docs-eyebrow">AI agent entry point</span>
      <h2 id="aura-agent-docs-title">Teach an agent Aura in one URL.</h2>
      <p>
        Aura publishes machine-readable documentation generated from the
        maintained Manual, Learn track, tutorials, installation guides, and
        project overview.
      </p>

      <nav class="aura-agent-docs-links" aria-label="Aura documentation for AI agents">
        <a :href="indexHref">
          <span>
            <strong>llms.txt</strong>
            <small>Start here for the map, summaries, and canonical links.</small>
          </span>
          <span aria-hidden="true">↗</span>
        </a>
        <a :href="fullHref">
          <span>
            <strong>llms-full.txt</strong>
            <small>Load the complete maintained language and tooling context.</small>
          </span>
          <span aria-hidden="true">↗</span>
        </a>
      </nav>
    </div>

    <div class="aura-agent-docs-instruction">
      <div class="aura-agent-docs-instruction-head">
        <span>Agent instruction</span>
        <button
          type="button"
          :aria-label="copied ? 'Aura agent instruction copied' : 'Copy Aura agent instruction'"
          aria-live="polite"
          @click="copyInstruction"
        >
          {{ copied ? 'Copied' : 'Copy' }}
        </button>
      </div>
      <p>{{ instruction }}</p>
    </div>
  </section>
</template>
