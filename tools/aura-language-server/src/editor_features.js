"use strict";

// Editor features computed from a compiler analysis record: highlights,
// semantic tokens, inlay type hints, quick fixes, workspace symbols, and
// implementations. The compiler owns the facts; these functions only reshape
// them for the protocol.

const fs = require("node:fs");
const path = require("node:path");
const { pathToFileURL } = require("node:url");
const { createPositionMapper } = require("./positions");
const { documentSymbols } = require("./recovery");

const SEMANTIC_TOKEN_TYPES = [
  "namespace",
  "class",
  "enum",
  "interface",
  "type",
  "parameter",
  "variable",
  "property",
  "enumMember",
  "function",
  "method"
];
const SEMANTIC_TOKEN_MODIFIERS = ["declaration", "readonly"];

const TOKEN_TYPE_BY_HOVER_WORD = {
  binding: "variable",
  local: "variable",
  view: "variable",
  constant: "variable",
  param: "parameter",
  parameter: "parameter",
  enum: "enum",
  variant: "enumMember",
  function: "function",
  class: "class",
  field: "property",
  property: "property",
  method: "method",
  module: "namespace",
  type: "type",
  trait: "interface"
};

const SKIPPED_DIRECTORIES = new Set([".git", "node_modules", "target", "dist", "out"]);
const WORKSPACE_FILE_LIMIT = 2000;

function definitionKey(definition) {
  return definition
    ? `${definition.file_path}:${definition.line}:${definition.start_character}`
    : null;
}

function isDefinitionSite(occurrence) {
  const definition = occurrence.definition;
  return Boolean(
    definition &&
      definition.line === occurrence.line &&
      definition.start_character === occurrence.start_character &&
      definition.end_character === occurrence.end_character
  );
}

// The first line of a hover's code block, such as `binding total: int64`.
function hoverSignature(hover) {
  const text = String(hover);
  const body = text.startsWith("```aura\n") ? text.slice("```aura\n".length) : text;
  return body.split("\n")[0];
}

function occurrenceAt(analysis, positions, line, character) {
  const column = positions.toCompilerColumn(line, character);
  return (
    ((analysis && analysis.occurrences) || []).find(
      (occurrence) =>
        occurrence.line === line &&
        column >= occurrence.start_character &&
        column < occurrence.end_character
    ) || null
  );
}

// Every occurrence that resolves to the same declaration as the one under
// the cursor. The declaration itself is a write; uses are reads.
function documentHighlights(analysis, source, line, character) {
  const positions = createPositionMapper(source);
  const target = occurrenceAt(analysis, positions, line, character);
  const key = target && definitionKey(target.definition);
  if (!key) {
    return [];
  }
  return analysis.occurrences
    .filter((occurrence) => definitionKey(occurrence.definition) === key)
    .map((occurrence) => ({
      range: positions.range(occurrence.line, occurrence.start_character, occurrence.end_character),
      kind: isDefinitionSite(occurrence) ? 3 : 2
    }));
}

function semanticTokenType(occurrence, textBefore) {
  const signature = hoverSignature(occurrence.hover).replace(/^module constant /, "constant ");
  const word = signature.split(" ")[0];
  if (Object.prototype.hasOwnProperty.call(TOKEN_TYPE_BY_HOVER_WORD, word)) {
    return TOKEN_TYPE_BY_HOVER_WORD[word];
  }
  if (word.includes("(")) {
    return textBefore.trimEnd().endsWith(".") ? "method" : "function";
  }
  return null;
}

// LSP relative token encoding of the analysis occurrences, in source order.
function semanticTokens(analysis, source) {
  const positions = createPositionMapper(source);
  const lines = String(source || "").split("\n");
  const occurrences = [...((analysis && analysis.occurrences) || [])].sort(
    (left, right) => left.line - right.line || left.start_character - right.start_character
  );
  const data = [];
  let previousLine = 0;
  let previousStart = 0;
  let previousEnd = -1;
  let previousEndLine = -1;
  for (const occurrence of occurrences) {
    if (occurrence.line === previousEndLine && occurrence.start_character < previousEnd) {
      continue;
    }
    const range = positions.range(
      occurrence.line,
      occurrence.start_character,
      occurrence.end_character
    );
    const textBefore = (lines[occurrence.line] || "").slice(0, range.start.character);
    const type = semanticTokenType(occurrence, textBefore);
    if (!type) {
      continue;
    }
    const length = range.end.character - range.start.character;
    if (length <= 0) {
      continue;
    }
    let modifiers = 0;
    if (isDefinitionSite(occurrence)) {
      modifiers |= 1;
    }
    if (/^(module )?constant /.test(hoverSignature(occurrence.hover))) {
      modifiers |= 2;
    }
    const deltaLine = range.start.line - previousLine;
    const deltaStart =
      deltaLine === 0 ? range.start.character - previousStart : range.start.character;
    data.push(deltaLine, deltaStart, length, SEMANTIC_TOKEN_TYPES.indexOf(type), modifiers);
    previousLine = range.start.line;
    previousStart = range.start.character;
    previousEnd = occurrence.end_character;
    previousEndLine = occurrence.line;
  }
  return { data };
}

// `: T` after each local the source binds without writing its type: an
// assignment such as `total = add(1, 2)` or a loop target such as `for item in`.
function inlayHints(analysis, source, range) {
  const positions = createPositionMapper(source);
  const lines = String(source || "").split("\n");
  const hints = [];
  for (const occurrence of (analysis && analysis.occurrences) || []) {
    if (!isDefinitionSite(occurrence)) {
      continue;
    }
    if (range && (occurrence.line < range.start.line || occurrence.line > range.end.line)) {
      continue;
    }
    const match = /^(binding|local) [^:\s]+: (.+)$/.exec(
      hoverSignature(occurrence.hover)
    );
    if (!match) {
      continue;
    }
    const rest = (lines[occurrence.line] || "").slice(
      positions.toEditorColumn(occurrence.line, occurrence.end_character)
    );
    const assigned = /^\s*=(?!=)/.test(rest);
    const loopTarget = match[1] === "local" && /^\s+in\s/.test(rest);
    if (!assigned && !loopTarget) {
      continue;
    }
    hints.push({
      position: positions.range(occurrence.line, occurrence.end_character, occurrence.end_character)
        .end,
      label: `: ${match[2]}`,
      kind: 1,
      paddingLeft: false
    });
  }
  return hints;
}

// Quick fixes: each compiler-suggested edit, and an underscore prefix for an
// unused local.
function codeActions(uri, source, diagnostics) {
  const positions = createPositionMapper(source);
  const actions = [];
  for (const diagnostic of diagnostics || []) {
    const data = diagnostic.data || {};
    const edits = (data.edits || []).filter((edit) => edit.applicability !== "unspecified");
    if (edits.length > 0) {
      actions.push({
        title: (data.help && data.help[0]) || "Apply the compiler's suggested fix",
        kind: "quickfix",
        diagnostics: [diagnostic],
        isPreferred: edits.every((edit) => edit.applicability === "machine-applicable"),
        edit: {
          changes: {
            [uri]: edits.map((edit) => ({
              range: positions.range(edit.line, edit.start_character, edit.end_character),
              newText: edit.replacement
            }))
          }
        }
      });
    }
    if (diagnostic.code === "AU5001") {
      actions.push({
        title: "Prefix the name with `_` to mark it unused",
        kind: "quickfix",
        diagnostics: [diagnostic],
        isPreferred: true,
        edit: {
          changes: {
            [uri]: [
              {
                range: { start: diagnostic.range.start, end: diagnostic.range.start },
                newText: "_"
              }
            ]
          }
        }
      });
    }
  }
  return actions;
}

// The `.au` files under the workspace roots, skipping build and dependency
// folders, up to a fixed count.
function workspaceAuraFiles(roots, limit = WORKSPACE_FILE_LIMIT) {
  const files = [];
  const pending = [...(roots || [])];
  while (pending.length > 0 && files.length < limit) {
    const directory = pending.shift();
    let entries;
    try {
      entries = fs.readdirSync(directory, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      const fullPath = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        if (!SKIPPED_DIRECTORIES.has(entry.name) && !entry.name.startsWith(".")) {
          pending.push(fullPath);
        }
      } else if (entry.isFile() && entry.name.endsWith(".au") && files.length < limit) {
        files.push(fullPath);
      }
    }
  }
  return files;
}

// Open documents win over the files on disk.
function workspaceSources(roots, openDocuments) {
  const sources = new Map();
  for (const filePath of workspaceAuraFiles(roots)) {
    try {
      sources.set(pathToFileURL(filePath).toString(), fs.readFileSync(filePath, "utf8"));
    } catch {
      // A file removed while scanning has nothing to offer.
    }
  }
  for (const document of openDocuments || []) {
    sources.set(document.uri, document.text);
  }
  return sources;
}

function fuzzyMatches(name, query) {
  let index = 0;
  const lowerName = name.toLowerCase();
  for (const character of String(query || "").toLowerCase()) {
    index = lowerName.indexOf(character, index);
    if (index < 0) {
      return false;
    }
    index += 1;
  }
  return true;
}

const SYMBOL_KIND = { class: 5, method: 6, enum: 10, trait: 11, function: 12 };

function workspaceSymbols(query, sources) {
  const results = [];
  for (const [uri, text] of sources) {
    const visit = (symbol, container) => {
      if (fuzzyMatches(symbol.name, query)) {
        const range = {
          start: { line: symbol.line, character: symbol.startCharacter },
          end: { line: symbol.line, character: symbol.endCharacter }
        };
        results.push({
          name: symbol.name,
          kind: SYMBOL_KIND[symbol.kind],
          location: { uri, range },
          ...(container ? { containerName: container } : {})
        });
      }
      for (const child of symbol.children) {
        visit(child, symbol.name);
      }
    };
    for (const symbol of documentSymbols(text)) {
      visit(symbol, null);
    }
  }
  return results;
}

function escapeRegExp(text) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

// `impl` blocks of a trait, or `impl` blocks for a type, by name.
function implementations(name, sources) {
  if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(name || "")) {
    return [];
  }
  const word = escapeRegExp(name);
  const ofTrait = new RegExp(`^\\s*impl(?:\\[[^\\]]*\\])?\\s+${word}\\b`);
  const forType = new RegExp(`^\\s*impl\\b.*\\bfor\\s+${word}\\b`);
  const locations = [];
  for (const [uri, text] of sources) {
    text.split("\n").forEach((lineText, line) => {
      if (ofTrait.test(lineText) || forType.test(lineText)) {
        const start = lineText.length - lineText.trimStart().length;
        locations.push({
          uri,
          range: { start: { line, character: start }, end: { line, character: lineText.trimEnd().length } }
        });
      }
    });
  }
  return locations;
}

function wordAt(source, line, character) {
  const lineText = String(source || "").split("\n")[line] || "";
  for (const match of lineText.matchAll(/[A-Za-z_][A-Za-z0-9_]*/g)) {
    if (character >= match.index && character <= match.index + match[0].length) {
      return match[0];
    }
  }
  return null;
}

module.exports = {
  SEMANTIC_TOKEN_MODIFIERS,
  SEMANTIC_TOKEN_TYPES,
  codeActions,
  documentHighlights,
  implementations,
  inlayHints,
  semanticTokens,
  wordAt,
  workspaceAuraFiles,
  workspaceSources,
  workspaceSymbols
};
