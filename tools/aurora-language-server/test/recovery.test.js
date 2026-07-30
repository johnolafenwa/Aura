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
  assert.ok(names.includes("assert"));
  assert.ok(names.includes("lambda"));
  assert.ok(names.includes("int"));
  assert.ok(names.includes("int32"));
  assert.ok(names.includes("int64"));
  assert.ok(names.includes("own"));
  assert.equal(names.includes("borrow"), false);
  assert.ok(names.includes("print"));
  assert.ok(names.includes("select"));
  assert.ok(names.includes("SelectOutcome"));
  assert.ok(names.includes("yield_now"));
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
  const symbols = documentSymbols(
    "enum State:\n    Ready\ntrait Show:\n    def show(self) -> String\n    def consume(own self) -> String\n"
  );
  assert.deepEqual(symbols.map((symbol) => symbol.kind), ["enum", "trait"]);
  assert.deepEqual(
    symbols[1].children.map((symbol) => [symbol.name, symbol.kind]),
    [["show", "method"], ["consume", "method"]]
  );
});

test("recovery preserves extern C declarations while the compiler is unavailable", () => {
  const ffiSource = [
    'public extern "C" opaque class ProcessHandle',
    'public extern "C" def getpid() -> int32',
    ""
  ].join("\n");
  const symbols = documentSymbols(ffiSource);

  assert.deepEqual(
    symbols.map((symbol) => [symbol.name, symbol.kind]),
    [["ProcessHandle", "class"], ["getpid", "function"]]
  );
  const completions = completionsForDocument(ffiSource, 2, 0, null);
  const names = new Set(completions.map((item) => item.name));
  for (const expected of ["extern", "opaque", "ProcessHandle", "getpid"]) {
    assert.ok(names.has(expected), `recovery completion should include ${expected}`);
  }
  assert.deepEqual(definitionForPosition(`${ffiSource}getpid()\n`, 2, 2), {
    line: 1,
    startCharacter: ffiSource.split("\n")[1].indexOf("getpid"),
    endCharacter: ffiSource.split("\n")[1].indexOf("getpid") + "getpid".length
  });
  assert.deepEqual(hoverForPosition(`${ffiSource}getpid()\n`, 2, 2), {
    value: 'extern "C" function getpid',
    range: {
      start: { line: 2, character: 0 },
      end: { line: 2, character: 6 }
    }
  });

  const unsupported = documentSymbols(
    [
      'public extern "C" enum WrongEnum',
      'public extern "C" opaque def wrong_function() -> int32',
      'copy extern "C" opaque class CopyHandle',
      ""
    ].join("\n")
  );
  assert.deepEqual(
    unsupported,
    [],
    "recovery must not present unsupported extern spellings as valid declarations"
  );
});

test("recovery locates an extern declaration named C after the ABI string", () => {
  for (const source of [
    'extern "C" def C() -> int32\nC()\n',
    'extern "C" opaque class C\nC\n'
  ]) {
    const declarationLine = source.split("\n")[0];
    const expectedStart = declarationLine.lastIndexOf("C");
    assert.deepEqual(documentSymbols(source)[0], {
      name: "C",
      kind: source.includes(" def ") ? "function" : "class",
      detail: source.includes(" def ")
        ? 'extern "C" function'
        : 'extern "C" opaque class',
      line: 0,
      startCharacter: expectedStart,
      endCharacter: expectedStart + 1,
      children: []
    });
    assert.deepEqual(definitionForPosition(source, 1, 0), {
      line: 0,
      startCharacter: expectedStart,
      endCharacter: expectedStart + 1
    });
  }
});
