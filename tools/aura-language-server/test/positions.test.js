"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");

const {
  byteToUtf16Column,
  createPositionMapper,
  utf16ToByteColumn
} = require("../src/positions");

test("UTF-16 editor columns convert to UTF-8 compiler columns", () => {
  const line = 'x = "é€😀" + y';
  assert.equal(utf16ToByteColumn("value", 3), 3);
  assert.equal(utf16ToByteColumn(line, 5), 5);
  assert.equal(utf16ToByteColumn(line, 6), 7);
  assert.equal(utf16ToByteColumn(line, 7), 10);
  assert.equal(utf16ToByteColumn(line, 9), 14);
  assert.equal(utf16ToByteColumn(line, line.length), Buffer.byteLength(line));
  assert.equal(utf16ToByteColumn(line, line.length + 2), Buffer.byteLength(line) + 2);
  // A position inside a surrogate pair moves to the end of that character.
  assert.equal(utf16ToByteColumn(line, 8), 14);
  assert.equal(utf16ToByteColumn("😀", 1), 4);
});

test("UTF-8 compiler columns convert to UTF-16 editor columns", () => {
  const line = 'x = "é€😀" + y';
  assert.equal(byteToUtf16Column("value", 3), 3);
  assert.equal(byteToUtf16Column(line, 5), 5);
  assert.equal(byteToUtf16Column(line, 7), 6);
  assert.equal(byteToUtf16Column(line, 10), 7);
  assert.equal(byteToUtf16Column(line, 14), 9);
  assert.equal(byteToUtf16Column(line, Buffer.byteLength(line)), line.length);
  assert.equal(byteToUtf16Column(line, Buffer.byteLength(line) + 3), line.length + 3);
  // A column inside a multibyte character moves to the end of that character.
  assert.equal(byteToUtf16Column(line, 6), 6);
});

test("position mappers convert per line and pass through without source text", () => {
  const mapper = createPositionMapper('first\r\nsay("😀", name)\nlast');
  assert.equal(mapper.toCompilerColumn(1, 9), 11);
  assert.equal(mapper.toEditorColumn(1, 11), 9);
  assert.deepEqual(mapper.range(1, 11, 15), {
    start: { line: 1, character: 9 },
    end: { line: 1, character: 13 }
  });
  assert.equal(mapper.toEditorColumn(0, 5), 5);
  assert.equal(mapper.toEditorColumn(7, 4), 4);

  const passthrough = createPositionMapper(null);
  assert.equal(passthrough.toCompilerColumn(0, 6), 6);
  assert.deepEqual(passthrough.range(2, 1, 3), {
    start: { line: 2, character: 1 },
    end: { line: 2, character: 3 }
  });
});
