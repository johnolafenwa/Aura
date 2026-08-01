import { defineConfig } from 'vitepress'
import { AURA_RELEASE, resolveImplementationCommit } from './release-metadata.mjs'

const base = process.env.VITEPRESS_BASE ?? '/'
const implementationCommit = resolveImplementationCommit()
const proposalStem = 'auro' + 'ra_language_proposal.md'

export default defineConfig({
  title: 'Aura',
  description: 'The guide and reference manual for the Aura programming language.',
  lang: 'en-US',
  base,
  vite: {
    define: {
      __AURA_RELEASE_VERSION__: JSON.stringify(AURA_RELEASE.version),
      __AURA_RELEASE_CHANNEL__: JSON.stringify(AURA_RELEASE.channel),
      __AURA_IMPLEMENTATION_COMMIT__: JSON.stringify(implementationCommit)
    }
  },
  head: [['link', { rel: 'icon', type: 'image/svg+xml', href: `${base}aura-mark.svg` }]],
  cleanUrls: true,
  srcExclude: [proposalStem, 'ml_systems_support_plan.md', 'testing_strategy.md'],
  lastUpdated: true,
  markdown: {
    lineNumbers: true
  },
  themeConfig: {
    logo: '/aura-mark.svg',
    siteTitle: 'Aura',
    search: {
      provider: 'local'
    },
    nav: [
      { text: 'Learn', link: '/learn/' },
      { text: 'Manual', link: '/manual/' },
      { text: 'Downloads', link: '/downloads' },
      { text: 'Why Aura', link: '/positioning' },
      { text: 'Examples', link: '/learn/case-studies/log-analyzer' },
      { text: 'GitHub', link: 'https://github.com/johnolafenwa/Aura' }
    ],
    sidebar: {
      '/learn/': [
        {
          text: 'Learn Aura',
          items: [
            { text: 'Overview', link: '/learn/' },
            { text: 'Install And Run', link: '/learn/install-and-run' },
            { text: 'Small Programs', link: '/learn/small-programs' },
            { text: 'Data Modeling', link: '/learn/data-modeling' },
            { text: 'Collections', link: '/learn/collections' },
            { text: 'Ownership And Borrowing', link: '/learn/ownership-and-borrowing' },
            { text: 'Errors With Result And Option', link: '/learn/results-and-options' },
            { text: 'Modules And Packages', link: '/learn/modules-and-packages' },
            { text: 'Concurrency', link: '/learn/concurrency' },
            { text: 'Files, Processes, Networking', link: '/learn/io-process-networking' },
            { text: 'Native Builds', link: '/learn/native-builds' },
            { text: 'Calling C With FFI', link: '/learn/ffi' }
          ]
        },
        {
          text: 'Use Case Lessons',
          items: [
            { text: 'Log Analyzer', link: '/learn/case-studies/log-analyzer' },
            { text: 'Queue Worker Pool', link: '/learn/case-studies/queue-worker-pool' },
            { text: 'Supervised Process Runner', link: '/learn/case-studies/process-supervisor' }
          ]
        }
      ],
      '/manual/': [
        {
          text: 'Manual',
          items: [
            { text: 'Overview', link: '/manual/' },
            { text: 'Language Specification', link: '/manual/language-specification' },
            { text: 'Status And Compatibility', link: '/manual/status-and-compatibility' },
            { text: 'Lexical Structure', link: '/manual/lexical-structure' },
            { text: 'Complete Grammar', link: '/manual/grammar' },
            { text: 'Names And Scopes', link: '/manual/names-and-scopes' },
            { text: 'Types', link: '/manual/types' },
            { text: 'Static Semantics', link: '/manual/static-semantics' },
            { text: 'Expressions', link: '/manual/expressions' },
            { text: 'Statements', link: '/manual/statements' },
            { text: 'Tuples', link: '/manual/tuples' },
            { text: 'Assertions', link: '/manual/assertions' },
            { text: 'Functions', link: '/manual/functions' },
            { text: 'Closures', link: '/manual/closures' },
            { text: 'Foreign Function Interface', link: '/manual/ffi' },
            { text: 'Classes', link: '/manual/classes' },
            { text: 'Enums And Pattern Matching', link: '/manual/enums-and-match' },
            { text: 'Generics And Traits', link: '/manual/generics-and-traits' },
            { text: 'Ownership And Borrowing', link: '/manual/ownership-and-borrowing' },
            { text: 'Execution Model', link: '/manual/execution-model' },
            { text: 'Collections', link: '/manual/collections' },
            { text: 'Numeric Arrays', link: '/manual/numeric-arrays' },
            { text: 'Bytes, Codecs, And SHA-256', link: '/manual/bytes' },
            { text: 'JSON Module', link: '/manual/json' },
            { text: 'Randomness Module', link: '/manual/randomness' },
            { text: 'Concurrency', link: '/manual/concurrency' },
            { text: 'I/O Module', link: '/manual/io' },
            { text: 'Filesystem Module', link: '/manual/filesystem' },
            { text: 'Network Module', link: '/manual/network' },
            { text: 'Process Module', link: '/manual/process' },
            { text: 'Control-Plane Modules', link: '/manual/control-plane' },
            { text: 'Packages', link: '/manual/packages' },
            { text: 'CLI And Tooling', link: '/manual/cli-and-tooling' },
            { text: 'API Index', link: '/manual/api-index' },
            { text: 'Diagnostics', link: '/manual/diagnostics' },
            { text: 'Current Limits', link: '/manual/current-limits' },
            { text: 'Conformance', link: '/manual/conformance' }
          ]
        }
      ],
      '/': [
        {
          text: 'Aura Book',
          items: [
            { text: 'Home', link: '/' },
            { text: 'Why Aura', link: '/positioning' },
            { text: 'Downloads', link: '/downloads' },
            { text: 'Release Process', link: '/release-process' },
            { text: 'Learn', link: '/learn/' },
            { text: 'Manual', link: '/manual/' }
          ]
        }
      ]
    },
    outline: {
      level: [2, 3]
    },
    socialLinks: [
      { icon: 'github', link: 'https://github.com/johnolafenwa/Aura' }
    ],
    footer: {
      message: `Aura ${AURA_RELEASE.version} ${AURA_RELEASE.channel}. Implementation baseline: ${implementationCommit}.`,
      copyright: 'Aura documentation.'
    }
  }
})
