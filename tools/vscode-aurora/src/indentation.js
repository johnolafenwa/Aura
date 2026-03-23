"use strict";

const BLOCK_HEADER_PATTERN = /^\s*(class|enum|trait|def|if|elif|else|while|for|match|case|with|select|impl)\b.*:\s*(#.*)?$/;

function computeAuroraNewlineIndent(lineText, character, indentUnit) {
  const safeCharacter = Math.max(0, Math.min(character, lineText.length));
  const beforeText = lineText.slice(0, safeCharacter);
  const lineIndent = (lineText.match(/^\s*/) || [""])[0];

  if (BLOCK_HEADER_PATTERN.test(beforeText)) {
    return lineIndent + indentUnit;
  }

  return lineIndent;
}

module.exports = {
  BLOCK_HEADER_PATTERN,
  computeAuroraNewlineIndent
};
