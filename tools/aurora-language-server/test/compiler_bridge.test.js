"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const {
  analyzeWithCompiler,
  completeWithCompiler,
  setWorkspaceRoots
} = require("../src/compiler_bridge");

const repoRoot = path.join(__dirname, "../../..");
const pointPath = path.join(repoRoot, "examples/point.au");
const pointUri = `file://${pointPath}`;
const pointSource = fs.readFileSync(pointPath, "utf8");
const traitPath = path.join(repoRoot, "examples/traits/greeter.au");
const traitUri = `file://${traitPath}`;
const traitSource = fs.readFileSync(traitPath, "utf8");
const modulesPath = path.join(repoRoot, "examples/modules/simple_import.au");
const modulesUri = `file://${modulesPath}`;
const modulesSource = fs.readFileSync(modulesPath, "utf8");
const namespaceTypesPath = path.join(repoRoot, "examples/modules/namespace_import_types.au");
const namespaceTypesUri = `file://${namespaceTypesPath}`;
const namespaceTypesSource = fs.readFileSync(namespaceTypesPath, "utf8");

test("compiler bridge returns machine-readable analysis for a real example", async () => {
  setWorkspaceRoots([repoRoot]);
  const analysis = await analyzeWithCompiler(pointUri, pointSource);

  assert.ok(analysis);
  assert.equal(analysis.diagnostics.length, 0);
  assert.ok(Array.isArray(analysis.symbols));
  assert.ok(Array.isArray(analysis.occurrences));
  assert.ok(analysis.symbols.some((symbol) => symbol.name === "Point"));
});

test("compiler bridge returns member completions from the compiler", async () => {
  setWorkspaceRoots([repoRoot]);
  const lineIndex = pointSource.split("\n").findIndex((line) => line.includes("a.x"));
  const lineText = pointSource.split("\n")[lineIndex];
  const character = lineText.indexOf(".") + 1;

  const completions = await completeWithCompiler(pointUri, pointSource, lineIndex, character, ".");

  assert.ok(completions);
  const names = completions.map((item) => item.name).sort();
  assert.deepEqual(names, ["x", "y"]);
});

test("compiler bridge understands trait symbols and trait method completions", async () => {
  setWorkspaceRoots([repoRoot]);
  const analysis = await analyzeWithCompiler(traitUri, traitSource);

  assert.ok(analysis);
  assert.equal(analysis.diagnostics.length, 0);
  assert.ok(analysis.symbols.some((symbol) => symbol.kind === "trait" && symbol.name === "Greeter"));

  const lineIndex = traitSource.split("\n").findIndex((line) => line.includes("value.greet()"));
  const lineText = traitSource.split("\n")[lineIndex];
  const character = lineText.indexOf(".") + 1;

  const completions = await completeWithCompiler(
    traitUri,
    traitSource,
    lineIndex,
    character,
    "."
  );
  assert.ok(completions);
  assert.ok(completions.some((item) => item.name === "greet"));
});

test("compiler bridge resolves local module imports for analysis and completions", async () => {
  setWorkspaceRoots([repoRoot]);
  const analysis = await analyzeWithCompiler(modulesUri, modulesSource);

  assert.ok(analysis);
  assert.equal(analysis.diagnostics.length, 0);
  assert.ok(
    analysis.occurrences.some((occurrence) => occurrence.hover.includes("function double"))
  );

  const lineIndex = modulesSource
    .split("\n")
    .findIndex((line) => line.includes("helpers.math.double"));
  const lineText = modulesSource.split("\n")[lineIndex];
  const character = lineText.indexOf(".double") + 1;

  const completions = await completeWithCompiler(
    modulesUri,
    modulesSource,
    lineIndex,
    character,
    "."
  );

  assert.ok(completions);
  assert.ok(completions.some((item) => item.name === "double"));
});

test("compiler bridge preserves definitions for namespace-imported symbols", async () => {
  setWorkspaceRoots([repoRoot]);
  const analysis = await analyzeWithCompiler(namespaceTypesUri, namespaceTypesSource);

  assert.ok(analysis);
  assert.equal(analysis.diagnostics.length, 0);
  assert.ok(
    analysis.occurrences.some(
      (occurrence) =>
        occurrence.hover.includes("module pkg.types") && occurrence.definition !== null
    )
  );
  assert.ok(
    analysis.occurrences.some(
      (occurrence) =>
        occurrence.hover.includes("class Counter") && occurrence.definition !== null
    )
  );
  assert.ok(
    analysis.occurrences.some(
      (occurrence) => occurrence.hover.includes("enum Status") && occurrence.definition !== null
    )
  );
});

test("compiler bridge records enum variant occurrences in match patterns", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-match-patterns-"));
  try {
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const source = [
      "enum Status:",
      "    Ready",
      "    Busy",
      "",
      "def render(status: Status) -> int32:",
      "    match status:",
      "        case Status.Ready:",
      "            return 1",
      "        case Status.Busy:",
      "            return 0"
    ].join("\n");

    setWorkspaceRoots([repoRoot, tempRoot]);
    const analysis = await analyzeWithCompiler(mainUri, source);
    assert.ok(analysis);
    assert.equal(analysis.diagnostics.length, 0);
    assert.ok(
      analysis.occurrences.some(
        (occurrence) =>
          occurrence.line === 6 &&
          occurrence.hover.includes("variant Ready") &&
          occurrence.definition !== null
      )
    );
    assert.ok(
      analysis.occurrences.some(
        (occurrence) =>
          occurrence.line === 8 &&
          occurrence.hover.includes("variant Busy") &&
          occurrence.definition !== null
      )
    );
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge includes imported trait methods in completions", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-imported-trait-"));
  try {
    fs.mkdirSync(path.join(tempRoot, "pkg"));
    fs.writeFileSync(
      path.join(tempRoot, "pkg/named.au"),
      "public trait Named:\n    def name(borrow self) -> String\n"
    );
    fs.writeFileSync(
      path.join(tempRoot, "pkg/user.au"),
      "from pkg.named import Named\n\npublic class User:\n    public label: String\n\nimpl Named for User:\n    def name(borrow self) -> String:\n        return self.label\n"
    );
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const source =
      "from pkg.user import User\n\ndef main() -> int32:\n    user = User(label=\"Ada\")\n    user.\n    return 0\n";

    setWorkspaceRoots([repoRoot, tempRoot]);
    const analysis = await analyzeWithCompiler(mainUri, source);
    assert.ok(analysis);
    assert.ok(Array.isArray(analysis.diagnostics));

    const lineIndex = source.split("\n").findIndex((line) => line.includes("user."));
    const lineText = source.split("\n")[lineIndex];
    const character = lineText.indexOf(".") + 1;
    const completions = await completeWithCompiler(mainUri, source, lineIndex, character, ".");

    assert.ok(completions);
    assert.ok(completions.some((item) => item.name === "label"));
    assert.ok(completions.some((item) => item.name === "name"));
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge recovers completions and symbols for dangling-dot EOF buffers", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-dangling-dot-"));
  try {
    const mainPath = path.join(tempRoot, "counter.au");
    const mainUri = `file://${mainPath}`;
    const source =
      "class Counter:\n    value: int32\n\ndef main() -> int32:\n    counter = Counter(value=1)\n    counter.";

    setWorkspaceRoots([repoRoot, tempRoot]);
    const analysis = await analyzeWithCompiler(mainUri, source);
    assert.ok(analysis);
    assert.ok(Array.isArray(analysis.symbols));
    assert.ok(analysis.symbols.some((symbol) => symbol.name === "Counter"));

    const lineIndex = source.split("\n").findIndex((line) => line.includes("counter."));
    const lineText = source.split("\n")[lineIndex];
    const character = lineText.indexOf(".") + 1;
    const completions = await completeWithCompiler(mainUri, source, lineIndex, character, ".");

    assert.ok(completions);
    assert.ok(completions.some((item) => item.name === "value"));
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge recovers imported completions and symbols when a buffer contains multiple dangling dots", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-multi-dangling-"));
  try {
    fs.mkdirSync(path.join(tempRoot, "helpers"));
    fs.writeFileSync(
      path.join(tempRoot, "helpers/math.au"),
      "public def double(value: int32) -> int32:\n    return value * 2\n"
    );
    fs.writeFileSync(
      path.join(tempRoot, "helpers/counter.au"),
      "public class Counter:\n    public value: int32\n"
    );
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const source = [
      "import helpers.math",
      "from helpers.counter import Counter",
      "",
      "def main() -> int32:",
      "    counter = Counter(value=1)",
      "    print(helpers.math.",
      "    print(counter.",
      "    return 0"
    ].join("\n");

    setWorkspaceRoots([repoRoot, tempRoot]);
    const analysis = await analyzeWithCompiler(mainUri, source);
    assert.ok(analysis);
    assert.ok(Array.isArray(analysis.symbols));
    assert.ok(analysis.symbols.length > 0);
    assert.ok(Array.isArray(analysis.occurrences));
    assert.ok(analysis.occurrences.length > 0);

    const lineIndex = source.split("\n").findIndex((line) => line.includes("helpers.math."));
    const lineText = source.split("\n")[lineIndex];
    const character = lineText.lastIndexOf(".") + 1;
    const completions = await completeWithCompiler(mainUri, source, lineIndex, character, ".");

    assert.ok(completions);
    assert.ok(completions.some((item) => item.name === "double"));
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});
