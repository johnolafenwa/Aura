// Builds the sidebar for The Aura Programming Language from the files in
// docs/book. Chapter files are named chNN-MM-slug.md (MM = 00 is the chapter
// landing page) and appendix files appendix-MM-slug.md. Each page's title is
// its first `# ` heading, so adding a chapter needs no edit here.
import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

const CHAPTER = /^ch(\d{2})-(\d{2})-[a-z0-9-]+\.md$/
const APPENDIX = /^appendix-(\d{2})-[a-z0-9-]+\.md$/

export function pageTitle(source) {
  const heading = source.split('\n').find((line) => line.startsWith('# '))
  return heading ? heading.slice(2).trim() : null
}

function letter(index) {
  return String.fromCharCode('A'.charCodeAt(0) + index - 1)
}

export function bookSidebar(directory, read = (path) => readFileSync(path, 'utf8')) {
  const files = readdirSync(directory).filter((name) => name.endsWith('.md')).sort()
  const title = (name) => pageTitle(read(join(directory, name))) ?? name
  const link = (name) => `/book/${name.replace(/\.md$/, '')}`

  const chapters = new Map()
  const appendices = []
  for (const name of files) {
    const chapter = CHAPTER.exec(name)
    if (chapter) {
      const number = Number(chapter[1])
      const section = Number(chapter[2])
      if (!chapters.has(number)) chapters.set(number, { landing: null, sections: [] })
      const entry = chapters.get(number)
      if (section === 0) entry.landing = name
      else entry.sections.push({ section, name })
      continue
    }
    const appendix = APPENDIX.exec(name)
    if (appendix) appendices.push({ index: Number(appendix[1]), name })
  }

  const front = [{ text: 'Introduction', link: '/book/' }]
  if (files.includes('foreword.md')) front.push({ text: title('foreword.md'), link: '/book/foreword' })

  const groups = [{ text: 'The Aura Programming Language', items: front }]
  for (const [number, entry] of [...chapters.entries()].sort((a, b) => a[0] - b[0])) {
    const head = entry.landing ?? entry.sections[0]?.name
    const items = entry.sections
      .sort((a, b) => a.section - b.section)
      .map(({ section, name }) => ({ text: `${number}.${section}. ${title(name)}`, link: link(name) }))
    groups.push({
      text: `${number}. ${title(head)}`,
      link: entry.landing ? link(entry.landing) : undefined,
      collapsed: items.length > 0 ? true : undefined,
      items
    })
  }
  if (appendices.length > 0) {
    const sorted = appendices.sort((a, b) => a.index - b.index)
    const index = sorted.find((entry) => entry.index === 0)
    groups.push({
      text: 'Appendices',
      link: index ? link(index.name) : undefined,
      collapsed: true,
      items: sorted
        .filter((entry) => entry.index !== 0)
        .map(({ index: position, name }) => ({ text: `${letter(position)}. ${title(name)}`, link: link(name) }))
    })
  }
  return groups
}

