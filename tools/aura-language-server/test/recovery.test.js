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
  assert.ok(names.includes("as"));
  assert.ok(names.includes("assert"));
  assert.ok(names.includes("lambda"));
  assert.ok(names.includes("int"));
  assert.ok(names.includes("int32"));
  assert.ok(names.includes("int64"));
  assert.ok(names.includes("own"));
  assert.ok(names.includes("print"));
  assert.ok(names.includes("select"));
  assert.ok(names.includes("SelectOutcome"));
  assert.ok(names.includes("Array"));
  for (const expected of ["list", "dict", "set", "str"]) {
    assert.ok(names.includes(expected), `recovery completion should include ${expected}`);
  }
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
    "enum State:\n    Ready\ntrait Show:\n    def show(self) -> str\n    def consume(own self) -> str\n"
  );
  assert.deepEqual(symbols.map((symbol) => symbol.kind), ["enum", "trait"]);
  assert.deepEqual(
    symbols[1].children.map((symbol) => [symbol.name, symbol.kind]),
    [["show", "method"], ["consume", "method"]]
  );
});

test("recovery finds exported C functions", () => {
  const source = [
    'export "C" def kernel_main(code: int64) -> int64:',
    "    return code",
    'public export "C" def uart_irq(status: uint32) -> uint32:',
    "    return status",
    ""
  ].join("\n");

  assert.deepEqual(
    documentSymbols(source).map((symbol) => [symbol.name, symbol.kind]),
    [["kernel_main", "function"], ["uart_irq", "function"]]
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

  const structSymbols = documentSymbols(
    ['extern "C" class Point:', "    x: float64", "    y: float64", ""].join("\n")
  );
  assert.deepEqual(
    structSymbols.map((symbol) => [symbol.name, symbol.kind, symbol.detail]),
    [["Point", "class", 'extern "C" class']]
  );
  assert.deepEqual(
    documentSymbols(
      ['extern "C" union Event:', "    kind: uint32", "    key: Key", ""].join("\n")
    ).map((symbol) => [symbol.name, symbol.kind, symbol.detail]),
    [["Event", "class", 'extern "C" union']]
  );
  assert.deepEqual(
    documentSymbols('extern "C" opaque class Database drop sqlite3_close\n').map(
      (symbol) => symbol.name
    ),
    ["Database"]
  );

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

test("recovery handles deeply indented declarations without stack overflow", () => {
  const deep = Array.from({ length: 6000 }, (_, index) => `${" ".repeat(index)}class C${index}:`).join("\n");
  const symbols = documentSymbols(deep);
  assert.equal(symbols.length, 1);
  assert.equal(symbols[0].name, "C0");
  assert.equal(definitionForPosition(`${deep}\nC5999\n`, 6000, 1)?.line, 5999);
});

test("navigation in a discarded suite prefers its own declarations and avoids ambiguous names", () => {
  const text = [
    "const shared = 9",
    "if build.hosted:",
    "    def implementation():",
    "        pass",
    "else:",
    "    def implementation():",
    "        pass",
    "    def caller():",
    "        print(implementation(), shared)",
    "    class First:",
    "        def work(self):",
    "            pass",
    "    class Second:",
    "        def work(self):",
    "            pass",
    "    def ambiguous():",
    "        work()"
  ].join("\n");
  const scope = { start: 5, end: 17 };
  assert.deepEqual(definitionForPosition(text, 8, 16, scope), {
    line: 5, startCharacter: 8, endCharacter: 22
  });
  assert.equal(hoverForPosition(text, 8, 33, scope)?.value, "constant shared");
  assert.equal(hoverForPosition(text, 16, 9, scope), null);
  assert.ok(completionsForDocument(text, 0, 0).some((item) => item.name === "const"));
});

test("assembly templates stay opaque to recovery symbols, navigation and completion", () => {
  const text = [
    'module_asm("""',
    'def fictitious():',
    'call real',
    '""")',
    'def real() -> None:',
    '    asm("call real")'
  ].join("\n");
  assert.deepEqual(documentSymbols(text).map(s => s.name), ["real"]);
  for (const [line, character] of [[2, 6], [5, 15]]) {
    assert.equal(hoverForPosition(text, line, character), null);
    assert.equal(definitionForPosition(text, line, character), null);
    assert.deepEqual(completionsForDocument(text, line, character), []);
  }
});

test("contextual machine names offer hover and completion only in their syntax slots", () => {
  const source = [
    '@machine.interrupt("x86_64")',
    'export "C" def irq() -> None:',
    '    asm("nop",',
    '        input("reg", helper()),',
    '        options("preserves_flags"))',
    '    x = ordinary(input())',
    '    # asm("call helper")',
    'def helper() -> uint64:',
    '    return 1'
  ].join("\n");
  const at = (line, word) => source.split("\n")[line].indexOf(word) + 1;
  assert.match(hoverForPosition(source, 0, at(0, "interrupt")).value, /interrupt/i);
  assert.match(hoverForPosition(source, 2, at(2, "asm")).value, /assembly/i);
  assert.match(hoverForPosition(source, 3, at(3, "input")).value, /operand/i);
  assert.equal(hoverForPosition(source, 5, at(5, "input")), null);
  assert.equal(definitionForPosition(source, 3, at(3, "helper")).line, 7);
  assert.deepEqual(completionsForDocument(source, 3, 8).map(item => item.name).sort(),
    ["clobber", "input", "options", "output"]);
  assert.deepEqual(completionsForDocument("@machine.", 0, 9, ".").map(item => item.name).sort(),
    ["align", "always_inline", "interrupt", "link_name", "naked", "noinline", "section", "used"]);
});

test("an unfinished multiline template and CRLF preserve editor coordinates", () => {
  const lines = [
    '# module_asm("""',
    'class Kernel:',
    '    asm(',
    "        r'Kernel',",
    '        input("reg", Kernel))',
    "module_asm('''",
    "😀 Kernel",
    "def hidden()"
  ];
  const text = lines.join("\r\n");
  assert.deepEqual(documentSymbols(text).map(s => s.name), ["Kernel"]);
  assert.deepEqual(completionsForDocument(text, 6, 4), []);
  assert.equal(hoverForPosition(text, 3, 13), null);
  assert.equal(definitionForPosition(text, 4, 22).line, 1);
  assert.deepEqual(hoverForPosition(text, 4, 10).range, {
    start: {line: 4, character: 8}, end: {line: 4, character: 13}
  });
});

test("typing at the end of an unfinished assembly string suppresses suggestions", () => {
  for (const source of ['    asm("mov', "module_asm('''\n", "    asm('mov\\"]) {
    const lines = source.split("\n");
    assert.deepEqual(completionsForDocument(source, lines.length - 1, lines.at(-1).length), []);
  }
  const complete = '    asm("ret")';
  assert.ok(completionsForDocument(complete, 0, complete.length).length > 0);
});

test("recovery navigates C layout and public module storage declarations", () => {
  const lines = [
    'public extern "C" packed align(8) class Header:',
    "    tag: uint8 bits(3)",
    "public mut ring: fixed[uint16, 2] = [0] * 2",
    'extern "C" packed class Payload:',
    "    bytes: fixed[uint8, 8]",
    'extern "C" align(16) class Wide:',
    "    value: int64",
    "def head(h: Header) -> uint16:",
    "    mut temporary = ring[0]",
    "    return temporary"
  ];
  const source = lines.join("\n");
  const symbols = documentSymbols(source);
  assert.deepEqual(symbols.map((s) => [s.name, s.kind]), [
    ["Header", "class"], ["ring", "variable"], ["Payload", "class"], ["Wide", "class"], ["head", "function"]
  ]);
  for (const [line, use, declaration] of [[7, "Header", 0], [8, "ring", 2]]) {
    const column = lines[line].indexOf(use);
    const start = lines[declaration].indexOf(use);
    assert.deepEqual(definitionForPosition(source, line, column), {
      line: declaration, startCharacter: start, endCharacter: start + use.length
    });
    assert.ok(hoverForPosition(source, line, column).value.includes(use));
  }
  const items = completionsForDocument(source, 9, 4, null);
  for (const name of ["fixed", "ptr", "ring", "Header"]) {
    assert.ok(items.some((item) => item.name === name), name);
  }
  assert.ok(!items.some((item) => item.name === "temporary"));
  assert.deepEqual(diagnosticsForDocument(source), []);
});

test("recovery keeps top-level script locals out of module symbols", () => {
  assert.deepEqual(documentSymbols("mut result = 0\nprint(result)"), []);
  const source = 'mut count: uint64 = 0\nexport "C" def tick() -> uint64:\n    return count';
  assert.deepEqual(documentSymbols(source).map((symbol) => symbol.name), ["count", "tick"]);
});

test("recovery recognizes alignment followed by packing and base-prefixed lengths", () => {
  const source = 'extern "C" align(0x10) packed class Header:\n    value: uint32 bits(0b1_00)';
  assert.deepEqual(documentSymbols(source).map((s) => s.name), ["Header"]);
});
