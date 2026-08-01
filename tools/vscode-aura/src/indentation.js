"use strict";

const BLOCK_HEADER_PATTERN = /^\s*(class|enum|trait|def|if|elif|else|while|for|match|case|with|impl)\b.*:\s*(#.*)?$/;

function stringLiteralEnd(text, openingIndex, quote) {
  let escaped = false;
  for (let index = openingIndex + 1; index < text.length; index += 1) {
    const character = text[index];
    if (escaped) {
      escaped = false;
    } else if (character === "\\") {
      escaped = true;
    } else if (character === quote) {
      return index;
    }
  }
  return text.length;
}

function fStringLiteralEnd(text, openingQuoteIndex) {
  let interpolationDepth = 0;
  let interpolationQuote = null;
  let interpolationEscaped = false;

  for (let index = openingQuoteIndex + 1; index < text.length; index += 1) {
    const character = text[index];
    if (interpolationDepth === 0) {
      if (character === '"') {
        return index;
      }
      if (character === "\\") {
        index += 1;
      } else if (
        (character === "{" || character === "}") &&
        text[index + 1] === character
      ) {
        index += 1;
      } else if (character === "{") {
        interpolationDepth = 1;
      }
      continue;
    }

    if (interpolationQuote !== null) {
      if (interpolationEscaped) {
        interpolationEscaped = false;
      } else if (character === "\\") {
        interpolationEscaped = true;
      } else if (character === interpolationQuote) {
        interpolationQuote = null;
      }
    } else if (character === "'" || character === '"') {
      interpolationQuote = character;
    } else if (character === "{") {
      interpolationDepth += 1;
    } else if (character === "}") {
      interpolationDepth -= 1;
    }
  }

  return text.length;
}

function scanLineDelimiters(text, initialStack = []) {
  const openingForClosing = {
    ")": "(",
    "]": "[",
    "}": "{"
  };
  const openingDelimiters = new Set(Object.values(openingForClosing));
  const stack = [...initialStack];
  let codeEnd = text.length;

  for (let index = 0; index < text.length; index += 1) {
    const character = text[index];
    if (character === "#") {
      codeEnd = index;
      break;
    }
    if (character === "f" && text[index + 1] === '"') {
      index = fStringLiteralEnd(text, index + 1);
      continue;
    }
    if (character === "'" || character === '"') {
      index = stringLiteralEnd(text, index, character);
      continue;
    }
    if (openingDelimiters.has(character)) {
      stack.push(character);
      continue;
    }

    const expectedOpening = openingForClosing[character];
    if (expectedOpening !== undefined && stack.at(-1) === expectedOpening) {
      stack.pop();
    }
  }

  return {
    code: text.slice(0, codeEnd),
    stack
  };
}

function hasUnmatchedOpeningDelimiter(text) {
  return scanLineDelimiters(text).stack.length > 0;
}

function computeAuraNewlineIndent(
  lineText,
  character,
  indentUnit,
  precedingLines = []
) {
  const safeCharacter = Math.max(0, Math.min(character, lineText.length));
  const beforeText = lineText.slice(0, safeCharacter);
  const lineIndent = (lineText.match(/^\s*/) || [""])[0];
  const lines = [...precedingLines, beforeText];
  const codeLines = [];
  let stack = [];
  let stackBeforeCurrent = [];
  let logicalStart = lines.length - 1;

  for (let index = 0; index < lines.length; index += 1) {
    if (index === lines.length - 1) {
      stackBeforeCurrent = [...stack];
    }
    const scanned = scanLineDelimiters(lines[index], stack);
    if (stack.length === 0 && scanned.code.trim() !== "") {
      logicalStart = index;
    }
    codeLines.push(scanned.code);
    stack = scanned.stack;
  }

  const logicalStartText = lines[logicalStart] || "";
  const logicalIndent = (logicalStartText.match(/^\s*/) || [""])[0];
  const logicalText = codeLines
    .slice(logicalStart)
    .map((line, index) => (index === 0 ? line.trimEnd() : line.trim()))
    .join(" ");

  if (BLOCK_HEADER_PATTERN.test(logicalText)) {
    return logicalIndent + indentUnit;
  }
  if (stack.length > stackBeforeCurrent.length) {
    return lineIndent + indentUnit;
  }
  if (stack.length > 0) {
    return lineIndent;
  }
  if (logicalStart < lines.length - 1) {
    return logicalIndent;
  }

  return lineIndent;
}

module.exports = {
  BLOCK_HEADER_PATTERN,
  computeAuraNewlineIndent,
  hasUnmatchedOpeningDelimiter,
  scanLineDelimiters
};
