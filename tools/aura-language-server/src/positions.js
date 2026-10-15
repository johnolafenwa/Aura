"use strict";

// Compiler analysis columns are zero-based UTF-8 byte offsets. LSP positions
// count UTF-16 code units. These helpers convert between the two at the
// protocol boundary, one source line at a time.

const ASCII_LINE = /^[\x00-\x7f]*$/;

function utf8Length(character) {
  const codePoint = character.codePointAt(0);
  if (codePoint < 0x80) {
    return 1;
  }
  if (codePoint < 0x800) {
    return 2;
  }
  return codePoint < 0x10000 ? 3 : 4;
}

function utf16ToByteColumn(lineText, character) {
  if (ASCII_LINE.test(lineText)) {
    return character;
  }
  let units = 0;
  let bytes = 0;
  for (const codePoint of lineText) {
    if (units >= character) {
      return bytes;
    }
    units += codePoint.length;
    bytes += utf8Length(codePoint);
  }
  return bytes + Math.max(0, character - units);
}

function byteToUtf16Column(lineText, column) {
  if (ASCII_LINE.test(lineText)) {
    return column;
  }
  let units = 0;
  let bytes = 0;
  for (const codePoint of lineText) {
    if (bytes >= column) {
      return units;
    }
    units += codePoint.length;
    bytes += utf8Length(codePoint);
  }
  return units + Math.max(0, column - bytes);
}

// Maps positions in one source text. Without text, positions pass through.
function createPositionMapper(source) {
  let lines = null;
  function lineText(line) {
    if (typeof source !== "string") {
      return "";
    }
    if (!lines) {
      lines = source.split("\n").map((text) => (text.endsWith("\r") ? text.slice(0, -1) : text));
    }
    return lines[line] || "";
  }
  return {
    toCompilerColumn(line, character) {
      return utf16ToByteColumn(lineText(line), character);
    },
    toEditorColumn(line, column) {
      return byteToUtf16Column(lineText(line), column);
    },
    range(line, startColumn, endColumn) {
      const text = lineText(line);
      return {
        start: { line, character: byteToUtf16Column(text, startColumn) },
        end: { line, character: byteToUtf16Column(text, endColumn) }
      };
    }
  };
}

module.exports = {
  byteToUtf16Column,
  createPositionMapper,
  utf16ToByteColumn
};
