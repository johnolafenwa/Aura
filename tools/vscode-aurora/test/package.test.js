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

test("extension package includes the assertion-aware Aurora grammar", () => {
  const extensionRoot = path.resolve(__dirname, "..");
  const manifest = JSON.parse(
    fs.readFileSync(path.join(extensionRoot, "package.json"), "utf8")
  );
  const grammarContribution = manifest.contributes.grammars.find(
    (grammar) => grammar.language === "aurora"
  );

  assert.ok(manifest.files.includes("syntaxes/**"));
  assert.equal(grammarContribution?.path, "./syntaxes/aurora.tmLanguage.json");
  const packagedGrammar = fs.readFileSync(
    path.join(extensionRoot, grammarContribution.path),
    "utf8"
  );
  assert.match(packagedGrammar, /assert/);
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
  assert.match(keywordRule.match, /assert/);
});

test("syntax grammar treats own as an Aurora storage modifier", () => {
  const extensionRoot = path.resolve(__dirname, "..");
  const grammarPath = path.join(extensionRoot, "syntaxes", "aurora.tmLanguage.json");
  const grammar = JSON.parse(fs.readFileSync(grammarPath, "utf8"));
  const modifierRule = grammar.repository.keywords.patterns.find(
    (pattern) => pattern.name === "storage.modifier.aurora"
  );

  assert.ok(modifierRule);
  assert.match(modifierRule.match, /borrow\|own/);
});

test("syntax grammar distinguishes ordinary quotes and nests strings in f-string interpolation", () => {
  const extensionRoot = path.resolve(__dirname, "..");
  const grammarPath = path.join(extensionRoot, "syntaxes", "aurora.tmLanguage.json");
  const grammar = JSON.parse(fs.readFileSync(grammarPath, "utf8"));
  const stringRules = grammar.repository.strings.patterns;
  const fStringRule = stringRules.find(
    (pattern) => pattern.name === "string.interpolated.double.aurora"
  );
  const doubleRule = stringRules.find(
    (pattern) => pattern.name === "string.quoted.double.aurora"
  );
  const singleRule = stringRules.find(
    (pattern) => pattern.name === "string.quoted.single.aurora"
  );

  assert.ok(fStringRule);
  assert.equal(fStringRule.begin, 'f"');
  assert.ok(doubleRule);
  assert.equal(doubleRule.begin, '"');
  assert.ok(singleRule);
  assert.equal(singleRule.begin, "'");

  const interpolation = fStringRule.patterns.find(
    (pattern) => pattern.name === "meta.interpolation.aurora"
  );
  assert.ok(interpolation);
  assert.ok(
    interpolation.patterns.some((pattern) => pattern.include === "#strings"),
    "f-string interpolations should recognize nested ordinary strings"
  );

  const configurationPath = path.join(extensionRoot, "language-configuration.json");
  const configuration = JSON.parse(fs.readFileSync(configurationPath, "utf8"));
  assert.ok(configuration.autoClosingPairs.some(([open, close]) => open === "'" && close === "'"));
  assert.ok(configuration.autoClosingPairs.some(([open, close]) => open === '"' && close === '"'));
  assert.ok(configuration.surroundingPairs.some(([open, close]) => open === "'" && close === "'"));
  assert.ok(configuration.surroundingPairs.some(([open, close]) => open === '"' && close === '"'));
});

test("syntax grammar treats floor-division operators as single tokens", () => {
  const extensionRoot = path.resolve(__dirname, "..");
  const grammarPath = path.join(extensionRoot, "syntaxes", "aurora.tmLanguage.json");
  const grammar = JSON.parse(fs.readFileSync(grammarPath, "utf8"));
  const operatorRule = grammar.repository.operators.patterns.find(
    (pattern) => pattern.name === "keyword.operator.aurora"
  );

  assert.ok(operatorRule);
  const operatorPattern = new RegExp(operatorRule.match);
  assert.equal("//=".match(operatorPattern)?.[0], "//=", "//= should be one operator token");
  assert.equal("//".match(operatorPattern)?.[0], "//", "// should be one operator token");
  assert.equal("%=".match(operatorPattern)?.[0], "%=", "%= should be one operator token");
});

test("syntax grammar tracks maintained builtin types", () => {
  const extensionRoot = path.resolve(__dirname, "..");
  const grammarPath = path.join(extensionRoot, "syntaxes", "aurora.tmLanguage.json");
  const grammar = JSON.parse(fs.readFileSync(grammarPath, "utf8"));
  const typeRule = grammar.repository.types.patterns.find(
    (pattern) => pattern.name === "support.type.primitive.aurora"
  );

  assert.ok(typeRule);
  const typePattern = new RegExp(typeRule.match);
  for (const typeName of [
    "int",
    "int32",
    "int64",
    "Queue",
    "QueueReceive",
    "TaskResult",
    "WaitAny",
    "WaitAll",
    "Map",
    "MapEntry",
    "Set",
    "process.Child",
    "fs.File",
    "net.TcpStream"
  ]) {
    assert.equal(typePattern.test(typeName), true, `${typeName} should be highlighted as a type`);
  }
  assert.doesNotMatch(typeRule.match, /Channel/);
});

test("Aurora newline indentation inherits the current block indent", () => {
  assert.equal(computeAuroraNewlineIndent("def main():", "def main():".length, "    "), "    ");
  assert.equal(computeAuroraNewlineIndent("    total = 1", "    total = 1".length, "    "), "    ");
  assert.equal(computeAuroraNewlineIndent("        ", 8, "    "), "        ");
  assert.equal(computeAuroraNewlineIndent("    if score < 10:", "    if score < 10:".length, "    "), "        ");
  assert.equal(computeAuroraNewlineIndent("print(1)", "print(1)".length, "    "), "");
});

test("Aurora newline indentation handles source delimiters", () => {
  for (const line of [
    "    total = add(",
    "    values = [",
    "    mapping = {"
  ]) {
    assert.equal(computeAuroraNewlineIndent(line, line.length, "    "), "        ", line);
  }

  const nested = "    value = ([{";
  assert.equal(
    computeAuroraNewlineIndent(nested, nested.length, "    "),
    "        ",
    "nested delimiters add one continuation level, not one level per delimiter"
  );

  for (const line of [
    "    total = add(value)",
    "    values = [1, 2]",
    "    mapping = {1: 2}",
    "    value = ([{}])"
  ]) {
    assert.equal(computeAuroraNewlineIndent(line, line.length, "    "), "    ", line);
  }

  const textAfterCursor = "    value = (later)";
  assert.equal(
    computeAuroraNewlineIndent(textAfterCursor, "    value = (".length, "    "),
    "        ",
    "only text before the cursor determines the inserted newline indentation"
  );
});

test("Aurora newline indentation ignores delimiters in strings, f-strings, and comments", () => {
  for (const line of [
    '    text = "("',
    "    text = ']'",
    '    text = "escaped \\"(\\""',
    '    text = f"("',
    '    text = f"{value[0]}"',
    '    text = f"{echo("(")}"',
    "    value = call() # ([{",
    '    text = "# (" # ['
  ]) {
    assert.equal(computeAuroraNewlineIndent(line, line.length, "    "), "    ", line);
  }

  const blockWithStringDelimiter = '    if label == "(":';
  assert.equal(
    computeAuroraNewlineIndent(
      blockWithStringDelimiter,
      blockWithStringDelimiter.length,
      "    "
    ),
    "        ",
    "block headers retain their single indentation level"
  );
});

test("Aurora newline indentation recognizes multiline block headers", () => {
  assert.equal(
    computeAuroraNewlineIndent(
      "    ) -> int64:",
      "    ) -> int64:".length,
      "    ",
      ["def total(", "    left: int64,", "    right: int64"]
    ),
    "    "
  );
  assert.equal(
    computeAuroraNewlineIndent(
      "    ):",
      "    ):".length,
      "    ",
      ["    if (", "        ready"]
    ),
    "        "
  );
  assert.equal(
    computeAuroraNewlineIndent(
      "            )",
      "            )".length,
      "    ",
      ["    value = call(", "        1"]
    ),
    "    ",
    "closing a continued expression returns to the logical line's base indent"
  );
});
