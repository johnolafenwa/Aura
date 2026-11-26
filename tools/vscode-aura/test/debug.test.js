"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const {
  ADAPTER_NAMES,
  LLDB_DAP_MISSING,
  adapterConfiguration,
  debugBinaryPath,
  debugBuildArguments,
  defaultConfiguration,
  environmentList,
  findLldbDap,
  registerDebugger
} = require("../src/debug");

test("the adapter is the configured one, then Xcode's, then the first on PATH", () => {
  assert.equal(findLldbDap({ configured: "/opt/lldb-dap" }), "/opt/lldb-dap");
  assert.equal(
    findLldbDap({
      platform: "darwin",
      runFile: (command, args) => {
        assert.equal(command, "/usr/bin/xcrun");
        assert.deepEqual(args, ["-f", "lldb-dap"]);
        return "/Applications/Xcode.app/lldb-dap\n";
      }
    }),
    "/Applications/Xcode.app/lldb-dap"
  );
  const onPath = (found) => (candidate) => candidate === found;
  assert.equal(
    findLldbDap({
      platform: "darwin",
      pathValue: ["/a", "/b"].join(path.delimiter),
      runFile: () => {
        throw new Error("no Xcode");
      },
      executable: onPath(path.join("/b", "lldb-dap"))
    }),
    path.join("/b", "lldb-dap")
  );
  assert.equal(
    findLldbDap({
      platform: "darwin",
      pathValue: "/a",
      runFile: () => "  ",
      executable: onPath(path.join("/a", "lldb-dap"))
    }),
    path.join("/a", "lldb-dap")
  );
  assert.equal(
    findLldbDap({
      platform: "linux",
      pathValue: "/usr/bin",
      executable: onPath(path.join("/usr/bin", "lldb-dap-18"))
    }),
    path.join("/usr/bin", "lldb-dap-18")
  );
  assert.equal(
    findLldbDap({
      platform: "win32",
      pathValue: "C:\\llvm",
      executable: (candidate) => candidate.endsWith("lldb-dap.exe")
    }),
    path.win32.join("C:\\llvm", "lldb-dap.exe")
  );
  assert.equal(findLldbDap({ platform: "linux", pathValue: "", executable: () => true }), null);
  assert.ok(ADAPTER_NAMES.includes("lldb-vscode"));
});

test("the default PATH search checks for an executable file", () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "aura-dap-"));
  try {
    const adapter = path.join(directory, "lldb-dap");
    fs.mkdirSync(path.join(directory, "lldb-dap-24"));
    assert.equal(findLldbDap({ platform: "linux", pathValue: directory }), null);
    fs.writeFileSync(adapter, "#!/bin/sh\n", { mode: 0o755 });
    assert.equal(findLldbDap({ platform: "linux", pathValue: directory }), adapter);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

test("a debug build goes to the extension's storage", () => {
  const linuxBinary = debugBinaryPath("/w/app.au", "/store", "linux");
  assert.equal(path.dirname(linuxBinary), path.join("/store", "debug-builds"));
  assert.match(path.basename(linuxBinary), /^app-[0-9a-f]{16}$/);
  const winBinary = debugBinaryPath("/w/app.au", "/store", "win32");
  assert.equal(path.dirname(winBinary), path.join("/store", "debug-builds"));
  assert.match(path.basename(winBinary), /^app-[0-9a-f]{16}\.exe$/);
  assert.deepEqual(debugBuildArguments("/w/app.au", "/o/app"), [
    "build",
    "--debug",
    "-o",
    "/o/app",
    "--",
    "/w/app.au"
  ]);
});

test("findLldbDap ignores relative PATH entries and invokes /usr/bin/xcrun on macOS", () => {
  const checked = [];
  const found = findLldbDap({
    platform: "linux",
    pathValue: [".", "relative/bin", "/usr/bin"].join(path.delimiter),
    executable: (candidate) => {
      checked.push(candidate);
      return candidate === path.join("/usr/bin", "lldb-dap");
    }
  });
  assert.equal(found, path.join("/usr/bin", "lldb-dap"));
  assert.ok(!checked.some((candidate) => candidate === "lldb-dap" || candidate.startsWith("relative")));

  let darwinCommand = null;
  assert.equal(
    findLldbDap({
      platform: "darwin",
      runFile: (command, args) => {
        darwinCommand = command;
        assert.deepEqual(args, ["-f", "lldb-dap"]);
        return "/Applications/Xcode.app/lldb-dap\n";
      }
    }),
    "/Applications/Xcode.app/lldb-dap"
  );
  assert.equal(darwinCommand, "/usr/bin/xcrun");
});

test("debugBinaryPath keeps ...au inside debug-builds and isolates distinct directories", () => {
  const storage = "/store";
  const buildsDir = path.join(storage, "debug-builds");
  const dots = debugBinaryPath("/repo/...au", storage, "linux");
  assert.equal(path.dirname(dots), buildsDir);
  assert.notEqual(path.basename(dots), "..");
  assert.notEqual(path.basename(dots), ".");

  const first = debugBinaryPath("/project-a/app.au", storage, "linux");
  const second = debugBinaryPath("/project-b/app.au", storage, "linux");
  assert.equal(path.dirname(first), buildsDir);
  assert.equal(path.dirname(second), buildsDir);
  assert.notEqual(first, second);
});

test("launch rejects program paths starting with dash, non-.au files, or missing files", async () => {
  const storage = fs.mkdtempSync(path.join(os.tmpdir(), "aura-debug-validate-"));
  try {
    const fake = fakeVscode({ settings: { compilerPath: "/opt/aura" } });
    let built = false;
    const { provider } = registerDebugger(fake.vscode, fakeContext(storage), {
      runFile: (_command, _args, _options, done) => {
        built = true;
        done(null, "", "");
      }
    });
    const notAura = path.join(storage, "notes.txt");
    fs.writeFileSync(notAura, "hi\n");
    for (const badProgram of ["--freestanding", "-o/tmp/x.au", notAura, path.join(storage, "missing.au")]) {
      assert.equal(
        await provider.resolveDebugConfigurationWithSubstitutedVariables(undefined, {
          type: "aura",
          request: "launch",
          name: "Debug",
          program: badProgram
        }),
        undefined
      );
    }
    assert.equal(built, false);
    assert.equal(fake.errors.length, 4);
  } finally {
    fs.rmSync(storage, { recursive: true, force: true });
  }
});

test("lldb-dap gets the built binary, the formatter, and listed variables", () => {
  assert.deepEqual(environmentList({ A: "1", B: 2 }), ["A=1", "B=2"]);
  assert.deepEqual(environmentList(["A=1"]), ["A=1"]);
  assert.deepEqual(environmentList(undefined), []);
  const adapter = adapterConfiguration(
    {
      type: "aura",
      request: "launch",
      name: "Debug",
      program: "/w/app.au",
      args: ["x", 3],
      env: { MODE: "debug" },
      initCommands: ["settings set x y"]
    },
    "/o/app",
    "/ext/debug/aura_lldb.py"
  );
  assert.equal(adapter.program, "/o/app");
  assert.equal(adapter.auraProgram, "/w/app.au");
  assert.deepEqual(adapter.args, ["x", "3"]);
  assert.deepEqual(adapter.env, ["MODE=debug"]);
  assert.equal(adapter.cwd, "/w");
  assert.deepEqual(adapter.initCommands, [
    'command script import "/ext/debug/aura_lldb.py"'
  ]);
  const withCwd = adapterConfiguration({ program: "/w/app.au", cwd: "/elsewhere" }, "/o/app", "/f.py");
  assert.equal(withCwd.cwd, "/elsewhere");
  assert.deepEqual(withCwd.args, []);
  assert.deepEqual(withCwd.initCommands, ['command script import "/f.py"']);
  const windowsFormatter = adapterConfiguration(
    { program: "C:\\w\\app.au" },
    "C:\\o\\app.exe",
    'C:\\Users\\dev\\"ext"\\debug\\aura_lldb.py'
  );
  assert.deepEqual(windowsFormatter.initCommands, [
    'command script import "C:\\\\Users\\\\dev\\\\\\"ext\\"\\\\debug\\\\aura_lldb.py"'
  ]);
});

test("adapterConfiguration forwards only documented Aura fields and drops LLDB command arrays", () => {
  const adapter = adapterConfiguration(
    {
      type: "aura",
      request: "launch",
      name: "Debug",
      program: "/w/app.au",
      args: ["one"],
      cwd: "/w/sub",
      env: { K: "V" },
      stopOnEntry: true,
      initCommands: ["platform shell id"],
      preRunCommands: ["platform shell id"],
      launchCommands: ["platform shell id"],
      postRunCommands: ["platform shell id"],
      attachCommands: ["platform shell id"],
      exitCommands: ["platform shell id"],
      terminateCommands: ["platform shell id"],
      unknownField: "ignored"
    },
    "/o/app",
    "/ext/debug/aura_lldb.py"
  );
  assert.deepEqual(adapter, {
    type: "aura",
    request: "launch",
    name: "Debug",
    program: "/o/app",
    args: ["one"],
    cwd: "/w/sub",
    env: ["K=V"],
    stopOnEntry: true,
    initCommands: ['command script import "/ext/debug/aura_lldb.py"'],
    auraProgram: "/w/app.au"
  });
});

test("F5 with no launch.json debugs the open Aura file", () => {
  assert.deepEqual(defaultConfiguration({ languageId: "aura" }), {
    type: "aura",
    request: "launch",
    name: "Debug Aura file",
    program: "${file}"
  });
  assert.equal(defaultConfiguration({ languageId: "python" }), null);
  assert.equal(defaultConfiguration(undefined), null);
});

function fakeVscode({ settings = {}, activeDocument = null, textDocuments = [] } = {}) {
  const errors = [];
  const warnings = [];
  const started = [];
  const registered = {};
  const lines = [];
  let shown = 0;
  const vscode = {
    Uri: { file: (fsPath) => ({ fsPath }) },
    DebugAdapterExecutable: class {
      constructor(command, args) {
        Object.assign(this, { command, args });
      }
    },
    window: {
      activeTextEditor: activeDocument ? { document: activeDocument } : null,
      showErrorMessage: (message) => errors.push(message),
      showWarningMessage: (message) => warnings.push(message),
      createOutputChannel: () => ({
        append: (text) => lines.push(text),
        appendLine: (text) => lines.push(`${text}\n`),
        show: () => {
          shown += 1;
        },
        dispose() {}
      })
    },
    workspace: {
      textDocuments,
      getConfiguration: () => ({
        get: (key, fallback) => (key in settings ? settings[key] : fallback)
      }),
      getWorkspaceFolder: (uri) => ({ name: "w", uri })
    },
    commands: {
      registerCommand(name, handler) {
        registered[name] = handler;
        return { dispose() {} };
      }
    },
    debug: {
      registerDebugConfigurationProvider(type, provider) {
        registered.provider = { type, provider };
        return { dispose() {} };
      },
      registerDebugAdapterDescriptorFactory(type, factory) {
        registered.factory = { type, factory };
        return { dispose() {} };
      },
      startDebugging: async (folder, configuration) => {
        started.push({ folder, configuration });
        return true;
      }
    }
  };
  return { vscode, errors, warnings, started, registered, lines, shown: () => shown };
}

function fakeContext(storage) {
  return {
    subscriptions: [],
    globalStorageUri: { fsPath: storage },
    asAbsolutePath: (relative) => path.join("/ext", relative)
  };
}

test("the debugger registers its provider, factory, and command", () => {
  const fake = fakeVscode();
  const context = fakeContext("/store");
  registerDebugger(fake.vscode, context);
  assert.equal(context.subscriptions.length, 4);
  assert.equal(fake.registered.provider.type, "aura");
  assert.equal(fake.registered.factory.type, "aura");
  assert.equal(typeof fake.registered["aura.debugFile"], "function");
});

test("an empty configuration debugs the active Aura file", () => {
  const fake = fakeVscode({ activeDocument: { languageId: "aura" } });
  const { provider } = registerDebugger(fake.vscode, fakeContext("/store"));
  assert.equal(provider.resolveDebugConfiguration(undefined, {}).program, "${file}");
  const kept = { type: "aura", request: "launch", name: "Mine", program: "/w/a.au" };
  assert.equal(provider.resolveDebugConfiguration(undefined, kept), kept);

  const none = fakeVscode();
  const { provider: other } = registerDebugger(none.vscode, fakeContext("/store"));
  const empty = {};
  assert.equal(other.resolveDebugConfiguration(undefined, empty), empty);
});

test("a launch builds the program with --debug and starts lldb-dap on it", async () => {
  const storage = fs.mkdtempSync(path.join(os.tmpdir(), "aura-debug-store-"));
  try {
    const program = path.join(storage, "app.au");
    fs.writeFileSync(program, "def main():\n    pass\n");
    const dirty = {
      uri: { fsPath: program },
      isDirty: true,
      saved: 0,
      async save() {
        this.saved += 1;
      }
    };
    const fake = fakeVscode({
      settings: { compilerPath: " /opt/aura " },
      textDocuments: [{ uri: null }, dirty]
    });
    const calls = [];
    const { provider } = registerDebugger(fake.vscode, fakeContext(storage), {
      runFile: (command, args, options, done) => {
        calls.push({ command, args, options });
        done(null, "", "aura: built\n");
      }
    });
    const resolved = await provider.resolveDebugConfigurationWithSubstitutedVariables(undefined, {
      type: "aura",
      request: "launch",
      name: "Debug",
      program
    });
    const binary = debugBinaryPath(program, storage);
    assert.equal(dirty.saved, 1);
    assert.deepEqual(calls, [
      {
        command: "/opt/aura",
        args: ["build", "--debug", "-o", binary, "--", program],
        options: {
          cwd: path.dirname(program),
          maxBuffer: 4 * 1024 * 1024,
          timeout: 120_000
        }
      }
    ]);
    assert.equal(fs.statSync(path.dirname(binary)).isDirectory(), true);
    assert.equal(resolved.program, binary);
    assert.equal(
      resolved.initCommands[0],
      `command script import "${path.join("/ext", "debug", "aura_lldb.py")}"`
    );
    assert.ok(fake.lines.join("").includes("aura: built"));
    assert.deepEqual(fake.errors, []);
  } finally {
    fs.rmSync(storage, { recursive: true, force: true });
  }
});

test("a failed debug build stops the launch with the compiler's message and avoids Windows CWD planting", async () => {
  const storage = fs.mkdtempSync(path.join(os.tmpdir(), "aura-debug-store-"));
  try {
    const program = path.join(storage, "app.au");
    fs.writeFileSync(program, "def main():\n    pass\n");
    const fake = fakeVscode();
    const { provider } = registerDebugger(fake.vscode, fakeContext(storage), {
      platform: "linux",
      pathValue: "",
      home: path.join(storage, "empty-home"),
      systemDirs: [],
      runFile: (command, _args, options, done) => {
        assert.equal(command, "aura");
        assert.equal(options.cwd, undefined);
        done(new Error("exit 1"), "", "error[AU2001]: unknown name\n");
      }
    });
    const resolved = await provider.resolveDebugConfigurationWithSubstitutedVariables(undefined, {
      program
    });
    assert.equal(resolved, undefined);
    assert.deepEqual(fake.errors, ["Aura: the debug build failed. error[AU2001]: unknown name"]);
    assert.equal(fake.shown(), 1);

    const winMissing = fakeVscode();
    let winSpawned = false;
    const { provider: winProvider } = registerDebugger(winMissing.vscode, fakeContext(storage), {
      platform: "win32",
      pathValue: "",
      home: path.join(storage, "empty-home"),
      systemDirs: [],
      isExistingFile: () => true,
      runFile: () => {
        winSpawned = true;
      }
    });
    assert.equal(
      await winProvider.resolveDebugConfigurationWithSubstitutedVariables(undefined, {
        program: "C:\\untrusted\\app.au"
      }),
      undefined
    );
    assert.equal(winSpawned, false);
    assert.match(winMissing.errors[0], /compilerPath/);

    const silent = fakeVscode();
    const { provider: quiet } = registerDebugger(silent.vscode, fakeContext(storage), {
      platform: "linux",
      pathValue: "",
      home: path.join(storage, "empty-home"),
      systemDirs: [],
      runFile: (_command, _args, _options, done) => done(new Error("spawn aura ENOENT"))
    });
    assert.equal(
      await quiet.resolveDebugConfigurationWithSubstitutedVariables(undefined, { program }),
      undefined
    );
    assert.deepEqual(silent.errors, ["Aura: the debug build failed. spawn aura ENOENT"]);
  } finally {
    fs.rmSync(storage, { recursive: true, force: true });
  }
});

test("a launch with no program asks for one", async () => {
  const fake = fakeVscode();
  const { provider } = registerDebugger(fake.vscode, fakeContext("/store"));
  assert.equal(await provider.resolveDebugConfigurationWithSubstitutedVariables(undefined, {}), undefined);
  assert.deepEqual(fake.errors, ["Aura: the debug configuration needs a `program` .au file."]);
});

test("the adapter factory starts the lldb-dap it finds", () => {
  const fake = fakeVscode({ settings: { lldbDapPath: " /opt/lldb-dap " } });
  const { factory } = registerDebugger(fake.vscode, fakeContext("/store"));
  const descriptor = factory.createDebugAdapterDescriptor();
  assert.equal(descriptor.command, "/opt/lldb-dap");
  assert.deepEqual(descriptor.args, []);

  const missing = fakeVscode();
  const { factory: none } = registerDebugger(missing.vscode, fakeContext("/store"));
  const previous = process.env.PATH;
  process.env.PATH = "";
  try {
    if (process.platform !== "darwin") {
      assert.equal(none.createDebugAdapterDescriptor(), undefined);
      assert.deepEqual(missing.errors, [LLDB_DAP_MISSING]);
    }
  } finally {
    process.env.PATH = previous;
  }
});

test("Debug File debugs the given or active file", async () => {
  const fake = fakeVscode({ activeDocument: { uri: { fsPath: "/w/active.au" } } });
  const { debugFile } = registerDebugger(fake.vscode, fakeContext("/store"));
  assert.equal(await debugFile({ fsPath: "/w/given.au" }), true);
  assert.equal(await debugFile(), true);
  assert.deepEqual(
    fake.started.map(({ folder, configuration }) => [folder.uri.fsPath, configuration]),
    [
      [
        "/w/given.au",
        { type: "aura", request: "launch", name: "Debug given.au", program: "/w/given.au" }
      ],
      [
        "/w/active.au",
        { type: "aura", request: "launch", name: "Debug active.au", program: "/w/active.au" }
      ]
    ]
  );

  const empty = fakeVscode();
  const { debugFile: nothing } = registerDebugger(empty.vscode, fakeContext("/store"));
  assert.equal(await nothing(), false);
  assert.deepEqual(empty.warnings, ["Aura: open an .au file first."]);
});

test("adapterConfiguration escapes backticks and rejects newlines or NUL bytes in formatter path", () => {
  const escaped = adapterConfiguration(
    { program: "/w/app.au" },
    "/store/debug-builds/app",
    "/ext/`demo`/aura_lldb.py"
  );
  assert.deepEqual(escaped.initCommands, [
    'command script import "/ext/\\`demo\\`/aura_lldb.py"'
  ]);
  for (const invalid of [
    "/ext/line1\nline2/aura_lldb.py",
    "/ext/line1\rline2/aura_lldb.py",
    "/ext/null\0byte/aura_lldb.py"
  ]) {
    assert.throws(
      () => adapterConfiguration({ program: "/w/app.au" }, "/store/debug-builds/app", invalid),
      /control characters/
    );
  }
});

test("aura_lldb.py render tolerates malformed escape sequences without raising", () => {
  const { execFileSync } = require("node:child_process");
  const scriptPath = path.join(__dirname, "..", "debug", "aura_lldb.py");
  const py = [
    "import importlib.util, sys, types",
    "fake_lldb = types.SimpleNamespace(",
    "    eSymbolTypeCode=1, LLDB_INVALID_ADDRESS=0xffffffffffffffff, eLanguageTypeC=1,",
    "    SBExpressionOptions=lambda: types.SimpleNamespace(",
    "        SetTimeoutInMicroSeconds=lambda _v: None,",
    "        SetTryAllThreads=lambda _v: None,",
    "        SetUnwindOnError=lambda _v: None,",
    "        SetIgnoreBreakpoints=lambda _v: None,",
    "        SetLanguage=lambda _v: None,",
    "    )",
    ")",
    "sys.modules['lldb'] = fake_lldb",
    `spec = importlib.util.spec_from_file_location('aura_lldb', ${JSON.stringify(scriptPath)})`,
    "mod = importlib.util.module_from_spec(spec)",
    "spec.loader.exec_module(mod)",
    "sym = types.SimpleNamespace(IsValid=lambda: True, GetName=lambda: 'aura_debug_render', GetStartAddress=lambda: types.SimpleNamespace(GetLoadAddress=lambda _t: 0x1000))",
    "target = types.SimpleNamespace(FindSymbols=lambda _n, _k: [types.SimpleNamespace(GetSymbol=lambda: sym)])",
    "expr_res = types.SimpleNamespace(GetError=lambda: types.SimpleNamespace(Success=lambda: True), GetSummary=lambda: '\"\\\\xZZ\"')",
    "frame = types.SimpleNamespace(IsValid=lambda: True, EvaluateExpression=lambda _e, _o: expr_res)",
    "val = types.SimpleNamespace(GetNonSyntheticValue=lambda: types.SimpleNamespace(GetChildMemberWithName=lambda _n: types.SimpleNamespace(GetValueAsUnsigned=lambda _d: 0x2000)), GetFrame=lambda: frame, GetTarget=lambda: target)",
    "out = mod.render(val, {})",
    "assert out == '\\\\xZZ', repr(out)"
  ].join("\n");
  execFileSync("python3", ["-c", py], { encoding: "utf8" });
});

