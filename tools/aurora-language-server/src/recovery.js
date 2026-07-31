"use strict";

// This module is intentionally lexical. The Rust compiler owns Aurora semantics;
// recovery exists only to keep a few editor affordances alive when `aura lsp`
// cannot be started at all.

const KEYWORDS = [
  "assert",
  "break",
  "case",
  "class",
  "continue",
  "def",
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
  "Map",
  "Option",
  "Queue",
  "Result",
  "SelectOutcome",
  "Set",
  "String",
  "TaskGroup",
  "Vec",
  "bool",
  "float32",
  "float64",
  "int",
  "int32",
  "int64",
  "print",
  "range",
  "select",
  "yield_now"
];

function indentation(line) {
  const [prefix] = line.match(/^\s*/);
  return prefix.replace(/\t/g, "    ").length;
}

function declarationForLine(line, lineNumber) {
  const ffiPatterns = [
    {
      pattern:
        /^(\s*)((?:public\s+)?extern\s+"C"\s+opaque\s+class\s+)([A-Za-z_][A-Za-z0-9_]*)/,
      kind: "class",
      detail: 'extern "C" opaque class'
    },
    {
      pattern:
        /^(\s*)((?:public\s+)?extern\s+"C"\s+def\s+)([A-Za-z_][A-Za-z0-9_]*)/,
      kind: "function",
      detail: 'extern "C" function'
    }
  ];
  for (const { pattern, kind, detail } of ffiPatterns) {
    const ffiMatch = line.match(pattern);
    if (ffiMatch) {
      const [, indentation, declarationPrefix, name] = ffiMatch;
      const startCharacter = indentation.length + declarationPrefix.length;
      return {
        name,
        kind,
        detail,
        line: lineNumber,
        startCharacter,
        endCharacter: startCharacter + name.length,
        indent: indentation.length,
        children: []
      };
    }
  }

  const match = line.match(
    /^(\s*)(?:(?:public|copy)\s+)*(class|enum|trait|def)\s+([A-Za-z_][A-Za-z0-9_]*)/
  );
  if (!match) {
    return null;
  }
  const [, prefix, declaration, name] = match;
  const startCharacter = line.indexOf(name, prefix.length);
  const kind = declaration === "def" ? "function" : declaration;
  return {
    name,
    kind,
    detail: kind,
    line: lineNumber,
    startCharacter,
    endCharacter: startCharacter + name.length,
    indent: indentation(line),
    children: []
  };
}

function recoverSymbols(text) {
  const roots = [];
  const containers = [];
  for (const [lineNumber, line] of text.split(/\r?\n/).entries()) {
    const symbol = declarationForLine(line, lineNumber);
    if (!symbol) {
      continue;
    }
    while (
      containers.length > 0 &&
      symbol.indent <= containers[containers.length - 1].indent
    ) {
      containers.pop();
    }
    const parent = containers[containers.length - 1];
    if (parent) {
      if (symbol.kind === "function") {
        symbol.kind = "method";
      }
      parent.children.push(symbol);
    } else {
      roots.push(symbol);
    }
    if (["class", "enum", "trait"].includes(symbol.kind)) {
      containers.push(symbol);
    }
  }
  return roots;
}

function publicSymbol(symbol) {
  return {
    name: symbol.name,
    kind: symbol.kind,
    detail: symbol.detail,
    line: symbol.line,
    startCharacter: symbol.startCharacter,
    endCharacter: symbol.endCharacter,
    children: symbol.children.map(publicSymbol)
  };
}

function documentSymbols(text) {
  return recoverSymbols(text).map(publicSymbol);
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
  for (const symbol of symbols) {
    flattened.push(symbol, ...flattenSymbols(symbol.children));
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
  if (triggerCharacter === "." || /\.\s*$/.test(linePrefix(text, line, character))) {
    return [];
  }
  const items = [
    ...KEYWORDS.map((name) => ({ name, kind: "keyword", detail: "keyword" })),
    ...BUILTINS.map((name) => ({ name, kind: "function", detail: "builtin" })),
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
  const lines = text.split(/\r?\n/);
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

function recoveredDeclaration(text, name) {
  return flattenSymbols(documentSymbols(text)).find((symbol) => symbol.name === name) || null;
}

function definitionForPosition(text, line, character) {
  const word = wordAtPosition(text, line, character);
  if (!word) {
    return null;
  }
  const declaration = recoveredDeclaration(text, word.name);
  if (!declaration) {
    return null;
  }
  return {
    line: declaration.line,
    startCharacter: declaration.startCharacter,
    endCharacter: declaration.endCharacter
  };
}

function hoverForPosition(text, line, character) {
  const word = wordAtPosition(text, line, character);
  if (!word) {
    return null;
  }
  const declaration = recoveredDeclaration(text, word.name);
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
