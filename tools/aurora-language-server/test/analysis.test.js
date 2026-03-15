"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const { analyzeDocument, completionsForDocument, documentSymbols } = require("../src/analysis");

const pointSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/point.au"),
  "utf8"
);

test("analyzeDocument finds classes and functions", () => {
  const moduleInfo = analyzeDocument(pointSource);
  assert.ok(moduleInfo.classes.has("Point"));
  assert.ok(moduleInfo.functions.has("distance"));
  assert.ok(moduleInfo.functions.has("main"));
});

test("member completion suggests class fields after dot", () => {
  const lineIndex = pointSource.split("\n").findIndex((line) => line.includes("a.x"));
  const lineText = pointSource.split("\n")[lineIndex];
  const character = lineText.indexOf(".") + 1;
  const items = completionsForDocument(pointSource, lineIndex, character, ".");
  const names = items.map((item) => item.name).sort();
  assert.deepEqual(names, ["x", "y"]);
});

test("top-level completion includes keywords and declarations", () => {
  const items = completionsForDocument(pointSource, 0, 0, null);
  const names = new Set(items.map((item) => item.name));
  assert.ok(names.has("class"));
  assert.ok(names.has("Point"));
  assert.ok(names.has("distance"));
});

test("document symbols include class and function entries", () => {
  const symbols = documentSymbols(pointSource);
  assert.ok(symbols.some((symbol) => symbol.kind === "class" && symbol.name === "Point"));
  assert.ok(symbols.some((symbol) => symbol.kind === "function" && symbol.name === "main"));
});
