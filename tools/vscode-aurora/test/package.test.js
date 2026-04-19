"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { computeAuroraNewlineIndent } = require("../src/indentation");

test("extension bundle contains built extension and language server files", () => {
  const extensionRoot = path.resolve(__dirname, "..");
  const distFiles = ["extension.js", "server.js"];

  for (const filename of distFiles) {
    const fullPath = path.join(extensionRoot, "dist", filename);
    assert.equal(fs.existsSync(fullPath), true, `${filename} should exist in extension/dist`);
  }
});

test("language configuration indents block headers on enter without blank-line dedent", () => {
  const extensionRoot = path.resolve(__dirname, "..");
  const configurationPath = path.join(extensionRoot, "language-configuration.json");
  const configuration = JSON.parse(fs.readFileSync(configurationPath, "utf8"));

  assert.match(
    configuration.indentationRules.increaseIndentPattern,
    /class\|enum\|trait\|def\|if\|elif\|else\|while\|for\|match\|case\|with\|impl/
  );
  assert.equal(
    Object.prototype.hasOwnProperty.call(configuration.indentationRules, "decreaseIndentPattern"),
    false,
    "blank lines should not be treated as a dedent signal"
  );
  assert.ok(Array.isArray(configuration.onEnterRules), "onEnterRules should be configured");
  assert.ok(configuration.onEnterRules.length > 0, "at least one onEnterRules entry is required");
  assert.equal(configuration.onEnterRules[0].action.indent, "indent");
});

test("syntax grammar treats boolean operators as Aurora keywords", () => {
  const extensionRoot = path.resolve(__dirname, "..");
  const grammarPath = path.join(extensionRoot, "syntaxes", "aurora.tmLanguage.json");
  const grammar = JSON.parse(fs.readFileSync(grammarPath, "utf8"));
  const keywordRule = grammar.repository.keywords.patterns.find(
    (pattern) => pattern.name === "keyword.control.aurora"
  );

  assert.ok(keywordRule);
  assert.match(keywordRule.match, /and\|or\|not/);
  assert.match(keywordRule.match, /pass/);
});

test("Aurora newline indentation inherits the current block indent", () => {
  assert.equal(computeAuroraNewlineIndent("def main():", "def main():".length, "    "), "    ");
  assert.equal(computeAuroraNewlineIndent("    total = 1", "    total = 1".length, "    "), "    ");
  assert.equal(computeAuroraNewlineIndent("        ", 8, "    "), "        ");
  assert.equal(computeAuroraNewlineIndent("    if score < 10:", "    if score < 10:".length, "    "), "        ");
  assert.equal(computeAuroraNewlineIndent("print(1)", "print(1)".length, "    "), "");
});
