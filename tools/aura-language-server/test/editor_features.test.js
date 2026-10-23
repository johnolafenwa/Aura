"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { pathToFileURL } = require("node:url");

const {
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
} = require("../src/editor_features");

const FILE = "/work/main.au";

function occurrence(line, start, end, hover, definition) {
  return {
    line,
    start_character: start,
    end_character: end,
    hover: `\`\`\`aura\n${hover}\n\`\`\``,
    definition: definition
      ? { file_path: FILE, line: definition[0], start_character: definition[1], end_character: definition[2] }
      : null
  };
}

const SOURCE = [
  "def main() -> int32:",
  "    total = 1 + LIMIT",
  "    for item in range(total):",
  "        print(item)",
  "    sq.area()",
  "    typed: int64 = 2",
  "    é = total == 1",
  ""
].join("\n");

const ANALYSIS = {
  occurrences: [
    occurrence(1, 4, 9, "binding total: int64", [1, 4, 9]),
    occurrence(1, 16, 21, "module constant LIMIT: int64", [0, 0, 5]),
    occurrence(2, 8, 12, "local item: int64", [2, 8, 12]),
    occurrence(2, 16, 21, "range(stop: int64) -> Range", null),
    occurrence(2, 22, 27, "binding total: int64", [1, 4, 9]),
    occurrence(3, 8, 13, "print(value) -> None", null),
    occurrence(3, 14, 18, "local item: int64", [2, 8, 12]),
    // An overlapping occurrence is skipped by semantic tokens.
    occurrence(3, 15, 17, "local item: int64", [2, 8, 12]),
    occurrence(4, 4, 6, "binding sq: Square", [9, 4, 6]),
    occurrence(4, 7, 11, "area() -> int64", null),
    occurrence(5, 4, 9, "binding typed: int64", [5, 4, 9]),
    occurrence(6, 4, 6, "binding é: bool", [6, 4, 6]),
    occurrence(6, 9, 14, "unknown thing", [1, 4, 9]),
    occurrence(6, 3, 3, "binding empty: int64", null)
  ]
};

test("document highlights mark the declaration as a write and uses as reads", () => {
  const highlights = documentHighlights(ANALYSIS, SOURCE, 2, 23);
  assert.deepEqual(
    highlights.map((highlight) => [highlight.range.start.line, highlight.range.start.character, highlight.kind]),
    [
      [1, 4, 3],
      [2, 22, 2],
      [6, 8, 2]
    ]
  );
  assert.deepEqual(documentHighlights(ANALYSIS, SOURCE, 3, 9), []);
  assert.deepEqual(documentHighlights(ANALYSIS, SOURCE, 0, 0), []);
  assert.deepEqual(documentHighlights(null, SOURCE, 0, 0), []);
});

test("semantic tokens classify occurrences from their hover", () => {
  const { data } = semanticTokens(ANALYSIS, SOURCE);
  const tokens = [];
  let line = 0;
  let start = 0;
  for (let index = 0; index < data.length; index += 5) {
    line += data[index];
    start = data[index] === 0 ? start + data[index + 1] : data[index + 1];
    tokens.push([line, start, data[index + 2], SEMANTIC_TOKEN_TYPES[data[index + 3]], data[index + 4]]);
  }
  assert.deepEqual(tokens, [
    [1, 4, 5, "variable", 1],
    [1, 16, 5, "variable", 2],
    [2, 8, 4, "variable", 1],
    [2, 16, 5, "function", 0],
    [2, 22, 5, "variable", 0],
    [3, 8, 5, "function", 0],
    [3, 14, 4, "variable", 0],
    [4, 4, 2, "variable", 0],
    [4, 7, 4, "method", 0],
    [5, 4, 5, "variable", 1],
    [6, 4, 1, "variable", 1]
  ]);
  assert.deepEqual(semanticTokens(null, null), { data: [] });
  // A hover without a code fence, and an occurrence past the end of the text.
  assert.deepEqual(
    semanticTokens(
      {
        occurrences: [
          { line: 0, start_character: 0, end_character: 1, hover: "binding a: int64", definition: null },
          occurrence(9, 0, 1, "f() -> None", null)
        ]
      },
      "a\n"
    ).data,
    [0, 0, 1, SEMANTIC_TOKEN_TYPES.indexOf("variable"), 0, 9, 0, 1, SEMANTIC_TOKEN_TYPES.indexOf("function"), 0]
  );
  assert.deepEqual(
    semanticTokens(
      {
        occurrences: [
          occurrence(0, 0, 1, "param a: int64", null),
          occurrence(0, 2, 3, "enum Color", null),
          occurrence(0, 4, 5, "variant Red -> Color", null),
          occurrence(0, 6, 7, "class Box", null),
          occurrence(0, 8, 9, "field x: int64", null),
          occurrence(0, 10, 11, "module math", null),
          occurrence(0, 12, 13, "trait Shape", null),
          occurrence(1, 0, 1, "type Alias = int64", null),
          occurrence(1, 2, 3, "method run(self) -> None", null),
          occurrence(1, 4, 5, "function f() -> None", null)
        ]
      },
      "a b c d e f g\na b c\n"
    ).data.filter((_, index) => index % 5 === 3).map((type) => SEMANTIC_TOKEN_TYPES[type]),
    ["parameter", "enum", "enumMember", "class", "property", "namespace", "interface", "type", "method", "function"]
  );
});

test("inlay hints show the inferred type of unannotated locals", () => {
  const hints = inlayHints(ANALYSIS, SOURCE);
  assert.deepEqual(
    hints.map((hint) => [hint.position.line, hint.position.character, hint.label]),
    [
      [1, 9, ": int64"],
      [2, 12, ": int64"],
      [6, 5, ": bool"]
    ]
  );
  assert.deepEqual(
    inlayHints(ANALYSIS, SOURCE, { start: { line: 2, character: 0 }, end: { line: 3, character: 0 } }).map(
      (hint) => hint.position.line
    ),
    [2]
  );
  assert.deepEqual(inlayHints(null, ""), []);
  assert.deepEqual(
    inlayHints({ occurrences: [occurrence(0, 0, 1, "param a: int64", [0, 0, 1])] }, "a = 1"),
    []
  );
  assert.deepEqual(
    inlayHints({ occurrences: [occurrence(4, 0, 1, "binding a: int64", [4, 0, 1])] }, "a = 1"),
    []
  );
});

test("quick fixes apply compiler edits and prefix unused locals", () => {
  const uri = pathToFileURL(FILE).toString();
  const range = { start: { line: 1, character: 4 }, end: { line: 1, character: 9 } };
  const actions = codeActions(uri, SOURCE, [
    {
      code: "AU2001",
      range,
      data: {
        help: ["use `true`"],
        edits: [
          { line: 1, start_character: 4, end_character: 9, replacement: "true", applicability: "machine-applicable" }
        ]
      }
    },
    {
      code: "AU2002",
      range,
      data: {
        edits: [
          { line: 1, start_character: 4, end_character: 9, replacement: "x", applicability: "maybe-incorrect" },
          { line: 1, start_character: 4, end_character: 9, replacement: "y", applicability: "unspecified" }
        ]
      }
    },
    { code: "AU5001", range },
    { code: "AU2003", range, data: { edits: [] } }
  ]);
  assert.deepEqual(
    actions.map((action) => [action.title, action.isPreferred, action.edit.changes[uri].map((edit) => edit.newText)]),
    [
      ["use `true`", true, ["true"]],
      ["Apply the compiler's suggested fix", false, ["x"]],
      ["Prefix the name with `_` to mark it unused", true, ["_"]]
    ]
  );
  assert.deepEqual(actions[2].edit.changes[uri][0].range, { start: range.start, end: range.start });
  assert.deepEqual(codeActions(uri, SOURCE, undefined), []);
});

test("workspace symbols and implementations read every Aura file in the workspace", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "aura-workspace-symbols-"));
  try {
    fs.mkdirSync(path.join(root, "shapes"));
    fs.mkdirSync(path.join(root, "node_modules", "dep"), { recursive: true });
    fs.mkdirSync(path.join(root, ".hidden"));
    fs.writeFileSync(
      path.join(root, "shapes", "square.au"),
      "class Square:\n    side: int64\n\n    def area(self) -> int64:\n        return 1\n\nimpl Shape for Square:\n    def area(self) -> int64:\n        return 1\n"
    );
    fs.writeFileSync(path.join(root, "shapes", "notes.txt"), "class Ignored:\n");
    fs.writeFileSync(path.join(root, "node_modules", "dep", "skip.au"), "class Skipped:\n");
    fs.writeFileSync(path.join(root, ".hidden", "skip.au"), "class Hidden:\n");
    fs.writeFileSync(path.join(root, "main.au"), "trait Shape:\n    def area(self) -> int64\n");

    assert.deepEqual(workspaceAuraFiles([root]).map((file) => path.relative(root, file)).sort(), [
      "main.au",
      path.join("shapes", "square.au")
    ]);
    assert.equal(workspaceAuraFiles([root], 1).length, 1);
    assert.deepEqual(workspaceAuraFiles([path.join(root, "missing")]), []);
    assert.deepEqual(workspaceAuraFiles(undefined), []);

    const openUri = pathToFileURL(path.join(root, "main.au")).toString();
    const sources = workspaceSources([root], [{ uri: openUri, text: "trait Shape:\n    def area(self) -> int64\n\ndef sq_helper():\n    pass\n" }]);
    assert.match(sources.get(openUri), /sq_helper/);
    assert.deepEqual(workspaceSources([], undefined).size, 0);

    const symbols = workspaceSymbols("sq", sources);
    assert.deepEqual(symbols.map((symbol) => [symbol.name, symbol.kind, symbol.containerName]).sort(), [
      ["Square", 5, undefined],
      ["sq_helper", 12, undefined]
    ]);
    const areas = workspaceSymbols("area", sources);
    assert.ok(areas.some((symbol) => symbol.containerName === "Square" && symbol.kind === 6));
    assert.ok(areas.some((symbol) => symbol.containerName === "Shape"));
    assert.equal(workspaceSymbols("", sources).length >= 4, true);

    const ofTrait = implementations("Shape", sources);
    assert.equal(ofTrait.length, 1);
    assert.equal(ofTrait[0].range.start.line, 6);
    assert.equal(implementations("Square", sources).length, 1);
    assert.deepEqual(implementations("not a name", sources), []);
    assert.deepEqual(implementations(null, sources), []);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test("a file that disappears while the workspace is read is skipped", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "aura-workspace-race-"));
  try {
    fs.writeFileSync(path.join(root, "gone.au"), "");
    const originalRead = fs.readFileSync;
    fs.readFileSync = (file, ...rest) => {
      if (String(file).endsWith("gone.au")) {
        throw new Error("ENOENT");
      }
      return originalRead(file, ...rest);
    };
    try {
      assert.equal(workspaceSources([root], []).size, 0);
    } finally {
      fs.readFileSync = originalRead;
    }
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test("wordAt finds the identifier under or just after the cursor", () => {
  assert.equal(wordAt("impl Shape for Square:", 0, 6), "Shape");
  assert.equal(wordAt("impl Shape for Square:", 0, 10), "Shape");
  assert.equal(wordAt("x = 1", 0, 3), null);
  assert.equal(wordAt("", 4, 0), null);
  assert.equal(wordAt(null, 0, 0), null);
});
