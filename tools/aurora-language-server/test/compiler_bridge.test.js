"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const childProcess = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const {
  analyzeWithCompiler,
  binaryName,
  CompilerService,
  completeWithCompiler,
  compilerDefinitionAtPosition,
  compilerDefinitionToLspLocation,
  compilerDiagnosticsToLsp,
  compilerHoverAtPosition,
  compilerSymbolsToLsp,
  findOccurrence,
  disposeCompilerService,
  resolveCompilerCommand,
  runCommand,
  setWorkspaceRoots
  ,
  uriToPath
} = require("../src/compiler_bridge");

test.after(() => disposeCompilerService());

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

test("compiler bridge helper conversions cover diagnostics, symbols, and definition ranges", () => {
  assert.deepEqual(compilerDiagnosticsToLsp({}), []);

  const diagnostics = compilerDiagnosticsToLsp(
    {
      diagnostics: [
        {
          code: "AU2001",
          severity: 1,
          line: 2,
          start_character: 4,
          end_character: 9,
          message: "unknown name",
          secondary_spans: [
            {
              line: 0,
              start_character: 4,
              end_character: 9,
              label: "declared here"
            }
          ],
          notes: ["names are lexically scoped"],
          help: ["declare the name before using it"],
          edits: []
        }
      ]
    },
    "file:///workspace/main.au"
  );
  assert.deepEqual(diagnostics, [
    {
      severity: 1,
      range: {
        start: { line: 2, character: 4 },
        end: { line: 2, character: 9 }
      },
      message: "unknown name",
      source: "aurora-compiler",
      code: "AU2001",
      relatedInformation: [
        {
          location: {
            uri: "file:///workspace/main.au",
            range: {
              start: { line: 0, character: 4 },
              end: { line: 0, character: 9 }
            }
          },
          message: "declared here"
        }
      ],
      data: {
        notes: ["names are lexically scoped"],
        help: ["declare the name before using it"],
        edits: []
      }
    }
  ]);

  assert.deepEqual(compilerSymbolsToLsp({}), []);
  const symbols = compilerSymbolsToLsp({
    symbols: [
      {
        name: "Point",
        kind: "class",
        detail: "class Point",
        line: 0,
        start_character: 0,
        end_character: 5,
        children: [
          {
            name: "x",
            kind: "field",
            detail: "x: int32",
            line: 1,
            start_character: 4,
            end_character: 5,
            children: []
          }
        ]
      },
      {
        name: "distance",
        kind: "function",
        detail: "function distance",
        line: 2,
        start_character: 0,
        end_character: 8,
        children: []
      },
      {
        name: "greet",
        kind: "method",
        detail: "method greet",
        line: 4,
        start_character: 4,
        end_character: 9,
        children: []
      },
      {
        name: "Status",
        kind: "enum",
        detail: "enum Status",
        line: 6,
        start_character: 0,
        end_character: 6,
        children: [
          {
            name: "Ready",
            kind: "variant",
            detail: "variant Ready",
            line: 7,
            start_character: 4,
            end_character: 9,
            children: []
          }
        ]
      },
      {
        name: "Greeter",
        kind: "trait",
        detail: "trait Greeter",
        line: 8,
        start_character: 0,
        end_character: 7,
        children: []
      },
      {
        name: "mystery",
        kind: "unknown",
        detail: undefined,
        line: 3,
        start_character: undefined,
        end_character: undefined,
        children: undefined
      }
    ]
  });
  assert.equal(symbols[0].kind, 5);
  assert.equal(symbols[0].children[0].kind, 8);
  assert.equal(symbols[1].kind, 12);
  assert.equal(symbols[2].kind, 6);
  assert.equal(symbols[3].kind, 10);
  assert.equal(symbols[3].children[0].kind, 22);
  assert.equal(symbols[4].kind, 11);
  assert.equal(symbols[5].kind, 13);

  assert.equal(
    findOccurrence(
      {
        occurrences: [{ line: 1, start_character: 2, end_character: 5, hover: "hover" }]
      },
      1,
      3
    ).hover,
    "hover"
  );
  assert.equal(findOccurrence({ occurrences: [] }, 0, 0), null);
  assert.equal(findOccurrence(null, 0, 0), null);

  assert.equal(compilerDefinitionToLspLocation("file:///workspace/main.au", null), null);
  assert.deepEqual(
    compilerDefinitionToLspLocation("file:///workspace/main.au", {
      file_path: null,
      line: 2,
      start_character: 1,
      end_character: 4
    }),
    {
      uri: "file:///workspace/main.au",
      range: {
        start: { line: 2, character: 1 },
        end: { line: 2, character: 4 }
      }
    }
  );

  assert.deepEqual(
    compilerHoverAtPosition(
      {
        occurrences: [
          {
            line: 4,
            start_character: 2,
            end_character: 6,
            hover: "```aurora\nfunction greet() -> String\n```"
          }
        ]
      },
      4,
      3
    ),
    {
      value: "```aurora\nfunction greet() -> String\n```",
      range: {
        start: { line: 4, character: 2 },
        end: { line: 4, character: 6 }
      }
    }
  );
  assert.equal(compilerHoverAtPosition({ occurrences: [] }, 1, 1), null);

  assert.deepEqual(
    compilerDefinitionAtPosition(
      "file:///workspace/main.au",
      {
        occurrences: [
          {
            line: 1,
            start_character: 0,
            end_character: 3,
            hover: "hover",
            definition: {
              file_path: path.join(repoRoot, "examples/modules/pkg/types.au"),
              line: 7,
              start_character: 4,
              end_character: 9
            }
          }
        ]
      },
      1,
      1
    ),
    {
      uri: `file://${path.join(repoRoot, "examples/modules/pkg/types.au")}`,
      range: {
        start: { line: 7, character: 4 },
        end: { line: 7, character: 9 }
      }
    }
  );
  assert.equal(
    compilerDefinitionAtPosition(
      "file:///workspace/main.au",
      {
        occurrences: [
          {
            line: 1,
            start_character: 0,
            end_character: 3,
            hover: "hover",
            definition: null
          }
        ]
      },
      1,
      1
    ),
    null
  );
  assert.equal(compilerDefinitionAtPosition("file:///workspace/main.au", { occurrences: [] }, 9, 9), null);
});

test("compiler bridge keeps diagnostic metadata optional across compiler schema versions", () => {
  const diagnostic = {
    code: "AU3001",
    severity: 1,
    line: 1,
    start_character: 4,
    end_character: 5,
    message: "use of moved value"
  };
  const convert = (metadata) =>
    compilerDiagnosticsToLsp({ diagnostics: [{ ...diagnostic, ...metadata }] })[0];

  assert.equal(convert({}).data, undefined);
  assert.deepEqual(convert({ notes: ["one owner"] }).data, {
    notes: ["one owner"],
    help: [],
    edits: []
  });
  assert.deepEqual(convert({ help: ["borrow or clone"] }).data, {
    notes: [],
    help: ["borrow or clone"],
    edits: []
  });
  assert.deepEqual(convert({ edits: [{ replacement: ".clone()" }] }).data, {
    notes: [],
    help: [],
    edits: [{ replacement: ".clone()" }]
  });
});

test("compiler bridge resolves compiler commands across env, cargo, binaries, and fallback", () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-bridge-command-"));
  const originalEnvPath = process.env.AURORA_LSP_AURA_PATH;
  try {
    const fakeAura = path.join(tempRoot, binaryName());
    fs.writeFileSync(fakeAura, "");
    process.env.AURORA_LSP_AURA_PATH = fakeAura;
    setWorkspaceRoots([]);
    assert.deepEqual(resolveCompilerCommand(), { cmd: fakeAura, args: [], cwd: undefined });

    delete process.env.AURORA_LSP_AURA_PATH;
    const cargoRoot = path.join(tempRoot, "cargo-root");
    fs.mkdirSync(path.join(cargoRoot, "crates", "aura"), { recursive: true });
    fs.writeFileSync(path.join(cargoRoot, "Cargo.toml"), "[workspace]\n");
    fs.writeFileSync(path.join(cargoRoot, "crates", "aura", "Cargo.toml"), "[package]\nname=\"aura\"\nversion=\"0.1.0\"\n");
    setWorkspaceRoots([cargoRoot]);
    assert.deepEqual(resolveCompilerCommand(), {
      cmd: "cargo",
      args: ["run", "-q", "-p", "aura", "--"],
      cwd: cargoRoot
    });

    const binaryRoot = path.join(tempRoot, "binary-root");
    fs.mkdirSync(path.join(binaryRoot, "target", "debug"), { recursive: true });
    const debugBinary = path.join(binaryRoot, "target", "debug", binaryName());
    fs.writeFileSync(debugBinary, "");
    setWorkspaceRoots([binaryRoot]);
    assert.deepEqual(resolveCompilerCommand(), {
      cmd: debugBinary,
      args: [],
      cwd: binaryRoot
    });

    fs.rmSync(debugBinary, { force: true });
    fs.mkdirSync(path.join(binaryRoot, "target", "release"), { recursive: true });
    const releaseBinary = path.join(binaryRoot, "target", "release", binaryName());
    fs.writeFileSync(releaseBinary, "");
    assert.deepEqual(resolveCompilerCommand(), {
      cmd: releaseBinary,
      args: [],
      cwd: binaryRoot
    });

    setWorkspaceRoots([tempRoot]);
    assert.deepEqual(resolveCompilerCommand(), {
      cmd: "aura",
      args: [],
      cwd: tempRoot
    });

    setWorkspaceRoots(null);
    assert.deepEqual(resolveCompilerCommand(), {
      cmd: "aura",
      args: [],
      cwd: undefined
    });
  } finally {
    if (originalEnvPath === undefined) {
      delete process.env.AURORA_LSP_AURA_PATH;
    } else {
      process.env.AURORA_LSP_AURA_PATH = originalEnvPath;
    }
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge uri and command helpers handle direct utility cases", async () => {
  const encodedPath = path.join(repoRoot, "examples/modules/simple_import.au").replace(/ /g, "%20");
  assert.equal(uriToPath(`file://${encodedPath}`), path.join(repoRoot, "examples/modules/simple_import.au"));
  assert.equal(uriToPath("file:///C:/aurora/examples/main.au", "win32"), "C:\\aurora\\examples\\main.au");
  assert.equal(
    uriToPath("file://server/share/project/main.au", "win32"),
    "\\\\server\\share\\project\\main.au"
  );
  assert.equal(uriToPath("not-a-file-uri"), null);
  assert.equal(binaryName("win32"), "aura.exe");
  assert.equal(binaryName("linux"), "aura");

  const stdout = await runCommand(
    process.execPath,
    ["-e", "process.stdin.resume();let data='';process.stdin.on('data',chunk=>data+=chunk);process.stdin.on('end',()=>process.stdout.write(data.toUpperCase()))"],
    "aurora",
    repoRoot
  );
  assert.equal(stdout, "AURORA");

  await assert.rejects(
    runCommand(
      process.execPath,
      ["-e", "process.stderr.write('boom');process.exit(2)"],
      "",
      repoRoot
    ),
    /boom/
  );
});

test("compiler bridge returns null when compiler output fails or is not valid JSON", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-bridge-null-"));
  const mainPath = path.join(tempRoot, "main.au");
  const mainUri = `file://${mainPath}`;
  const originalEnvPath = process.env.AURORA_LSP_AURA_PATH;
  try {
    const failingScript = path.join(tempRoot, "fail.js");
    fs.writeFileSync(failingScript, "process.stderr.write('nope');process.exit(1);");
    process.env.AURORA_LSP_AURA_PATH = process.execPath;
    setWorkspaceRoots([]);
    assert.equal(
      await analyzeWithCompiler(mainUri, "def main() -> int32:\n    return 0\n"),
      null
    );
    assert.equal(
      await completeWithCompiler(mainUri, "def main() -> int32:\n    value.\n    return 0\n", 1, 10, "."),
      null
    );

    const invalidJsonScript = path.join(tempRoot, "invalid-json.js");
    fs.writeFileSync(
      invalidJsonScript,
      "process.stdin.once('data',()=>process.stdout.write('not json\\n'));"
    );
    process.env.AURORA_LSP_AURA_PATH = path.join(tempRoot, "aurora-invalid-json");
    fs.writeFileSync(
      process.env.AURORA_LSP_AURA_PATH,
      `#!/bin/sh\nexec "${process.execPath}" "${invalidJsonScript}" "$@"\n`
    );
    fs.chmodSync(process.env.AURORA_LSP_AURA_PATH, 0o755);
    assert.equal(
      await analyzeWithCompiler(mainUri, "def main() -> int32:\n    return 0\n"),
      null
    );
  } finally {
    if (originalEnvPath === undefined) {
      delete process.env.AURORA_LSP_AURA_PATH;
    } else {
      process.env.AURORA_LSP_AURA_PATH = originalEnvPath;
    }
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge reuses one persistent compiler process", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-bridge-persistent-"));
  const originalEnvPath = process.env.AURORA_LSP_AURA_PATH;
  try {
    const counterPath = path.join(tempRoot, "starts.txt");
    const fakeCompiler = path.join(tempRoot, "aurora-persistent");
    const script = path.join(tempRoot, "persistent.js");
    fs.writeFileSync(
      script,
      [
        "const fs = require('node:fs');",
        "const readline = require('node:readline');",
        `fs.appendFileSync(${JSON.stringify(counterPath)}, 'start\\n');`,
        "if (process.argv[2] !== 'lsp') process.exit(2);",
        "const lines = readline.createInterface({ input: process.stdin });",
        "lines.on('line', (line) => {",
        "  const request = JSON.parse(line);",
        "  const result = request.method === 'analyze'",
        "    ? { diagnostics: [], symbols: [], occurrences: [] }",
        "    : [{ name: 'len', kind: 'method', detail: 'len() -> intsize' }];",
        "  process.stdout.write(JSON.stringify({ id: request.id, result }) + '\\n');",
        "});"
      ].join("\n")
    );
    fs.writeFileSync(fakeCompiler, `#!/bin/sh\nexec "${process.execPath}" "${script}" "$@"\n`);
    fs.chmodSync(fakeCompiler, 0o755);
    process.env.AURORA_LSP_AURA_PATH = fakeCompiler;
    setWorkspaceRoots([]);

    const analysis = await analyzeWithCompiler("file:///workspace/main.au", "print(1)\n");
    const completions = await completeWithCompiler(
      "file:///workspace/main.au",
      "value.\n",
      0,
      6,
      null
    );
    assert.deepEqual(analysis.diagnostics, []);
    assert.equal(completions[0].name, "len");
    assert.equal(fs.readFileSync(counterPath, "utf8").trim().split("\n").length, 1);
  } finally {
    disposeCompilerService();
    if (originalEnvPath === undefined) {
      delete process.env.AURORA_LSP_AURA_PATH;
    } else {
      process.env.AURORA_LSP_AURA_PATH = originalEnvPath;
    }
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("persistent compiler service handles errors, cancellation, and closed requests", async () => {
  const script = [
    "const readline = require('node:readline');",
    "const lines = readline.createInterface({ input: process.stdin });",
    "lines.on('line', (line) => {",
    "  const request = JSON.parse(line);",
    "  if (request.method === 'error') {",
    "    process.stdout.write(JSON.stringify({ id: request.id, error: 'compiler boom' }) + '\\n');",
    "  }",
    "});"
  ].join("\n");
  const command = { cmd: process.execPath, args: ["-e", script], cwd: repoRoot };
  const service = new CompilerService(command);
  let responseDisposals = 0;
  const responseToken = {
    isCancellationRequested: false,
    onCancellationRequested() {
      return { dispose: () => responseDisposals++ };
    }
  };
  await assert.rejects(service.request("error", {}, responseToken), /compiler boom/);
  assert.equal(responseDisposals, 1);
  await assert.rejects(
    service.request("ignored", {}, { isCancellationRequested: true }),
    /cancelled/
  );

  let cancel;
  let cancellationDisposals = 0;
  const cancellationToken = {
    isCancellationRequested: false,
    onCancellationRequested(handler) {
      cancel = handler;
      return { dispose: () => cancellationDisposals++ };
    }
  };
  const pending = service.request("hang", {}, cancellationToken);
  await new Promise((resolve) => setImmediate(resolve));
  cancel();
  await assert.rejects(pending, /cancelled/);
  assert.equal(cancellationDisposals, 1);
  await assert.rejects(service.request("after-close", {}), /closed/);
  service.fail(new Error("already closed"));
  service.dispose();
});

test("persistent compiler service enforces timeout and response-size limits", async () => {
  const hanging = new CompilerService(
    {
      cmd: process.execPath,
      args: ["-e", "process.stdin.resume()"],
      cwd: repoRoot
    },
    { requestTimeoutMs: 10, responseLimitBytes: 1024 }
  );
  await assert.rejects(hanging.request("analyze", {}), /timed out after 10ms/);
  hanging.dispose();

  const oversized = new CompilerService(
    {
      cmd: process.execPath,
      args: ["-e", "process.stdin.once('data',()=>process.stdout.write('x'.repeat(32)))"],
      cwd: repoRoot
    },
    { requestTimeoutMs: 1000, responseLimitBytes: 16 }
  );
  await assert.rejects(oversized.request("analyze", {}), /exceeded 16 MiB/);
  oversized.dispose();
});

test("persistent compiler service reports spawn and empty-stderr exit failures", async () => {
  const missing = new CompilerService({
    cmd: path.join(os.tmpdir(), "definitely-missing-aurora-compiler"),
    args: [],
    cwd: repoRoot
  });
  await assert.rejects(missing.request("analyze", {}), /ENOENT/);
  missing.dispose();

  const exited = new CompilerService({
    cmd: process.execPath,
    args: ["-e", "process.exit(7)"],
    cwd: repoRoot
  });
  await assert.rejects(exited.request("analyze", {}), /status 7/);
  exited.dispose();
});

test("compiler bridge accepts non-file URIs when using the compiler subprocess helpers", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-bridge-memory-uri-"));
  const originalEnvPath = process.env.AURORA_LSP_AURA_PATH;

  try {
    const fakeCompiler = path.join(tempRoot, "aurora-fake");
    const fakeCompilerScript = path.join(tempRoot, "fake-compiler.js");
    fs.writeFileSync(
      fakeCompilerScript,
      [
        "const readline = require('node:readline');",
        "if (process.argv[2] !== 'lsp') process.exit(2);",
        "const lines = readline.createInterface({ input: process.stdin });",
        "lines.on('line', (line) => {",
        "  const request = JSON.parse(line);",
        "  const result = request.method === 'analyze'",
        "    ? { diagnostics: [], symbols: [], occurrences: [{ line: 0, start_character: 0, end_character: 3, hover: request.path, definition: null }] }",
        "    : [{ label: request.path }];",
        "  process.stdout.write(JSON.stringify({ id: request.id, result }) + '\\n');",
        "});"
      ].join("\n")
    );
    fs.writeFileSync(fakeCompiler, `#!/bin/sh\nexec "${process.execPath}" "${fakeCompilerScript}" "$@"\n`);
    fs.chmodSync(fakeCompiler, 0o755);

    process.env.AURORA_LSP_AURA_PATH = fakeCompiler;
    setWorkspaceRoots([]);

    const analysis = await analyzeWithCompiler("untitled:aurora-buffer", "def main():\n    pass\n");
    assert.equal(analysis.occurrences[0].hover, "untitled:aurora-buffer");

    const completions = await completeWithCompiler(
      "untitled:aurora-buffer",
      "def main():\n    value.\n",
      1,
      10,
      "."
    );
    assert.equal(completions[0].label, "untitled:aurora-buffer");
  } finally {
    if (originalEnvPath === undefined) {
      delete process.env.AURORA_LSP_AURA_PATH;
    } else {
      process.env.AURORA_LSP_AURA_PATH = originalEnvPath;
    }
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("runCommand reports exit status when stderr is empty", async () => {
  await assert.rejects(
    runCommand(process.execPath, ["-e", "process.exit(7)"], "", repoRoot),
    /status 7/
  );
});

test("compiler bridge returns machine-readable analysis for a real example", async () => {
  setWorkspaceRoots([repoRoot]);
  const analysis = await analyzeWithCompiler(pointUri, pointSource);

  assert.ok(analysis);
  assert.equal(analysis.diagnostics.length, 0);
  assert.ok(Array.isArray(analysis.symbols));
  assert.ok(Array.isArray(analysis.occurrences));
  assert.ok(analysis.symbols.some((symbol) => symbol.name === "Point"));
});

test("compiler bridge analyzes and completes inside continued delimiters", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-continuation-"));
  const source = [
    "def add(left: int32, right: int32) -> int32:",
    "    return left + right",
    "",
    "def main() -> int32:",
    "    base: int32 = 40",
    "    result = add(",
    "        base,",
    "        2",
    "    )",
    "    return result",
    ""
  ].join("\n");

  try {
    setWorkspaceRoots([repoRoot, tempRoot]);
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const analysis = await analyzeWithCompiler(mainUri, source);

    assert.ok(analysis);
    assert.deepEqual(analysis.diagnostics, []);

    const hover = compilerHoverAtPosition(analysis, 6, 9);
    assert.deepEqual(hover, {
      value: "```aurora\nbinding base: int32\n```",
      range: {
        start: { line: 6, character: 8 },
        end: { line: 6, character: 12 }
      }
    });

    const definition = compilerDefinitionAtPosition(mainUri, analysis, 6, 9);
    assert.equal(
      definition?.uri,
      `file://${path.join(fs.realpathSync(tempRoot), "main.au")}`
    );
    assert.deepEqual(definition?.range, {
      start: { line: 4, character: 4 },
      end: { line: 4, character: 8 }
    });

    const completions = await completeWithCompiler(mainUri, source, 6, 12, null);
    assert.ok(
      completions.some(
        (completion) => completion.name === "add" && completion.kind === "function"
      )
    );
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge recovers member completion when an earlier line owns the open delimiter", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-multiline-dangling-"));
  const source = [
    "def main() -> int32:",
    "    text = \"hello\"",
    "    print(",
    "        text."
  ].join("\n");

  try {
    setWorkspaceRoots([repoRoot, tempRoot]);
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const analysis = await analyzeWithCompiler(mainUri, source);

    assert.ok(analysis);
    assert.ok(analysis.symbols.length > 0);
    assert.ok(analysis.occurrences.length > 0);

    const completions = await completeWithCompiler(mainUri, source, 3, 13, ".");
    assert.ok(completions);
    assert.ok(completions.some((item) => item.name === "len"));
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge maps mismatched delimiter diagnostics to the opening delimiter", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-delimiter-error-"));
  const source = [
    "def main():",
    "    values = [",
    "        1,",
    "        2)",
    ""
  ].join("\n");

  try {
    setWorkspaceRoots([repoRoot, tempRoot]);
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const analysis = await analyzeWithCompiler(mainUri, source);

    assert.ok(analysis);
    assert.equal(analysis.diagnostics.length, 1);
    assert.equal(analysis.diagnostics[0].code, "AU1001");
    assert.match(
      analysis.diagnostics[0].message,
      /mismatched closing delimiter `\)`; expected `]`/
    );
    assert.deepEqual(analysis.diagnostics[0].secondary_spans, [
      {
        line: 1,
        start_character: 13,
        end_character: 14,
        label: "opening delimiter `[` is here"
      }
    ]);

    const [diagnostic] = compilerDiagnosticsToLsp(analysis, mainUri);
    assert.deepEqual(diagnostic.range, {
      start: { line: 3, character: 9 },
      end: { line: 3, character: 10 }
    });
    assert.deepEqual(diagnostic.relatedInformation, [
      {
        location: {
          uri: mainUri,
          range: {
            start: { line: 1, character: 13 },
            end: { line: 1, character: 14 }
          }
        },
        message: "opening delimiter `[` is here"
      }
    ]);
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge keeps unclosed-delimiter EOF ranges inside a document without a final newline", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-unclosed-delimiter-"));
  const source = ["def main():", "    print("].join("\n");

  try {
    setWorkspaceRoots([repoRoot, tempRoot]);
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const analysis = await analyzeWithCompiler(mainUri, source);

    assert.ok(analysis);
    assert.equal(analysis.diagnostics.length, 1);
    assert.equal(analysis.diagnostics[0].code, "AU1001");
    assert.match(analysis.diagnostics[0].message, /unclosed delimiter `\(`/);

    const [diagnostic] = compilerDiagnosticsToLsp(analysis, mainUri);
    assert.deepEqual(diagnostic.range, {
      start: { line: 1, character: 10 },
      end: { line: 1, character: 11 }
    });
    assert.equal(diagnostic.relatedInformation?.[0]?.location.uri, mainUri);
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge preserves assert operand occurrences and keyword completion", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-assert-"));
  const source = [
    "def main():",
    "    ready = true",
    "    message = \"ready assertion\"",
    "    assert ready, message",
    ""
  ].join("\n");

  try {
    setWorkspaceRoots([repoRoot, tempRoot]);
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const analysis = await analyzeWithCompiler(mainUri, source);

    assert.ok(analysis);
    assert.deepEqual(analysis.diagnostics, []);
    const conditionUse = analysis.occurrences.find(
      (occurrence) =>
        occurrence.line === 3 &&
        occurrence.start_character === 11 &&
        occurrence.end_character === 16
    );
    const messageUse = analysis.occurrences.find(
      (occurrence) =>
        occurrence.line === 3 &&
        occurrence.start_character === 18 &&
        occurrence.end_character === 25
    );
    assert.ok(conditionUse, "assert condition should expose its identifier use");
    assert.ok(messageUse, "assert message should expose its identifier use");
    assert.equal(conditionUse.hover, "```aurora\nbinding ready: bool\n```");
    assert.equal(messageUse.hover, "```aurora\nbinding message: String\n```");
    assert.equal(conditionUse.definition?.line, 1);
    assert.equal(messageUse.definition?.line, 2);

    const completions = await completeWithCompiler(mainUri, source, 3, 10, null);
    assert.ok(
      completions.some(
        (completion) => completion.name === "assert" && completion.kind === "keyword"
      ),
      "compiler completion should include the assert keyword"
    );
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge exposes invalid assert diagnostics at the keyword", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-invalid-assert-"));
  const source = ["def main():", "    assert 1", ""].join("\n");

  try {
    setWorkspaceRoots([repoRoot, tempRoot]);
    const mainUri = `file://${path.join(tempRoot, "main.au")}`;
    const analysis = await analyzeWithCompiler(mainUri, source);

    assert.ok(analysis);
    assert.equal(analysis.diagnostics.length, 1);
    const [diagnostic] = compilerDiagnosticsToLsp(analysis, mainUri);
    assert.equal(diagnostic.code, "AU2002");
    assert.equal(
      diagnostic.message,
      "`assert` condition must have type `bool`, found `int64`"
    );
    assert.equal(diagnostic.source, "aurora-compiler");
    assert.equal(diagnostic.severity, 1);
    assert.deepEqual(diagnostic.range, {
      start: { line: 1, character: 4 },
      end: { line: 1, character: 5 }
    });
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge preserves enumerate and zip loop operands", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-lockstep-"));
  const source = [
    "def report(hosts: Vec[String], ports: Vec[int32]):",
    "    for index, host in enumerate(hosts):",
    "        print(index)",
    "    for host, port in zip(hosts, ports):",
    "        print(port)",
    ""
  ].join("\n");

  try {
    setWorkspaceRoots([repoRoot, tempRoot]);
    const mainUri = `file://${path.join(tempRoot, "main.au")}`;
    const analysis = await analyzeWithCompiler(mainUri, source);

    assert.ok(analysis);
    assert.deepEqual(analysis.diagnostics, []);
    const hosts = analysis.occurrences.find(
      (candidate) => candidate.line === 1 && candidate.start_character === 33
    );
    assert.ok(hosts, "missing enumerate operand occurrence");
    assert.ok(hosts.hover.includes("param hosts: Vec[String]"));

    const invalid = await analyzeWithCompiler(
      mainUri,
      [
        "def main():",
        "    for index, value in enumerate(range(3)):",
        "        print(index)",
        ""
      ].join("\n")
    );
    assert.ok(invalid);
    assert.equal(invalid.diagnostics.length, 1);
    const [diagnostic] = compilerDiagnosticsToLsp(invalid, mainUri);
    assert.equal(diagnostic.code, "AU2002");
    assert.equal(
      diagnostic.message,
      "`enumerate` requires a `Vec[T]` or `Set[T]` iterable, found `Range`"
    );
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge preserves membership and comparison chain operands", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-membership-"));
  const source = [
    "def probe(ports: Vec[int32], port: int32, low: int32, high: int32) -> bool:",
    "    return port in ports and low <= port < high",
    ""
  ].join("\n");

  try {
    setWorkspaceRoots([repoRoot, tempRoot]);
    const mainUri = `file://${path.join(tempRoot, "main.au")}`;
    const analysis = await analyzeWithCompiler(mainUri, source);

    assert.ok(analysis);
    assert.deepEqual(analysis.diagnostics, []);
    for (const [start, end, hover] of [
      [11, 15, "param port: int32"],
      [19, 24, "param ports: Vec[int32]"],
      [29, 32, "param low: int32"],
      [36, 40, "param port: int32"],
      [43, 47, "param high: int32"]
    ]) {
      const occurrence = analysis.occurrences.find(
        (candidate) =>
          candidate.line === 1 &&
          candidate.start_character === start &&
          candidate.end_character === end
      );
      assert.ok(occurrence, `missing membership occurrence at ${start}-${end}`);
      assert.ok(occurrence.hover.includes(hover));
    }

    const invalid = await analyzeWithCompiler(
      mainUri,
      ["def main():", "    print(1 in 5)", ""].join("\n")
    );
    assert.ok(invalid);
    assert.equal(invalid.diagnostics.length, 1);
    const [diagnostic] = compilerDiagnosticsToLsp(invalid, mainUri);
    assert.equal(diagnostic.code, "AU2003");
    assert.equal(
      diagnostic.message,
      "`in` requires a `Vec[T]`, `Set[T]`, `Map[K, V]`, or `String` container, found `int64`"
    );
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge preserves conditional operands and bool diagnostics", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-conditional-"));
  const source = [
    "def choose(ready: bool, left: String, right: String) -> String:",
    "    return left.clone() if ready else right.clone()",
    ""
  ].join("\n");

  try {
    setWorkspaceRoots([repoRoot, tempRoot]);
    const mainUri = `file://${path.join(tempRoot, "main.au")}`;
    const analysis = await analyzeWithCompiler(mainUri, source);

    assert.ok(analysis);
    assert.deepEqual(analysis.diagnostics, []);
    for (const [start, end, hover] of [
      [11, 15, "param left: String"],
      [27, 32, "param ready: bool"],
      [38, 43, "param right: String"]
    ]) {
      const occurrence = analysis.occurrences.find(
        (candidate) =>
          candidate.line === 1 &&
          candidate.start_character === start &&
          candidate.end_character === end
      );
      assert.ok(occurrence, `missing conditional occurrence at ${start}-${end}`);
      assert.ok(occurrence.hover.includes(hover));
    }

    const invalid = await analyzeWithCompiler(
      mainUri,
      ["def main():", "    value = \"yes\" if 1 else \"no\"", ""].join("\n")
    );
    assert.ok(invalid);
    assert.equal(invalid.diagnostics.length, 1);
    const [diagnostic] = compilerDiagnosticsToLsp(invalid, mainUri);
    assert.equal(diagnostic.code, "AU2002");
    assert.equal(
      diagnostic.message,
      "conditional expression condition must have type `bool`, found `int64`"
    );
    assert.deepEqual(diagnostic.range, {
      start: { line: 1, character: 21 },
      end: { line: 1, character: 22 }
    });
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge preserves real ownership provenance, help, and safe edits", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-ownership-diag-"));
  const source = "def take(value: String) -> String:\n    return value\n";

  try {
    setWorkspaceRoots([repoRoot, tempRoot]);
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const analysis = await analyzeWithCompiler(mainUri, source);

    assert.ok(analysis);
    assert.equal(analysis.diagnostics.length, 1);
    const diagnostics = compilerDiagnosticsToLsp(analysis, mainUri);
    assert.equal(diagnostics[0].code, "AU3002");
    assert.deepEqual(diagnostics[0].range, {
      start: { line: 1, character: 11 },
      end: { line: 1, character: 12 }
    });
    assert.deepEqual(diagnostics[0].relatedInformation, [
      {
        location: {
          uri: mainUri,
          range: {
            start: { line: 0, character: 9 },
            end: { line: 0, character: 10 }
          }
        },
        message: "parameter `value` is borrowed here"
      }
    ]);
    assert.deepEqual(diagnostics[0].data.help, [
      "declare the parameter as `own String` when the function should consume it, or call `.clone()` to consume an independent copy"
    ]);
    assert.deepEqual(diagnostics[0].data.edits, [
      {
        line: 1,
        start_character: 16,
        end_character: 16,
        replacement: ".clone()",
        applicability: "machine-applicable"
      }
    ]);
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
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

test("compiler bridge preserves the integer true-division teaching diagnostic", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-int-division-"));
  const message =
    "integer `/` is not supported; use `//` for floor division, or call `.to_float()` on both operands for true division";
  const cases = [
    {
      name: "binary division",
      source: [
        "def main() -> int32:",
        "    left: int32 = 7",
        "    right: int32 = 2",
        "    result = left / right",
        "    print(result)",
        "    return 0",
        ""
      ].join("\n")
    },
    {
      name: "augmented division",
      source: [
        "def main() -> int32:",
        "    mut value: int32 = 7",
        "    value /= 2",
        "    return value",
        ""
      ].join("\n")
    }
  ];

  try {
    setWorkspaceRoots([repoRoot, tempRoot]);
    for (const entry of cases) {
      const mainPath = path.join(tempRoot, `${entry.name.replaceAll(" ", "-")}.au`);
      const analysis = await analyzeWithCompiler(`file://${mainPath}`, entry.source);

      assert.ok(analysis, `${entry.name} should return compiler analysis`);
      assert.equal(analysis.diagnostics.length, 1, entry.name);
      assert.equal(analysis.diagnostics[0].message, message, entry.name);
      assert.equal(
        compilerDiagnosticsToLsp(analysis)[0].message,
        message,
        `${entry.name} LSP conversion`
      );
    }
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge accepts the retired chained-comparison spelling", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-chained-comparison-"));
  const source = [
    "def main():",
    "    if 1 < 2 < 3:",
    "        pass",
    ""
  ].join("\n");

  try {
    setWorkspaceRoots([repoRoot, tempRoot]);
    const mainPath = path.join(tempRoot, "main.au");
    const analysis = await analyzeWithCompiler(`file://${mainPath}`, source);

    assert.ok(analysis);
    assert.deepEqual(analysis.diagnostics, []);

    const mismatched = await analyzeWithCompiler(
      `file://${mainPath}`,
      ["def main():", "    if 1 < 2 < true:", "        pass", ""].join("\n")
    );
    assert.ok(mismatched);
    assert.equal(mismatched.diagnostics.length, 1);
    const diagnostic = compilerDiagnosticsToLsp(mismatched)[0];
    assert.ok(
      diagnostic.message.includes("binary operator operands must match"),
      diagnostic.message
    );
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge preserves indexed non-copy ownership diagnostic codes", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-index-ownership-"));
  const cases = [
    {
      code: "AU3005",
      source: [
        "def main():",
        "    values: Vec[String] = [\"one\"]",
        "    value: String = values[0]",
        ""
      ].join("\n")
    },
    {
      code: "AU3006",
      source: [
        "def main():",
        "    mut values: Vec[String] = [\"one\"]",
        "    values[0] += \"two\"",
        ""
      ].join("\n")
    }
  ];

  try {
    setWorkspaceRoots([repoRoot, tempRoot]);
    for (const entry of cases) {
      const mainPath = path.join(tempRoot, `${entry.code}.au`);
      const mainUri = `file://${mainPath}`;
      const analysis = await analyzeWithCompiler(mainUri, entry.source);

      assert.ok(analysis, `${entry.code} should return compiler analysis`);
      assert.equal(analysis.diagnostics.length, 1, entry.code);
      assert.equal(analysis.diagnostics[0].code, entry.code);
      assert.equal(compilerDiagnosticsToLsp(analysis, mainUri)[0].code, entry.code);
    }
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge reports typed self with the receiver-forms diagnostic", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-typed-self-"));
  const source = [
    "class Counter:",
    "    def read(self: Counter) -> int32:",
    "        return 0",
    ""
  ].join("\n");
  const message =
    "`self: Type` is not a method receiver; use `self` or `borrow self` for shared access, `own self` to consume, or `borrow mut self` to mutate";

  try {
    setWorkspaceRoots([repoRoot, tempRoot]);
    const mainPath = path.join(tempRoot, "main.au");
    const analysis = await analyzeWithCompiler(`file://${mainPath}`, source);

    assert.ok(analysis);
    assert.equal(analysis.diagnostics.length, 1);
    assert.equal(analysis.diagnostics[0].message, message);
    assert.deepEqual(
      compilerDiagnosticsToLsp(analysis)[0].range,
      {
        start: { line: 1, character: 13 },
        end: { line: 1, character: 14 }
      }
    );
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge preserves canonical receiver contracts in hover and completion", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-receiver-contracts-"));
  const source = [
    "class Modes:",
    "    value: int32",
    "    def read(self) -> int32:",
    "        return self.value",
    "    def explicit(borrow self) -> int32:",
    "        return self.value",
    "    def take(own self) -> int32:",
    "        return self.value",
    "    def bump(borrow mut self):",
    "        self.value += 1",
    "",
    "def main() -> int32:",
    "    mut value = Modes(value=1)",
    "    print(value.read())",
    "    print(value.explicit())",
    "    value.bump()",
    "    print(Modes(value=3).take())",
    "    return 0",
    ""
  ].join("\n");

  try {
    setWorkspaceRoots([repoRoot, tempRoot]);
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const analysis = await analyzeWithCompiler(mainUri, source);

    assert.ok(analysis);
    assert.equal(analysis.diagnostics.length, 0);
    for (const signature of [
      "method read(self) -> int32",
      "method explicit(self) -> int32",
      "method take(own self) -> int32",
      "method bump(borrow mut self) -> None"
    ]) {
      assert.ok(
        analysis.occurrences.some((occurrence) => occurrence.hover.includes(signature)),
        `missing hover signature: ${signature}`
      );
    }

    const completionSource = source.replace("    return 0\n", "    value.\n    return 0\n");
    const lineIndex = completionSource.split("\n").findIndex((line) => line.trim() === "value.");
    const character = completionSource.split("\n")[lineIndex].indexOf(".") + 1;
    const completions = await completeWithCompiler(
      mainUri,
      completionSource,
      lineIndex,
      character,
      "."
    );
    const details = new Map(completions.map((item) => [item.name, item.detail]));

    assert.equal(details.get("read"), "read(self) -> int32");
    assert.equal(details.get("explicit"), "explicit(self) -> int32");
    assert.equal(details.get("take"), "take(own self) -> int32");
    assert.equal(details.get("bump"), "bump(borrow mut self) -> None");
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge preserves ordinary parameter ownership in hover and diagnostics", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-param-contracts-"));
  const source = [
    "def inspect(value: String):",
    "    print(value)",
    "def consume(value: own String):",
    "    print(value)",
    "def explicit(value: borrow String = \"fallback\"):",
    "    print(value)",
    "def mutate(value: borrow mut String):",
    "    pass",
    "def main():",
    "    mut text = \"aurora\"",
    "    inspect(text)",
    "    consume(text.clone())",
    "    explicit()",
    "    mutate(text)",
    ""
  ].join("\n");

  try {
    setWorkspaceRoots([repoRoot, tempRoot]);
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const analysis = await analyzeWithCompiler(mainUri, source);

    assert.ok(analysis);
    assert.equal(analysis.diagnostics.length, 0);
    for (const signature of [
      "function inspect(value: String) -> None",
      "function consume(value: own String) -> None",
      "function explicit(value: borrow String = ...) -> None",
      "function mutate(value: borrow mut String) -> None"
    ]) {
      assert.ok(
        analysis.occurrences.some((occurrence) => occurrence.hover.includes(signature)),
        `missing hover signature: ${signature}`
      );
    }

    const invalid = [
      "def lost(value: borrow mut String = \"fallback\"):",
      "    pass",
      ""
    ].join("\n");
    const invalidAnalysis = await analyzeWithCompiler(mainUri, invalid);
    assert.equal(invalidAnalysis.diagnostics.length, 1);
    assert.equal(
      invalidAnalysis.diagnostics[0].message,
      "`borrow mut` parameter `value` cannot have a default: the default creates a caller-invisible temporary, so mutations through it would be silently lost; require the caller to pass a value, or take the parameter as `own T` and return the result"
    );
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge completes to_float for every integer type", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-int-to-float-"));
  const integerTypes = [
    "int",
    "int8",
    "int16",
    "int32",
    "int64",
    "int128",
    "intsize",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "uint128",
    "uintsize"
  ];

  try {
    setWorkspaceRoots([repoRoot, tempRoot]);
    for (const type of integerTypes) {
      const mainPath = path.join(tempRoot, `${type}.au`);
      const mainUri = `file://${mainPath}`;
      const source = [
        "def main() -> int32:",
        `    value: ${type} = 1`,
        "    value.",
        "    return 0",
        ""
      ].join("\n");
      const line = 2;
      const character = source.split("\n")[line].indexOf(".") + 1;
      const completions = await completeWithCompiler(
        mainUri,
        source,
        line,
        character,
        "."
      );

      assert.ok(completions, `${type} should return compiler completions`);
      const toFloat = completions.find((item) => item.name === "to_float");
      assert.ok(toFloat, `${type} should complete to_float`);
      assert.equal(toFloat.kind, "method", type);
      assert.equal(toFloat.detail, "to_float() -> float64", type);
    }
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge exposes the complete Duration surface and operator precedence", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-duration-"));
  const source = [
    "trait FloorDiv[Rhs, Out]:",
    "    def floor_div(borrow self, rhs: Rhs) -> Out",
    "",
    "class Counter:",
    "    value: int64",
    "",
    "impl FloorDiv[Counter, Counter] for Counter:",
    "    def floor_div(borrow self, rhs: Counter) -> Counter:",
    "        return Counter(value=self.value + rhs.value)",
    "",
    "def inspect(value: int64, left: Duration, right: Duration) -> float64:",
    "    millis: Duration = Duration.ms(value)",
    "    seconds: Duration = Duration.seconds(value=value)",
    "    minutes: Duration = Duration.minutes(value)",
    "    added: Duration = left + right",
    "    subtracted: Duration = left - right",
    "    scaled_right: Duration = left * value",
    "    scaled_left: Duration = value * right",
    "    divided: Duration = left // value",
    "    numeric: int64 = value // 2",
    "    custom: Counter = Counter(value=1) // Counter(value=2)",
    "    equal: bool = left == right",
    "    unequal: bool = left != right",
    "    less: bool = left < right",
    "    less_equal: bool = left <= right",
    "    greater: bool = left > right",
    "    greater_equal: bool = left >= right",
    "    return millis.to_ms() + seconds.to_seconds() + minutes.to_ms()",
    "",
    "def main() -> int32:",
    "    return 0",
    ""
  ].join("\n");

  try {
    setWorkspaceRoots([repoRoot, tempRoot]);
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const analysis = await analyzeWithCompiler(mainUri, source);

    assert.ok(analysis);
    assert.deepEqual(analysis.diagnostics, []);
    for (const signature of [
      "type Duration",
      "ms(value: int64) -> Duration",
      "seconds(value: int64) -> Duration",
      "minutes(value: int64) -> Duration",
      "to_ms() -> float64",
      "to_seconds() -> float64"
    ]) {
      assert.ok(
        analysis.occurrences.some((occurrence) => occurrence.hover.includes(signature)),
        `missing Duration hover: ${signature}`
      );
    }

    const staticSource = source.replace(
      "    return 0\n",
      "    Duration.\n    return 0\n"
    );
    const staticLine = staticSource
      .split("\n")
      .findIndex((line) => line.trim() === "Duration.");
    const staticCharacter = staticSource.split("\n")[staticLine].indexOf(".") + 1;
    const staticCompletions = await completeWithCompiler(
      mainUri,
      staticSource,
      staticLine,
      staticCharacter,
      "."
    );
    const staticDetails = new Map(
      staticCompletions.map((item) => [item.name, item.detail])
    );
    assert.equal(staticDetails.get("ms"), "ms(value: int64) -> Duration");
    assert.equal(staticDetails.get("seconds"), "seconds(value: int64) -> Duration");
    assert.equal(staticDetails.get("minutes"), "minutes(value: int64) -> Duration");
    assert.equal(staticDetails.has("to_ms"), false);

    const instanceSource = source.replace(
      "    return millis.to_ms() + seconds.to_seconds() + minutes.to_ms()\n",
      "    left.\n    return millis.to_ms() + seconds.to_seconds() + minutes.to_ms()\n"
    );
    const instanceLine = instanceSource
      .split("\n")
      .findIndex((line) => line.trim() === "left.");
    const instanceCharacter = instanceSource.split("\n")[instanceLine].indexOf(".") + 1;
    const instanceCompletions = await completeWithCompiler(
      mainUri,
      instanceSource,
      instanceLine,
      instanceCharacter,
      "."
    );
    const instanceDetails = new Map(
      instanceCompletions.map((item) => [item.name, item.detail])
    );
    assert.equal(instanceDetails.get("to_ms"), "to_ms() -> float64");
    assert.equal(instanceDetails.get("to_seconds"), "to_seconds() -> float64");
    assert.equal(instanceDetails.has("seconds"), false);

    const mixedAnalysis = await analyzeWithCompiler(
      mainUri,
      "def invalid(duration: Duration):\n    value = duration / duration\n"
    );
    assert.equal(mixedAnalysis.diagnostics.length, 1);
    assert.equal(
      mixedAnalysis.diagnostics[0].message,
      "unsupported Duration operands: `Duration` and `Duration`; supported forms are `Duration + Duration`, `Duration - Duration`, `Duration * int64`, `int64 * Duration`, `Duration // int64`, and comparisons between two Duration values"
    );
    assert.equal(mixedAnalysis.diagnostics[0].code, "AU2003");
    const lspDiagnostic = compilerDiagnosticsToLsp(mixedAnalysis, mainUri)[0];
    assert.equal(lspDiagnostic.source, "aurora-compiler");
    assert.equal(lspDiagnostic.code, "AU2003");
    assert.equal(lspDiagnostic.message, mixedAnalysis.diagnostics[0].message);

    const constructorAnalysis = await analyzeWithCompiler(
      mainUri,
      "def invalid():\n    value = Duration.seconds(true)\n"
    );
    assert.equal(constructorAnalysis.diagnostics.length, 1);
    assert.equal(
      constructorAnalysis.diagnostics[0].message,
      "`Duration.seconds` expects `int64`, found `bool`"
    );
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
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
  assert.ok(completions.some((item) => item.name === "greet" && item.detail === "greet(self) -> String"));
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
  const typesPath = path.join(repoRoot, "examples/modules/pkg/types.au");

  assert.ok(analysis);
  assert.equal(analysis.diagnostics.length, 0);
  assert.ok(
    analysis.occurrences.some(
      (occurrence) =>
        occurrence.hover.includes("module pkg.types") &&
        occurrence.definition !== null &&
        occurrence.definition.file_path === typesPath
    )
  );
  assert.ok(
    analysis.occurrences.some(
      (occurrence) =>
        occurrence.hover.includes("class Counter") &&
        occurrence.definition !== null &&
        occurrence.definition.file_path === typesPath
    )
  );
  assert.ok(
    analysis.occurrences.some(
      (occurrence) =>
          occurrence.hover.includes("enum pkg.types.Status") &&
        occurrence.definition !== null &&
        occurrence.definition.file_path === typesPath
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

test("compiler bridge preserves complete owned enum payload signatures", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-enum-payloads-"));
  try {
    fs.mkdirSync(path.join(tempRoot, "pkg"));
    const eventsPath = path.join(tempRoot, "pkg/events.au");
    fs.writeFileSync(
      eventsPath,
      [
        "public enum Event:",
        "    Message(code: int32, body: String)",
        ""
      ].join("\n")
    );
    const canonicalEventsPath = fs.realpathSync(eventsPath);

    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const source = [
      "import pkg.events",
      "",
      "def inspect(event: own pkg.events.Event):",
      "    match event:",
      "        case pkg.events.Event.Message(code, body):",
      "            print(code)",
      "            print(body)",
      "",
      "def main():",
      "    event = pkg.events.Event.Message(code=7, body=\"hello\")",
      "    inspect(event)",
      ""
    ].join("\n");

    setWorkspaceRoots([repoRoot, tempRoot]);
    const analysis = await analyzeWithCompiler(mainUri, source);
    assert.ok(analysis);
    assert.equal(analysis.diagnostics.length, 0);
    const matchingVariantOccurrences = analysis.occurrences.filter(
      (occurrence) =>
        occurrence.hover.includes(
          "variant Message(code: own int32, body: own String) -> pkg.events.Event"
        ) &&
        occurrence.definition !== null &&
        occurrence.definition.file_path === canonicalEventsPath
    );
    assert.equal(
      matchingVariantOccurrences.length,
      2,
      JSON.stringify(analysis.occurrences, null, 2)
    );

    const completionSource = [
      "import pkg.events",
      "",
      "def main():",
      "    pkg.events.Event.",
      ""
    ].join("\n");
    const lineIndex = completionSource
      .split("\n")
      .findIndex((line) => line.trim() === "pkg.events.Event.");
    const character = completionSource.split("\n")[lineIndex].lastIndexOf(".") + 1;
    const completions = await completeWithCompiler(
      mainUri,
      completionSource,
      lineIndex,
      character,
      "."
    );
    const message = completions.find((completion) => completion.name === "Message");
    assert.ok(message);
    assert.equal(
      message.detail,
      "Message(code: own int32, body: own String) -> pkg.events.Event"
    );

    for (const [enumName, variantName, detail] of [
      ["WaitAny", "Ready", "Ready(own int32, own T) -> WaitAny"],
      ["WaitAll", "Error", "Error(own int32, own String) -> WaitAll"]
    ]) {
      const builtinSource = `def main():\n    ${enumName}.\n`;
      const builtinCompletions = await completeWithCompiler(
        mainUri,
        builtinSource,
        1,
        `    ${enumName}.`.length,
        "."
      );
      assert.equal(
        builtinCompletions.find((completion) => completion.name === variantName)?.detail,
        detail
      );
    }
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
      "from pkg.named import Named\n\npublic class User:\n    public label: String\n\nimpl Named for User:\n    def name(borrow self) -> String:\n        return self.label.clone()\n"
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
    assert.ok(completions.some((item) => item.name === "name" && item.detail === "name(self) -> String"));
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge preserves cross-file definitions for imported function, field, and trait method uses", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-cross-file-"));
  try {
    fs.mkdirSync(path.join(tempRoot, "pkg"));
    const mathPath = path.join(tempRoot, "pkg/math.au");
    const userPath = path.join(tempRoot, "pkg/user.au");
    const canonicalMathPath = fs.realpathSync.native(path.dirname(mathPath))
      ? path.join(fs.realpathSync.native(path.dirname(mathPath)), path.basename(mathPath))
      : mathPath;
    const canonicalUserPath = fs.realpathSync.native(path.dirname(userPath))
      ? path.join(fs.realpathSync.native(path.dirname(userPath)), path.basename(userPath))
      : userPath;
    fs.writeFileSync(
      mathPath,
      "public def add(left: int32, right: int32) -> int32:\n    return left + right\n"
    );
    fs.writeFileSync(
      path.join(tempRoot, "pkg/named.au"),
      "public trait Named:\n    def name(borrow self) -> String\n"
    );
    fs.writeFileSync(
      userPath,
      "from pkg.named import Named\n\npublic class User:\n    public label: String\n\nimpl Named for User:\n    def name(borrow self) -> String:\n        return self.label.clone()\n"
    );
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const source = [
      "from pkg.math import add",
      "from pkg.user import User",
      "",
      "def main() -> int32:",
      "    total = add(left=1, right=2)",
      "    user = User(label=\"Ada\")",
      "    print(user.label)",
      "    print(user.name())",
      "    return total"
    ].join("\n");

    setWorkspaceRoots([repoRoot, tempRoot]);
    const analysis = await analyzeWithCompiler(mainUri, source);
    assert.ok(analysis);
    assert.equal(analysis.diagnostics.length, 0);
    assert.ok(
      analysis.occurrences.some(
        (occurrence) =>
          occurrence.hover.includes("function add") &&
          occurrence.definition?.file_path === canonicalMathPath
      )
    );
    assert.ok(
      analysis.occurrences.some(
        (occurrence) =>
          occurrence.hover.includes("class User") &&
          occurrence.definition?.file_path === canonicalUserPath
      )
    );
    assert.ok(
      analysis.occurrences.some(
        (occurrence) =>
          occurrence.hover.includes("field label: String") &&
          occurrence.definition?.file_path === canonicalUserPath
      )
    );
    assert.ok(
      analysis.occurrences.some(
        (occurrence) =>
          occurrence.hover.includes("method name(self) -> String") &&
          occurrence.definition?.file_path === canonicalUserPath
      )
    );
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge analyzes and completes manifest-rooted path dependencies", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-packages-"));
  try {
    fs.mkdirSync(path.join(tempRoot, "app", "src", "helpers"), { recursive: true });
    fs.mkdirSync(path.join(tempRoot, "util", "src"), { recursive: true });
    fs.writeFileSync(
      path.join(tempRoot, "app", "Aurora.toml"),
      [
        "[package]",
        'name = "app"',
        'version = "0.1.0"',
        'edition = "2026"',
        "",
        "[dependencies]",
        'util = { path = "../util" }'
      ].join("\n")
    );
    fs.writeFileSync(
      path.join(tempRoot, "app", "src", "helpers", "math.au"),
      "public def triple(value: int32) -> int32:\n    return value * 3\n"
    );
    fs.writeFileSync(
      path.join(tempRoot, "util", "Aurora.toml"),
      ['[package]', 'name = "util"', 'version = "0.1.0"', 'edition = "2026"'].join("\n")
    );
    fs.writeFileSync(
      path.join(tempRoot, "util", "src", "math.au"),
      "public def double(value: int32) -> int32:\n    return value * 2\n"
    );
    const canonicalUtilMathPath = path.join(
      fs.realpathSync.native(path.join(tempRoot, "util", "src")),
      "math.au"
    );

    const mainPath = path.join(tempRoot, "app", "src", "main.au");
    const mainUri = `file://${mainPath}`;
    const validSource = [
      "import util.math",
      "import helpers.math",
      "",
      "def main() -> int32:",
      "    print(util.math.double(value=helpers.math.triple(value=2)))",
      "    return 0"
    ].join("\n");

    setWorkspaceRoots([repoRoot, tempRoot]);
    const analysis = await analyzeWithCompiler(mainUri, validSource);
    assert.ok(analysis);
    assert.equal(analysis.diagnostics.length, 0);
    assert.ok(
      analysis.occurrences.some(
        (occurrence) =>
          occurrence.hover.includes("function double") &&
          occurrence.definition?.file_path === canonicalUtilMathPath
      )
    );

    const completionSource = [
      "import util.math",
      "import helpers.math",
      "",
      "def main() -> int32:",
      "    util.math.",
      "    return helpers.math.triple(value=2)"
    ].join("\n");
    const completions = await completeWithCompiler(mainUri, completionSource, 4, 14, ".");
    assert.ok(completions);
    assert.ok(completions.some((item) => item.name === "double"));
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge analyzes and completes manifest-rooted git dependencies", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-git-packages-"));
  try {
    const appRoot = path.join(tempRoot, "app");
    const repoRootPath = path.join(tempRoot, "util-repo");
    fs.mkdirSync(path.join(appRoot, "src"), { recursive: true });
    fs.mkdirSync(path.join(repoRootPath, "src"), { recursive: true });
    fs.writeFileSync(
      path.join(appRoot, "Aurora.toml"),
      [
        "[package]",
        'name = "app"',
        'version = "0.1.0"',
        'edition = "2026"',
        "",
        "[dependencies]",
        'util = { git = "../util-repo" }'
      ].join("\n")
    );
    fs.writeFileSync(
      path.join(repoRootPath, "Aurora.toml"),
      ['[package]', 'name = "util"', 'version = "0.1.0"', 'edition = "2026"'].join("\n")
    );
    fs.writeFileSync(
      path.join(repoRootPath, "src", "math.au"),
      "public def double(value: int32) -> int32:\n    return value * 2\n"
    );
    childProcess.execFileSync("git", ["init", "-b", "main"], { cwd: repoRootPath });
    childProcess.execFileSync("git", ["config", "user.name", "Aurora Tests"], { cwd: repoRootPath });
    childProcess.execFileSync("git", ["config", "user.email", "aurora-tests@example.com"], {
      cwd: repoRootPath
    });
    childProcess.execFileSync("git", ["add", "."], { cwd: repoRootPath });
    childProcess.execFileSync("git", ["commit", "-m", "initial"], { cwd: repoRootPath });
    const mainPath = path.join(appRoot, "src", "main.au");
    const mainUri = `file://${mainPath}`;
    const validSource = [
      "import util.math",
      "",
      "def main() -> int32:",
      "    print(util.math.double(value=2))",
      "    return 0"
    ].join("\n");

    setWorkspaceRoots([repoRoot, tempRoot]);
    const analysis = await analyzeWithCompiler(mainUri, validSource);
    assert.ok(analysis);
    assert.equal(analysis.diagnostics.length, 0);
    assert.ok(
      analysis.occurrences.some(
        (occurrence) =>
          occurrence.hover.includes("function double") &&
          occurrence.definition?.file_path &&
          occurrence.definition.file_path.endsWith(`${path.sep}src${path.sep}math.au`)
      )
    );

    const completionSource = [
      "import util.math",
      "",
      "def main() -> int32:",
      "    util.math.",
      "    return 0"
    ].join("\n");
    const completions = await completeWithCompiler(mainUri, completionSource, 3, 14, ".");
    assert.ok(completions);
    assert.ok(completions.some((item) => item.name === "double"));
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge maps cross-file definitions to file URIs", () => {
  const location = compilerDefinitionToLspLocation("file:///workspace/main.au", {
    file_path: path.join(repoRoot, "examples/modules/pkg/types.au"),
    line: 1,
    start_character: 4,
    end_character: 11
  });

  assert.equal(location.uri, `file://${path.join(repoRoot, "examples/modules/pkg/types.au")}`);
  assert.deepEqual(location.range, {
    start: { line: 1, character: 4 },
    end: { line: 1, character: 11 }
  });
});

test("compiler bridge includes Vec collection members in completions", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-vec-"));
  try {
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const source =
      "def main() -> int32:\n    mut values = [1, 2, 3]\n    values.\n    return 0\n";

    setWorkspaceRoots([repoRoot, tempRoot]);
    const analysis = await analyzeWithCompiler(mainUri, source);
    assert.ok(analysis);
    assert.ok(Array.isArray(analysis.diagnostics));

    const lineIndex = source.split("\n").findIndex((line) => line.includes("values."));
    const lineText = source.split("\n")[lineIndex];
    const character = lineText.indexOf(".") + 1;
    const completions = await completeWithCompiler(mainUri, source, lineIndex, character, ".");

    assert.ok(completions);
    const names = new Set(completions.map((item) => item.name));
    assert.ok(names.has("len"));
    assert.ok(names.has("is_empty"));
    assert.ok(names.has("push"));
    assert.ok(names.has("pop"));
    assert.ok(names.has("get"));
    assert.ok(names.has("set"));
    assert.ok(names.has("remove"));
    assert.ok(names.has("swap"));
    assert.ok(names.has("contains"));
    assert.ok(names.has("extend"));
    assert.ok(names.has("insert"));
    assert.ok(names.has("clear"));
    assert.ok(names.has("reverse"));
    const details = new Map(completions.map((item) => [item.name, item.detail]));
    assert.equal(details.get("push"), "push(value: own T) -> None");
    assert.equal(details.get("set"), "set(index: int32, value: own T) -> Option[T]");
    assert.equal(details.get("extend"), "extend(other: own Vec[T]) -> None");
    assert.equal(details.get("insert"), "insert(index: int32, value: own T) -> bool");
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge includes String and Map builtin members in completions", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-string-map-"));
  try {
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const source =
      "def main() -> int32:\n    text = '  aurora repo  '\n    mut counts = Map[String, int32]()\n    text.\n    counts.\n    return 0\n";

    setWorkspaceRoots([repoRoot, tempRoot]);
    const analysis = await analyzeWithCompiler(mainUri, source);
    assert.ok(analysis);
    assert.ok(Array.isArray(analysis.diagnostics));

    const lines = source.split("\n");
    const textLineIndex = lines.findIndex((line) => line.includes("text."));
    const textCharacter = lines[textLineIndex].indexOf(".") + 1;
    const textCompletions = await completeWithCompiler(
      mainUri,
      source,
      textLineIndex,
      textCharacter,
      "."
    );

    assert.ok(textCompletions);
    const textNames = new Set(textCompletions.map((item) => item.name));
    assert.ok(textNames.has("len"));
    assert.ok(textNames.has("byte_len"));
    assert.ok(textNames.has("contains"));
    assert.ok(textNames.has("starts_with"));
    assert.ok(textNames.has("ends_with"));
    assert.ok(textNames.has("trim"));
    assert.ok(textNames.has("split"));
    assert.ok(textNames.has("replace"));
    assert.ok(textNames.has("to_lower"));
    assert.ok(textNames.has("to_upper"));
    assert.ok(textNames.has("strip_prefix"));
    assert.ok(textNames.has("strip_suffix"));
    assert.ok(textNames.has("clone"));
    assert.ok(textNames.has("join"));
    assert.equal(
      textCompletions.find((item) => item.name === "len")?.detail,
      "len() -> int32"
    );
    assert.equal(
      textCompletions.find((item) => item.name === "byte_len")?.detail,
      "byte_len() -> int32"
    );

    const mapLineIndex = lines.findIndex((line) => line.includes("counts."));
    const mapCharacter = lines[mapLineIndex].indexOf(".") + 1;
    const mapCompletions = await completeWithCompiler(
      mainUri,
      source,
      mapLineIndex,
      mapCharacter,
      "."
    );

    assert.ok(mapCompletions);
    const mapNames = new Set(mapCompletions.map((item) => item.name));
    assert.ok(mapNames.has("len"));
    assert.ok(mapNames.has("is_empty"));
    assert.ok(mapNames.has("clone"));
    assert.ok(mapNames.has("get"));
    assert.ok(mapNames.has("set"));
    assert.ok(mapNames.has("remove"));
    assert.ok(mapNames.has("contains_key"));
    assert.ok(mapNames.has("keys"));
    assert.ok(mapNames.has("values"));
    assert.ok(mapNames.has("items"));
    assert.ok(mapNames.has("entries"));
    assert.ok(mapNames.has("clear"));
    assert.ok(mapNames.has("extend"));
    assert.equal(
      mapCompletions.find((item) => item.name === "set")?.detail,
      "set(key: own K, value: own V) -> Option[V]"
    );
    assert.equal(
      mapCompletions.find((item) => item.name === "extend")?.detail,
      "extend(other: own Map[K, V]) -> None"
    );
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge includes Set collection members and MapEntry fields", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-set-mapentry-"));
  try {
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const source = [
      "def main() -> int32:",
      "    mut seen = {1, 2, 3}",
      "    counts: Map[String, int32] = {\"a\": 1, \"b\": 2}",
      "    entries: Vec[MapEntry[String, int32]] = counts.items()",
      "    match entries.get(index=0):",
      "        case Some(found):",
      "            entry = found",
      "            seen.",
      "            entry.",
      "        case None:",
      "            pass",
      "    return 0"
    ].join("\n");

    setWorkspaceRoots([repoRoot, tempRoot]);
    const analysis = await analyzeWithCompiler(mainUri, source);
    assert.ok(analysis);
    assert.ok(Array.isArray(analysis.diagnostics));

    const lines = source.split("\n");
    const seenLineIndex = lines.findIndex((line) => line.includes("seen."));
    const seenCharacter = lines[seenLineIndex].indexOf(".") + 1;
    const setCompletions = await completeWithCompiler(
      mainUri,
      source,
      seenLineIndex,
      seenCharacter,
      "."
    );

    assert.ok(setCompletions);
    const setNames = new Set(setCompletions.map((item) => item.name));
    assert.ok(setNames.has("len"));
    assert.ok(setNames.has("is_empty"));
    assert.ok(setNames.has("clone"));
    assert.ok(setNames.has("contains"));
    assert.ok(setNames.has("insert"));
    assert.ok(setNames.has("remove"));

    const entryLineIndex = lines.findIndex((line) => line.includes("entry."));
    const entryCharacter = lines[entryLineIndex].indexOf(".") + 1;
    const entryCompletions = await completeWithCompiler(
      mainUri,
      source,
      entryLineIndex,
      entryCharacter,
      "."
    );

    assert.ok(entryCompletions);
    const entryNames = new Set(entryCompletions.map((item) => item.name));
    assert.ok(entryNames.has("key"));
    assert.ok(entryNames.has("value"));
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge includes builtin io/fs/net module and resource members", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-io-net-"));
  try {
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const prelude = [
      "import io",
      "import fs",
      "import net",
      "",
      "def inspect(file: fs.File, listener: net.TcpListener, stream: net.TcpStream, udp: net.UdpSocket, packet: net.UdpDatagram, http_listener: net.HttpListener, exchange: net.HttpExchange, response: net.HttpResponse, ws_listener: net.WebSocketListener, socket: net.WebSocket, unix_listener: net.UnixListener, unix_stream: net.UnixStream, tls_listener: net.TlsListener, tls_stream: net.TlsStream) -> int32:"
    ];
    const sourceForLine = (line) => [...prelude, line, "    return 0"].join("\n");
    const completionsForLine = async (line) => {
      const source = sourceForLine(line);
      const lines = source.split("\n");
      const lineIndex = lines.findIndex((candidate) => candidate === line);
      const character = lines[lineIndex].indexOf(".") + 1;
      const items = await completeWithCompiler(mainUri, source, lineIndex, character, ".");
      assert.ok(items);
      return new Set(items.map((item) => item.name));
    };

    setWorkspaceRoots([repoRoot, tempRoot]);
    const analysis = await analyzeWithCompiler(mainUri, sourceForLine("    return 0"));
    assert.ok(analysis);
    assert.ok(Array.isArray(analysis.diagnostics));
    assert.equal(analysis.diagnostics.length, 0);

    const ioNames = await completionsForLine("    io.");
    assert.ok(ioNames.has("write"));
    assert.ok(ioNames.has("flush"));
    assert.ok(ioNames.has("read_line"));
    assert.ok(ioNames.has("Error"));

    const fsNames = await completionsForLine("    fs.");
    assert.ok(fsNames.has("open"));
    assert.ok(fsNames.has("create"));
    assert.ok(fsNames.has("append"));
    assert.ok(fsNames.has("read_to_string"));
    assert.ok(fsNames.has("read_bytes"));
    assert.ok(fsNames.has("write_string"));
    assert.ok(fsNames.has("write_bytes"));
    assert.ok(fsNames.has("append_bytes"));
    assert.ok(fsNames.has("File"));

    const fileNames = await completionsForLine("    file.");
    assert.ok(fileNames.has("read_all"));
    assert.ok(fileNames.has("read_bytes"));
    assert.ok(fileNames.has("write_all"));
    assert.ok(fileNames.has("write_bytes"));
    assert.ok(fileNames.has("flush"));
    assert.ok(fileNames.has("close"));

    const netNames = await completionsForLine("    net.");
    assert.ok(netNames.has("connect_timeout"));
    assert.ok(netNames.has("udp_bind"));
    assert.ok(netNames.has("http_listen"));
    assert.ok(netNames.has("http_request_text"));
    assert.ok(netNames.has("http_request_text_timeout"));
    assert.ok(netNames.has("http_request_bytes"));
    assert.ok(netNames.has("http_request_bytes_timeout"));
    assert.ok(netNames.has("websocket_listen"));
    assert.ok(netNames.has("websocket_connect"));
    assert.ok(netNames.has("websocket_connect_timeout"));
    assert.ok(netNames.has("unix_listen"));
    assert.ok(netNames.has("unix_connect"));
    assert.ok(netNames.has("unix_connect_timeout"));
    assert.ok(netNames.has("tls_listen"));
    assert.ok(netNames.has("tls_connect"));
    assert.ok(netNames.has("tls_connect_timeout"));
    assert.ok(netNames.has("UdpSocket"));
    assert.ok(netNames.has("HttpResponse"));
    assert.ok(netNames.has("TlsStream"));

    const streamNames = await completionsForLine("    stream.");
    assert.ok(streamNames.has("read_all"));
    assert.ok(streamNames.has("read_line"));
    assert.ok(streamNames.has("read_bytes"));
    assert.ok(streamNames.has("read_exact"));
    assert.ok(streamNames.has("write_all"));
    assert.ok(streamNames.has("write_bytes"));
    assert.ok(streamNames.has("flush"));
    assert.ok(streamNames.has("local_addr"));
    assert.ok(streamNames.has("peer_addr"));
    assert.ok(streamNames.has("shutdown_read"));
    assert.ok(streamNames.has("shutdown_write"));
    assert.ok(streamNames.has("shutdown_both"));
    assert.ok(streamNames.has("close"));

    const udpNames = await completionsForLine("    udp.");
    assert.ok(udpNames.has("send_text"));
    assert.ok(udpNames.has("send_bytes"));
    assert.ok(udpNames.has("recv"));
    assert.ok(udpNames.has("recv_from"));
    assert.ok(udpNames.has("local_addr"));
    assert.ok(udpNames.has("peer_addr"));

    const exchangeNames = await completionsForLine("    exchange.");
    assert.ok(exchangeNames.has("method"));
    assert.ok(exchangeNames.has("path"));
    assert.ok(exchangeNames.has("headers"));
    assert.ok(exchangeNames.has("body_text"));
    assert.ok(exchangeNames.has("body_bytes"));
    assert.ok(exchangeNames.has("respond_text"));
    assert.ok(exchangeNames.has("respond_bytes"));

    const responseNames = await completionsForLine("    response.");
    assert.ok(responseNames.has("status"));
    assert.ok(responseNames.has("reason"));
    assert.ok(responseNames.has("headers"));
    assert.ok(responseNames.has("text"));
    assert.ok(responseNames.has("bytes"));

    const socketNames = await completionsForLine("    socket.");
    assert.ok(socketNames.has("send_text"));
    assert.ok(socketNames.has("send_bytes"));
    assert.ok(socketNames.has("recv_text"));
    assert.ok(socketNames.has("recv_bytes"));

    const unixStreamNames = await completionsForLine("    unix_stream.");
    assert.ok(unixStreamNames.has("read_line"));
    assert.ok(unixStreamNames.has("read_exact"));
    assert.ok(unixStreamNames.has("write_all"));

    const tlsStreamNames = await completionsForLine("    tls_stream.");
    assert.ok(tlsStreamNames.has("read_line"));
    assert.ok(tlsStreamNames.has("read_exact"));
    assert.ok(tlsStreamNames.has("write_all"));
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge includes builtin process module and resource members", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-process-"));
  try {
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const prelude = [
      "import process",
      "",
      "def inspect(child: process.Child, pipe: process.Pipe, completed: process.Completed, status: process.ExitStatus, wait: process.Wait, stdio: process.Stdio, error: process.Error) -> int32:"
    ];
    const sourceForLine = (line) => [...prelude, line, "    return 0"].join("\n");
    const completionsForLine = async (line) => {
      const source = sourceForLine(line);
      const lines = source.split("\n");
      const lineIndex = lines.findIndex((candidate) => candidate === line);
      const character = lines[lineIndex].indexOf(".") + 1;
      const items = await completeWithCompiler(mainUri, source, lineIndex, character, ".");
      assert.ok(items);
      return new Set(items.map((item) => item.name));
    };

    setWorkspaceRoots([repoRoot, tempRoot]);
    const analysis = await analyzeWithCompiler(mainUri, sourceForLine("    return 0"));
    assert.ok(analysis);
    assert.ok(Array.isArray(analysis.diagnostics));
    assert.equal(analysis.diagnostics.length, 0);

    const processNames = await completionsForLine("    process.");
    assert.ok(processNames.has("start"));
    assert.ok(processNames.has("run"));
    assert.ok(processNames.has("inherit"));
    assert.ok(processNames.has("null"));
    assert.ok(processNames.has("pipe"));
    assert.ok(processNames.has("Child"));
    assert.ok(processNames.has("Pipe"));
    assert.ok(processNames.has("Completed"));
    assert.ok(processNames.has("ExitStatus"));
    assert.ok(processNames.has("Wait"));
    assert.ok(processNames.has("Stdio"));
    assert.ok(processNames.has("Error"));

    const childNames = await completionsForLine("    child.");
    assert.ok(childNames.has("stdin"));
    assert.ok(childNames.has("stdout"));
    assert.ok(childNames.has("stderr"));
    assert.ok(childNames.has("wait"));
    assert.ok(childNames.has("kill"));
    assert.ok(childNames.has("terminate"));
    assert.ok(childNames.has("close"));

    const pipeNames = await completionsForLine("    pipe.");
    assert.ok(pipeNames.has("read_all"));
    assert.ok(pipeNames.has("read_line"));
    assert.ok(pipeNames.has("read_bytes"));
    assert.ok(pipeNames.has("write_all"));
    assert.ok(pipeNames.has("write_bytes"));
    assert.ok(pipeNames.has("flush"));
    assert.ok(pipeNames.has("close"));

    const completedNames = await completionsForLine("    completed.");
    assert.ok(completedNames.has("status"));
    assert.ok(completedNames.has("success"));
    assert.ok(completedNames.has("stdout"));
    assert.ok(completedNames.has("stderr"));
    assert.ok(completedNames.has("stdout_bytes"));
    assert.ok(completedNames.has("stderr_bytes"));
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge exposes control-plane module completions", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-control-plane-"));
  try {
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const modules = ["sys", "path", "json", "toml", "log", "metrics", "trace"];
    const prelude = [...modules.map((name) => `import ${name}`), "", "def main() -> int32:"];
    const completions = async (moduleName) => {
      const line = `    ${moduleName}.`;
      const source = [...prelude, line, "    return 0"].join("\n");
      const lineIndex = prelude.length;
      const items = await completeWithCompiler(
        mainUri,
        source,
        lineIndex,
        line.length,
        "."
      );
      assert.ok(items);
      return new Set(items.map((item) => item.name));
    };
    setWorkspaceRoots([repoRoot, tempRoot]);
    assert.ok((await completions("sys")).has("args"));
    assert.ok((await completions("path")).has("join"));
    assert.ok((await completions("json")).has("parse_string_map"));
    assert.ok((await completions("toml")).has("stringify_map"));
    assert.ok((await completions("log")).has("info"));
    assert.ok((await completions("metrics")).has("increment"));
    assert.ok((await completions("trace")).has("event"));
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge exposes the recursive json.Value contract", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-json-value-"));
  try {
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const prelude = ["import json", "", "def main() -> int32:"];
    const completionsForLine = async (line) => {
      const source = [...prelude, line, "    return 0"].join("\n");
      const lineIndex = prelude.length;
      const items = await completeWithCompiler(mainUri, source, lineIndex, line.length, ".");
      assert.ok(items);
      return items;
    };

    setWorkspaceRoots([repoRoot, tempRoot]);
    const validSource = [
      "import json",
      "",
      "def decode(text: String) -> Result[json.Value, json.Error]:",
      "    return json.parse(text)",
      "",
      "def main() -> int32:",
      "    value = json.Value.Int(7)",
      "    print(json.dumps(value, indent=Option.Some(2)))",
      "    return 0"
    ].join("\n");
    const analysis = await analyzeWithCompiler(mainUri, validSource);
    assert.ok(analysis);
    assert.deepEqual(analysis.diagnostics, []);
    assert.ok(
      analysis.occurrences.some(
        (occurrence) =>
          occurrence.hover.includes("parse") &&
          occurrence.hover.includes("Result[json.Value, json.Error]")
      )
    );

    const moduleItems = await completionsForLine("    json.");
    const moduleNames = new Set(moduleItems.map((item) => item.name));
    for (const expected of [
      "Value",
      "Error",
      "parse",
      "dumps",
      "is_null",
      "as_bool",
      "as_int",
      "as_float",
      "into_string",
      "into_array",
      "into_object"
    ]) {
      assert.ok(moduleNames.has(expected), `json completion should include ${expected}`);
    }
    assert.equal(
      moduleItems.find((item) => item.name === "parse")?.detail,
      "parse(text: String) -> Result[json.Value, json.Error]"
    );
    assert.equal(
      moduleItems.find((item) => item.name === "dumps")?.detail,
      "dumps(value: json.Value, indent: Option[int64] = ...) -> String"
    );
    const accessorDetails = {
      as_bool: "as_bool(value: borrow json.Value) -> Option[bool]",
      as_float: "as_float(value: borrow json.Value) -> Option[float64]",
      as_int: "as_int(value: borrow json.Value) -> Option[int64]",
      into_array:
        "into_array(value: own json.Value) -> Option[Vec[json.Value]]",
      into_object:
        "into_object(value: own json.Value) -> Option[Map[String, json.Value]]",
      into_string: "into_string(value: own json.Value) -> Option[String]",
      is_null: "is_null(value: borrow json.Value) -> bool"
    };
    for (const [name, detail] of Object.entries(accessorDetails)) {
      assert.equal(moduleItems.find((item) => item.name === name)?.detail, detail);
    }

    const valueItems = await completionsForLine("    json.Value.");
    assert.deepEqual(
      new Set(valueItems.map((item) => item.name)),
      new Set(["Null", "Bool", "Int", "Float", "String", "Array", "Object"])
    );
    assert.deepEqual(
      Object.fromEntries(valueItems.map((item) => [item.name, item.detail])),
      {
        Array: "Array(own Vec[json.Value]) -> json.Value",
        Bool: "Bool(own bool) -> json.Value",
        Float: "Float(own float64) -> json.Value",
        Int: "Int(own int64) -> json.Value",
        Null: "Null -> json.Value",
        Object: "Object(own Map[String, json.Value]) -> json.Value",
        String: "String(own String) -> json.Value"
      }
    );
    const errorItems = await completionsForLine("    json.Error.");
    assert.deepEqual(
      Object.fromEntries(errorItems.map((item) => [item.name, item.detail])),
      {
        InputTooLarge:
          "InputTooLarge(actual_bytes: own int64, limit_bytes: own int64) -> json.Error",
        NestingTooDeep:
          "NestingTooDeep(limit: own int32, line: own int32, column: own int32) -> json.Error",
        NumberOutOfRange:
          "NumberOutOfRange(line: own int32, column: own int32) -> json.Error",
        Syntax:
          "Syntax(message: own String, line: own int32, column: own int32) -> json.Error"
      }
    );
    assert.ok(
      analysis.occurrences.some(
        (occurrence) =>
          occurrence.hover === "```aurora\nvariant Int(own int64) -> json.Value\n```"
      )
    );

    const fromImportSource = [
      "from json import Value, Error, parse, dumps",
      "",
      "def decode(text: String) -> Result[Value, Error]:",
      "    return parse(text)",
      "",
      "def main() -> int32:",
      "    value = Value.Int(7)",
      "    print(dumps(value))",
      "    return 0"
    ].join("\n");
    const fromImportAnalysis = await analyzeWithCompiler(mainUri, fromImportSource);
    assert.ok(fromImportAnalysis);
    assert.deepEqual(fromImportAnalysis.diagnostics, []);
    assert.ok(
      fromImportAnalysis.occurrences.some(
        (occurrence) =>
          occurrence.hover === "```aurora\nvariant Int(own int64) -> json.Value\n```"
      )
    );

    const fromImportCompletionSource = [
      "from json import Value",
      "",
      "def main() -> int32:",
      "    Value.",
      "    return 0"
    ].join("\n");
    const fromImportItems = await completeWithCompiler(
      mainUri,
      fromImportCompletionSource,
      3,
      "    Value.".length,
      "."
    );
    assert.ok(fromImportItems);
    assert.equal(
      fromImportItems.find((item) => item.name === "Int")?.detail,
      "Int(own int64) -> json.Value"
    );
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge analyzes indexed member chains and f-string indexed lookups", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-index-chain-"));
  try {
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const source = [
      "def main() -> int32:",
      "    keys = [\"a\", \"b\"]",
      "    idx: int32 = 1",
      "    mut counts = {\"key\": 7}",
      "    match keys.get(index=idx):",
      "        case Some(key):",
      "            print(key)",
      "        case None:",
      "            return 1",
      "    print(f\"val: {counts[\"key\"]}\")",
      "    return 0"
    ].join("\n");

    setWorkspaceRoots([repoRoot, tempRoot]);
    const analysis = await analyzeWithCompiler(mainUri, source);
    assert.ok(analysis);
    assert.deepStrictEqual(analysis.diagnostics, []);
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge analyzes single-quoted strings nested in f-string interpolations", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-single-strings-"));
  try {
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const source = [
      "def main() -> int32:",
      "    print('single # quote')",
      "    print(f\"{'{left} and }'}\")",
      "    return 0"
    ].join("\n");

    setWorkspaceRoots([repoRoot, tempRoot]);
    const analysis = await analyzeWithCompiler(mainUri, source);
    assert.ok(analysis);
    assert.deepStrictEqual(analysis.diagnostics, []);
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

test("compiler bridge exposes one random.Rng constructor and its stateful members", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-random-"));
  try {
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const prelude = [
      "import random",
      "",
      "def inspect(rng: borrow mut random.Rng) -> int32:"
    ];
    const sourceForLine = (line) => [...prelude, line, "    return 0"].join("\n");
    const completionsForLine = async (line) => {
      const source = sourceForLine(line);
      const lines = source.split("\n");
      const lineIndex = lines.findIndex((candidate) => candidate === line);
      const character = lines[lineIndex].indexOf(".") + 1;
      const items = await completeWithCompiler(mainUri, source, lineIndex, character, ".");
      assert.ok(items);
      return items;
    };

    setWorkspaceRoots([repoRoot, tempRoot]);
    const validSource = sourceForLine("    print(rng.next_int(lo=0, hi=10))");
    const analysis = await analyzeWithCompiler(mainUri, validSource);
    assert.ok(analysis);
    assert.deepEqual(analysis.diagnostics, []);

    const unavailableSecureFloat = await analyzeWithCompiler(
      mainUri,
      sourceForLine("    print(random.secure_float())")
    );
    assert.ok(unavailableSecureFloat);
    assert.equal(unavailableSecureFloat.diagnostics.length, 1);
    assert.equal(unavailableSecureFloat.diagnostics[0].code, "AU2001");
    assert.match(
      unavailableSecureFloat.diagnostics[0].message,
      /module `random` has no callable member `secure_float`/
    );

    const moduleItems = await completionsForLine("    random.");
    const rngItems = moduleItems.filter((item) => item.name === "Rng");
    assert.equal(rngItems.length, 1);
    assert.equal(rngItems[0].kind, "class");
    assert.equal(rngItems[0].detail, "Rng(seed: int64)");
    assert.equal(
      moduleItems.find((item) => item.name === "secure_int")?.detail,
      "secure_int(lo: int64, hi: int64) -> int64"
    );
    assert.equal(
      moduleItems.find((item) => item.name === "secure_bytes")?.detail,
      "secure_bytes(n: int64) -> Vec[uint8]"
    );
    assert.equal(moduleItems.some((item) => item.name === "secure_float"), false);

    const memberItems = await completionsForLine("    rng.");
    for (const [name, detail] of [
      ["next_int", "next_int(lo: int64, hi: int64) -> int64"],
      ["next_float", "next_float() -> float64"],
      ["shuffle", "shuffle(values: borrow mut Vec[T]) -> None"]
    ]) {
      const matching = memberItems.filter((item) => item.name === name);
      assert.equal(matching.length, 1);
      assert.equal(matching[0].kind, "method");
      assert.equal(matching[0].detail, detail);
    }
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge exposes the canonical bytes module, errors, and String conversions", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-bytes-"));
  try {
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const prelude = ["import bytes", "", "def main() -> int32:"];
    const completionsForLine = async (line) => {
      const source = [...prelude, line, "    return 0"].join("\n");
      const lineIndex = prelude.length;
      const items = await completeWithCompiler(
        mainUri,
        source,
        lineIndex,
        line.length,
        "."
      );
      assert.ok(items);
      return items;
    };

    setWorkspaceRoots([repoRoot, tempRoot]);
    const validSource = [
      "import bytes",
      "",
      "def decode(value: Vec[uint8]) -> Result[String, bytes.Error]:",
      "    return String.from_bytes(bytes=value)",
      "",
      "def main() -> int32:",
      "    text = \"abc\"",
      "    payload = text.to_bytes()",
      "    print(bytes.hex_encode(value=payload))",
      "    print(bytes.base64_encode(value=payload))",
      "    print(bytes.sha256(value=payload))",
      "    print(bytes.sha256_string(text=text))",
      "    return 0"
    ].join("\n");
    const analysis = await analyzeWithCompiler(mainUri, validSource);
    assert.ok(analysis);
    assert.deepEqual(analysis.diagnostics, []);
    for (const signature of [
      "from_bytes(bytes: Vec[uint8]) -> Result[String, bytes.Error]",
      "to_bytes() -> Vec[uint8]",
      "hex_encode(value: Vec[uint8]) -> String",
      "base64_encode(value: Vec[uint8]) -> String",
      "sha256(value: Vec[uint8]) -> Vec[uint8]",
      "sha256_string(text: String) -> Vec[uint8]"
    ]) {
      assert.ok(
        analysis.occurrences.some((occurrence) =>
          occurrence.hover.includes(signature)
        ),
        `missing Bytes hover: ${signature}`
      );
    }

    const moduleItems = await completionsForLine("    bytes.");
    const moduleNames = new Set(moduleItems.map((item) => item.name));
    for (const expected of [
      "Error",
      "hex_encode",
      "hex_decode",
      "base64_encode",
      "base64_decode",
      "sha256",
      "sha256_string"
    ]) {
      assert.ok(moduleNames.has(expected), `bytes completion should include ${expected}`);
    }
    const moduleDetails = {
      hex_encode: "hex_encode(value: Vec[uint8]) -> String",
      hex_decode: "hex_decode(text: String) -> Result[Vec[uint8], bytes.Error]",
      base64_encode: "base64_encode(value: Vec[uint8]) -> String",
      base64_decode:
        "base64_decode(text: String) -> Result[Vec[uint8], bytes.Error]",
      sha256: "sha256(value: Vec[uint8]) -> Vec[uint8]",
      sha256_string: "sha256_string(text: String) -> Vec[uint8]"
    };
    for (const [name, detail] of Object.entries(moduleDetails)) {
      assert.equal(moduleItems.find((item) => item.name === name)?.detail, detail);
    }

    const errorItems = await completionsForLine("    bytes.Error.");
    assert.deepEqual(
      Object.fromEntries(errorItems.map((item) => [item.name, item.detail])),
      {
        InvalidBase64:
          "InvalidBase64(index: own int32) -> bytes.Error",
        InvalidHexDigit:
          "InvalidHexDigit(index: own int32, byte: own uint8) -> bytes.Error",
        InvalidHexLength:
          "InvalidHexLength(length: own int32) -> bytes.Error",
        InvalidUtf8:
          "InvalidUtf8(index: own int32) -> bytes.Error"
      }
    );

    const staticItems = await completionsForLine("    String.");
    assert.equal(
      staticItems.find((item) => item.name === "from_bytes")?.detail,
      "from_bytes(bytes: Vec[uint8]) -> Result[String, bytes.Error]"
    );
    assert.equal(staticItems.some((item) => item.name === "to_bytes"), false);

    const instanceLine = "    text.";
    const instanceSource = [
      ...prelude,
      "    text = \"abc\"",
      instanceLine,
      "    return 0"
    ].join("\n");
    const instanceItems = await completeWithCompiler(
      mainUri,
      instanceSource,
      prelude.length + 1,
      instanceLine.length,
      "."
    );
    assert.ok(instanceItems);
    assert.equal(
      instanceItems.find((item) => item.name === "to_bytes")?.detail,
      "to_bytes() -> Vec[uint8]"
    );
    assert.equal(instanceItems.some((item) => item.name === "from_bytes"), false);

    const fromImportSource = [
      "from bytes import Error, hex_decode",
      "",
      "def decode(text: String) -> Result[Vec[uint8], Error]:",
      "    return hex_decode(text)",
      "",
      "def main() -> int32:",
      "    return 0"
    ].join("\n");
    const fromImportAnalysis = await analyzeWithCompiler(mainUri, fromImportSource);
    assert.ok(fromImportAnalysis);
    assert.deepEqual(fromImportAnalysis.diagnostics, []);
    assert.ok(
      fromImportAnalysis.occurrences.some(
        (occurrence) =>
          occurrence.hover.includes("hex_decode") &&
          occurrence.hover.includes("bytes.Error")
      )
    );
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

test("compiler bridge exposes tuple return, index, destructuring, loop, and pattern analysis", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-tuples-"));
  const source = [
    "def make() -> (int64, String):",
    "    return (1, \"one\")",
    "",
    "def main():",
    "    pair = make()",
    "    first = pair[0]",
    "    number, label = pair",
    "    rows = [(2, 3)]",
    "    for left, right in rows:",
    "        print(left + right)",
    "    match (4, 5):",
    "        case (matched_left, matched_right):",
    "            print(matched_left + matched_right)",
    "    print(first)",
    "    print(number)",
    "    print(label)",
    ""
  ].join("\n");

  try {
    setWorkspaceRoots([repoRoot, tempRoot]);
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const analysis = await analyzeWithCompiler(mainUri, source);

    assert.ok(analysis);
    assert.deepEqual(analysis.diagnostics, []);
    assert.ok(
      analysis.symbols.some(
        (symbol) =>
          symbol.name === "make" &&
          symbol.kind === "function" &&
          symbol.detail === "(int64, String)"
      )
    );
    assert.deepEqual(compilerHoverAtPosition(analysis, 5, 13), {
      value: "```aurora\nbinding pair: (int64, String)\n```",
      range: {
        start: { line: 5, character: 12 },
        end: { line: 5, character: 16 }
      }
    });
    assert.ok(
      analysis.occurrences.some(
        (occurrence) =>
          occurrence.hover === "```aurora\nbinding number: int64\n```"
      )
    );
    assert.ok(
      analysis.occurrences.some(
        (occurrence) =>
          occurrence.hover === "```aurora\nbinding label: String\n```"
      )
    );
    assert.ok(
      analysis.occurrences.some(
        (occurrence) =>
          occurrence.hover === "```aurora\nlocal left: int64\n```"
      )
    );
    assert.ok(
      analysis.occurrences.some(
        (occurrence) =>
          occurrence.hover === "```aurora\nlocal right: int64\n```"
      )
    );
    assert.ok(
      analysis.occurrences.some(
        (occurrence) =>
          occurrence.hover === "```aurora\nlocal matched_left: int64\n```"
      )
    );
    assert.ok(
      analysis.occurrences.some(
        (occurrence) =>
          occurrence.hover === "```aurora\nlocal matched_right: int64\n```"
      )
    );
    assert.deepEqual(
      compilerDefinitionAtPosition(mainUri, analysis, 14, 12)?.range,
      {
        start: { line: 6, character: 4 },
        end: { line: 6, character: 10 }
      }
    );
    assert.deepEqual(
      compilerDefinitionAtPosition(mainUri, analysis, 12, 20)?.range,
      {
        start: { line: 11, character: 14 },
        end: { line: 11, character: 26 }
      }
    );
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("compiler bridge maps non-copy tuple index diagnostics", async () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "aurora-lsp-tuple-index-"));
  const source = [
    "def main():",
    "    pair = (\"left\", 1)",
    "    print(pair[0])",
    ""
  ].join("\n");

  try {
    setWorkspaceRoots([repoRoot, tempRoot]);
    const mainPath = path.join(tempRoot, "main.au");
    const mainUri = `file://${mainPath}`;
    const analysis = await analyzeWithCompiler(mainUri, source);

    assert.ok(analysis);
    assert.equal(analysis.diagnostics.length, 1);
    assert.deepEqual(analysis.diagnostics[0], {
      code: "AU3005",
      edits: [],
      end_character: 11,
      help: [],
      line: 2,
      message:
        "cannot consume non-copy tuple element `String` by indexing; unpack the tuple to move its elements",
      notes: [],
      secondary_spans: [],
      severity: 1,
      start_character: 10
    });

    const [diagnostic] = compilerDiagnosticsToLsp(analysis, mainUri);
    assert.equal(diagnostic.code, "AU3005");
    assert.equal(diagnostic.source, "aurora-compiler");
    assert.equal(
      diagnostic.message,
      "cannot consume non-copy tuple element `String` by indexing; unpack the tuple to move its elements"
    );
    assert.deepEqual(diagnostic.range, {
      start: { line: 2, character: 10 },
      end: { line: 2, character: 11 }
    });
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});
