"use strict";

// This module is intentionally lexical. The Rust compiler owns Aura semantics;
// recovery exists only to keep a few editor affordances alive when `aura lsp`
// cannot be started at all.

const { machineCompletions, machineHover, maskAssemblyTemplates } = require("./machine_syntax");

const KEYWORDS = [
  "as",
  "assert",
  "break",
  "case",
  "class",
  "const",
  "continue",
  "def",
  "dyn",
  "elif",
  "else",
  "enum",
  "extern",
  "false",
  "for",
  "from",
  "if",
  "impl",
  "import",
  "in",
  "lambda",
  "match",
  "mut",
  "None",
  "opaque",
  "own",
  "pass",
  "public",
  "return",
  "spawn",
  "trait",
  "true",
  "while",
  "with"
];

const BUILTINS = [
  "Array",
  "Lookup",
  "Poll",
  "Queue",
  "Result",
  "SelectOutcome",
  "TaskGroup",
  "bool",
  "dict",
  "float32",
  "float64",
  "int",
  "int32",
  "int64",
  "list",
  "print",
  "range",
  "select",
  "set",
  "str",
  "yield_now"
];

function indentation(line) {
  const [prefix] = line.match(/^\s*/);
  return prefix.replace(/\t/g, "    ").length;
}

const MAX_SYMBOL_DEPTH = 64;

function declarationForLine(line, lineNumber) {
  const declarationPatterns = [
    // At column zero, mut introduces script storage or a freestanding module cell.
    {
      pattern: /^((?:public\s+)?mut\s+)([A-Za-z_][A-Za-z0-9_]*)(?=\s*[:=])/,
      kind: "variable",
      detail: "mut"
    },
    {
      pattern:
        /^(\s*(?:public\s+)?extern\s+"C"\s+opaque\s+class\s+)([A-Za-z_][A-Za-z0-9_]*)/,
      kind: "class",
      detail: 'extern "C" opaque class'
    },
    {
      pattern:
        /^(\s*(?:public\s+)?extern\s+"C"\s+(?:(?:packed|align\s*\(\s*[0-9][\w]*\s*\))\s+){0,2}class\s+)([A-Za-z_][A-Za-z0-9_]*)/,
      kind: "class",
      detail: 'extern "C" class'
    },
    {
      pattern:
        /^(\s*(?:public\s+)?extern\s+"C"\s+union\s+)([A-Za-z_][A-Za-z0-9_]*)/,
      kind: "class",
      detail: 'extern "C" union'
    },
    {
      pattern:
        /^(\s*(?:public\s+)?extern\s+"C"\s+def\s+)([A-Za-z_][A-Za-z0-9_]*)/,
      kind: "function",
      detail: 'extern "C" function'
    }
  ];
  for (const { pattern, kind, detail } of declarationPatterns) {
    const match = line.match(pattern);
    if (match) {
      const [, declarationPrefix, name] = match;
      const startCharacter = declarationPrefix.length;
      return {
        indent: indentation(line),
        symbol: {
          name,
          kind,
          detail,
          line: lineNumber,
          startCharacter,
          endCharacter: startCharacter + name.length,
          children: []
        }
      };
    }
  }

  const match = line.match(
    /^\s*(?:(?:public|copy)\s+)*(?:export\s+"C"\s+)?(class|enum|trait|def|const)\s+([A-Za-z_][A-Za-z0-9_]*)/
  );
  if (!match) {
    return null;
  }
  const [, declaration, name] = match;
  const startCharacter = match[0].length - name.length;
  const kind = ({ def: "function", const: "constant" })[declaration] || declaration;
  return {
    indent: indentation(line),
    symbol: {
      name,
      kind,
      detail: kind,
      line: lineNumber,
      startCharacter,
      endCharacter: startCharacter + name.length,
      children: []
    }
  };
}

function recoverSymbols(text) {
  const moduleStorage = process.env.AURA_LSP_FREESTANDING === "1" ||
    /^(?:public[ \t]+)?(?:export[ \t]+"C"[ \t]+def\b|def[ \t]+main\b)/m.test(text);
  const roots = [];
  const containers = [];
  for (const [lineNumber, line] of maskAssemblyTemplates(text).split(/\r?\n/).entries()) {
    const entry = declarationForLine(line, lineNumber);
    if (
      !entry ||
      (entry.symbol.kind === "variable" && !moduleStorage && !line.startsWith("public"))
    ) {
      continue;
    }
    const { indent, symbol } = entry;
    // Navigation reads the indent; the published symbol leaves it out.
    Object.defineProperty(symbol, "indent", { value: indent, enumerable: false });
    while (
      containers.length > 0 &&
      indent <= containers[containers.length - 1].indent
    ) {
      containers.pop();
    }
    const parent = containers[containers.length - 1];
    if (parent) {
      if (symbol.kind === "function") {
        symbol.kind = "method";
      }
      parent.symbol.children.push(symbol);
    } else {
      roots.push(symbol);
    }
    if (
      ["class", "enum", "trait"].includes(symbol.kind) &&
      containers.length < MAX_SYMBOL_DEPTH
    ) {
      containers.push({ indent, symbol });
    }
  }
  return roots;
}

function documentSymbols(text) {
  return recoverSymbols(text);
}

function analyzeDocument(text) {
  return {
    diagnostics: [],
    symbols: documentSymbols(text)
  };
}

function diagnosticsForDocument(_text) {
  return [];
}

function flattenSymbols(symbols) {
  const flattened = [];
  const stack = [];
  for (let index = symbols.length - 1; index >= 0; index -= 1) {
    stack.push(symbols[index]);
  }
  while (stack.length > 0) {
    const symbol = stack.pop();
    flattened.push(symbol);
    for (let index = symbol.children.length - 1; index >= 0; index -= 1) {
      stack.push(symbol.children[index]);
    }
  }
  return flattened;
}

function linePrefix(text, line, character) {
  const lines = text.split(/\r?\n/);
  if (line < 0 || line >= lines.length) {
    return "";
  }
  return lines[line].slice(0, Math.max(0, character));
}

function completionsForDocument(text, line, character, triggerCharacter) {
  const machine = machineCompletions(text, line, character);
  if (machine) return machine;
  if (triggerCharacter === "." || /\.\s*$/.test(linePrefix(text, line, character))) {
    return [];
  }
  const items = [
    ...KEYWORDS.map((name) => ({ name, kind: "keyword", detail: "keyword" })),
    ...BUILTINS.map((name) => ({ name, kind: "function", detail: "builtin" })),
    ...["fixed", "ptr"].map((name) => ({ name, kind: "type", detail: "builtin type" })),
    ...flattenSymbols(documentSymbols(text)).map((symbol) => ({
      name: symbol.name,
      kind: symbol.kind,
      detail: symbol.detail.startsWith("extern ")
        ? symbol.detail
        : `recovered ${symbol.kind}`
    }))
  ];
  const seen = new Set();
  return items.filter((item) => {
    if (seen.has(item.name)) {
      return false;
    }
    seen.add(item.name);
    return true;
  });
}

function wordAtPosition(text, line, character) {
  const lines = maskAssemblyTemplates(text).split(/\r?\n/);
  if (line < 0 || line >= lines.length) {
    return null;
  }
  const current = lines[line];
  const identifier = /[A-Za-z_][A-Za-z0-9_]*/g;
  for (const match of current.matchAll(identifier)) {
    const start = match.index;
    const end = start + match[0].length;
    if (character >= start && character < end) {
      return { name: match[0], line, start, end };
    }
  }
  return null;
}

function recoveredDeclaration(text, name, inactiveSuite) {
  const matches = flattenSymbols(recoverSymbols(text)).filter((symbol) => symbol.name === name);
  if (!inactiveSuite) {
    return matches[0] || null;
  }
  // Two targets can define the same function. Prefer the queried suite.
  // Outside it, only an unindented declaration is unconditionally visible.
  const local = matches.filter(({ line }) => inactiveSuite.start <= line && line < inactiveSuite.end);
  const candidates = local.length ? local : matches.filter(({ indent }) => indent === 0);
  return candidates.length === 1 ? candidates[0] : null;
}

function definitionForPosition(text, line, character, inactiveSuite) {
  const word = wordAtPosition(text, line, character);
  if (!word) {
    return null;
  }
  const declaration = recoveredDeclaration(text, word.name, inactiveSuite);
  if (!declaration) {
    return null;
  }
  return {
    line: declaration.line,
    startCharacter: declaration.startCharacter,
    endCharacter: declaration.endCharacter
  };
}

function hoverForPosition(text, line, character, inactiveSuite) {
  const machine = machineHover(text, line, character);
  if (machine) return machine;
  const word = wordAtPosition(text, line, character);
  if (!word) {
    return null;
  }
  const declaration = recoveredDeclaration(text, word.name, inactiveSuite);
  if (!declaration) {
    return null;
  }
  return {
    value: `${declaration.detail} ${declaration.name}`,
    range: {
      start: { line: word.line, character: word.start },
      end: { line: word.line, character: word.end }
    }
  };
}

module.exports = {
  analyzeDocument,
  completionsForDocument,
  definitionForPosition,
  diagnosticsForDocument,
  documentSymbols,
  hoverForPosition
};
