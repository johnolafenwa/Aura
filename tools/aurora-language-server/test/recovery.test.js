"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const {
  analyzeDocument,
  completionsForDocument,
  definitionForPosition,
  diagnosticsForDocument,
  documentSymbols,
  hoverForPosition
} = require("../src/recovery");

const source = [
  "class Point:",
  "    x: int32",
  "    def magnitude(self) -> float64:",
  "        return 0.0",
  "",
  "def main() -> int32:",
  "    value = Point(x=1)",
  "    print(value)",
  "    return 0",
  ""
].join("\n");

test("recovery analysis only recovers declaration structure", () => {
  const analysis = analyzeDocument(source);
  assert.deepEqual(analysis.diagnostics, []);
  assert.deepEqual(
    analysis.symbols.map((symbol) => [symbol.name, symbol.kind]),
    [["Point", "class"], ["main", "function"]]
  );
  assert.deepEqual(
    analysis.symbols[0].children.map((symbol) => [symbol.name, symbol.kind]),
    [["magnitude", "method"]]
  );
  assert.deepEqual(diagnosticsForDocument("def broken("), []);
});

test("recovery completion exposes keywords builtins and declarations but no member semantics", () => {
  const names = completionsForDocument(source, 8, 4, null).map((item) => item.name);
  assert.ok(names.includes("class"));
  assert.ok(names.includes("int"));
  assert.ok(names.includes("int32"));
  assert.ok(names.includes("int64"));
  assert.ok(names.includes("print"));
  assert.ok(names.includes("Point"));
  assert.ok(names.includes("main"));
  assert.deepEqual(completionsForDocument(source, 6, 10, "."), []);
  assert.deepEqual(completionsForDocument("value.", 0, 6, null), []);
  assert.ok(completionsForDocument("def print():\n    pass\n", 99, -1, null));
  assert.equal(
    completionsForDocument("def print():\n    pass\n", 1, 0, null).filter(
      (item) => item.name === "print"
    ).length,
    1
  );
});

test("recovery hover and definition resolve only recovered declarations", () => {
  assert.deepEqual(definitionForPosition(source, 6, 14), {
    line: 0,
    startCharacter: 6,
    endCharacter: 11
  });
  assert.deepEqual(hoverForPosition(source, 6, 14), {
    value: "class Point",
    range: {
      start: { line: 6, character: 12 },
      end: { line: 6, character: 17 }
    }
  });
  assert.equal(definitionForPosition(source, 7, 10), null);
  assert.equal(definitionForPosition(source, 7, 4), null);
  assert.equal(definitionForPosition(source, 7, 0), null);
  assert.equal(definitionForPosition(source, 99, 0), null);
  assert.equal(hoverForPosition(source, 7, 10), null);
  assert.equal(hoverForPosition(source, 99, 0), null);
});

test("recovery document symbols tolerate empty and nested declarations", () => {
  assert.deepEqual(documentSymbols(""), []);
  const symbols = documentSymbols("enum State:\n    Ready\ntrait Show:\n    def show(self) -> String\n");
  assert.deepEqual(symbols.map((symbol) => symbol.kind), ["enum", "trait"]);
  assert.equal(symbols[1].children[0].kind, "method");
});
