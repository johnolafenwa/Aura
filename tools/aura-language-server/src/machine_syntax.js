"use strict";

// The compiler resolves Aura expressions inside input and output operands.
// The fixed syntax surrounding those expressions has no Aura declaration.
const DIRECTIVES = {
  asm: "Emit inline assembly for the freestanding target.",
  module_asm: "Emit a module-level assembly block in the freestanding object."
};
const ATTRIBUTES = {
  interrupt: "Enter and return according to the target interrupt convention.",
  naked: "Let the assembly body supply the function entry and return.",
  section: "Place the definition in the named object section.",
  link_name: "Give the definition this linker symbol.",
  align: "Align the definition to this power of two in bytes.",
  noinline: "Keep this function out of its callers.",
  always_inline: "Inline this function at its calls.",
  used: "Retain the symbol when collecting unused sections."
};
const CLAUSES = {
  input: "Pass the value of an Aura scalar as an input operand.",
  output: "Write an output operand to a mutable Aura scalar.",
  clobber: "Declare additional registers or memory changed by the assembly.",
  options: "Describe assembly effects with pure, preserves_flags or noreturn."
};

// Consume complete strings and comments in one token, including an open
// triple string while the author is typing. Offsets stay in LSP UTF-16 units.
const TRIPLE = /(?<triple>"""|''')(?:(?!\k<triple>)[^\\]|\\[\s\S]?)*(?<closedTriple>\k<triple>)?/;
const SINGLE = /(?<quote>["'])(?:(?!\k<quote>)[^\\\r\n]|\\[^\r\n]?)*(?<closedSingle>\k<quote>)?/;
const TOKENS = new RegExp([
  /#[^\n]*/.source,
  `(?:r|b|f)?(?:${TRIPLE.source}|${SINGLE.source})`,
  /^[\t ]*(?:asm|module_asm)[\t ]*\(/.source,
  /^[\t ]*@machine\.\w*/.source,
  /\w+|[^\s]/.source
].join("|"), "gm");

function machineSpans(source) {
  const spans = [];
  if (!source.includes("asm") && !source.includes("@machine.")) return spans;
  const groups = [];
  let previous = "";
  for (const match of source.matchAll(TOKENS)) {
    const { 0: raw, index } = match;
    const token = raw.trimStart();
    const end = index + raw.length;
    const start = end - token.length;
    const group = groups.at(-1);
    if (token.startsWith("#")) continue;
    if (token.startsWith("@machine.")) {
      const name = token.slice("@machine.".length);
      spans.push({
        start: start + "@machine.".length,
        end,
        choices: ATTRIBUTES,
        detail: Object.hasOwn(ATTRIBUTES, name) && ATTRIBUTES[name]
      });
    } else if (/^(?:asm|module_asm)[\t ]*\($/.test(token)) {
      const name = token.split(/[\t (]/, 1)[0];
      groups.push(name);
      spans.push({ start, end: start + name.length, detail: DIRECTIVES[name] });
    } else if (token === "(" || token === "[" || token === "{") {
      groups.push(null);
    } else if (token === ")" || token === "]" || token === "}") {
      groups.pop();
    } else if ((group === "asm" || group === "module_asm") &&
               previous.endsWith("(") && /^(?:r?["'])/.test(token)) {
      const open = !match.groups.closedSingle && !match.groups.closedTriple;
      spans.push({ start, end, template: true, open });
    } else if (group === "asm" && token === ",") {
      const next = /^\s*\w*/.exec(source.slice(end))[0];
      spans.push({ start: end, end: end + next.length, choices: CLAUSES });
    } else if (group === "asm" && previous === "," && Object.hasOwn(CLAUSES, token)) {
      spans.push({ start, end, detail: CLAUSES[token] });
    }
    previous = token;
  }
  return spans;
}

function atPosition(source, line, character) {
  const lines = source.split("\n");
  if (line < 0 || line >= lines.length) return [];
  const lineStart = lines.slice(0, line).reduce((start, text) => start + text.length + 1, 0);
  const offset = lineStart + character;
  return machineSpans(source)
    .filter(span => offset >= span.start &&
      (offset < span.end || (offset === span.end && (span.open || span.choices))))
    .map(span => ({ ...span, start: span.start - lineStart, end: span.end - lineStart }));
}

function machineCompletions(source, line, character) {
  for (const span of atPosition(source, line, character)) {
    if (span.template) return [];
    if (span.choices) {
      return Object.entries(span.choices).map(([name, detail]) => ({ name, kind: "keyword", detail }));
    }
  }
  return null;
}

function machineHover(source, line, character) {
  const span = atPosition(source, line, character).find(item => item.detail && character < item.end);
  if (!span) return null;
  return {
    value: span.detail,
    range: {
      start: { line, character: span.start },
      end: { line, character: span.end }
    }
  };
}

// Prevent the compiler-unavailable recovery and workspace indexers from
// treating lines inside a template as Aura declarations or references.
function maskAssemblyTemplates(source) {
  let result = "";
  let copied = 0;
  for (const span of machineSpans(source)) {
    if (!span.template) continue;
    result += source.slice(copied, span.start) +
      source.slice(span.start, span.end).replace(/[^\r\n]/g, " ");
    copied = span.end;
  }
  return result + source.slice(copied);
}

module.exports = { machineCompletions, machineHover, maskAssemblyTemplates };
