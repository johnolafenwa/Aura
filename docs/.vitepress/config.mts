import { defineConfig } from 'vitepress'

const base = process.env.VITEPRESS_BASE ?? '/'

export default defineConfig({
  title: 'Aurora',
  description: 'The guide and reference manual for the Aurora programming language.',
  lang: 'en-US',
  base,
  head: [['link', { rel: 'icon', type: 'image/svg+xml', href: `${base}aurora-mark.svg` }]],
  cleanUrls: true,
  srcExclude: ['aurora_language_proposal.md', 'ml_systems_support_plan.md', 'testing_strategy.md'],
  lastUpdated: true,
  markdown: {
    lineNumbers: true
  },
  themeConfig: {
    logo: '/aurora-mark.svg',
    siteTitle: 'Aurora',
    search: {
      provider: 'local'
    },
    nav: [
      { text: 'Learn', link: '/learn/' },
      { text: 'Manual', link: '/manual/' },
      { text: 'Examples', link: '/learn/case-studies/log-analyzer' },
      { text: 'GitHub', link: 'https://github.com/johnolafenwa/Aurora' }
    ],
    sidebar: {
      '/learn/': [
        {
          text: 'Learn Aurora',
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
            { text: 'Native Builds', link: '/learn/native-builds' }
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
            { text: 'Functions', link: '/manual/functions' },
            { text: 'Classes', link: '/manual/classes' },
            { text: 'Enums And Pattern Matching', link: '/manual/enums-and-match' },
            { text: 'Generics And Traits', link: '/manual/generics-and-traits' },
            { text: 'Ownership And Borrowing', link: '/manual/ownership-and-borrowing' },
            { text: 'Execution Model', link: '/manual/execution-model' },
            { text: 'Collections', link: '/manual/collections' },
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
          text: 'Aurora Book',
          items: [
            { text: 'Home', link: '/' },
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
      { icon: 'github', link: 'https://github.com/johnolafenwa/Aurora' }
    ],
    footer: {
      message: 'Generated from the current Aurora repository surface.',
      copyright: 'Aurora documentation.'
    }
  }
})
