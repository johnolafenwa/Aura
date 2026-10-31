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
        assert.equal(command, "xcrun");
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
      pathValue: "/llvm",
      executable: (candidate) => candidate.endsWith("lldb-dap.exe")
    }),
    path.join("/llvm", "lldb-dap.exe")
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
  assert.equal(
    debugBinaryPath("/w/app.au", "/store", "linux"),
    path.join("/store", "debug-builds", "app")
  );
  assert.equal(
    debugBinaryPath("/w/app.au", "/store", "win32"),
    path.join("/store", "debug-builds", "app.exe")
  );
  assert.deepEqual(debugBuildArguments("/w/app.au", "/o/app"), [
    "build",
    "--debug",
    "-o",
    "/o/app",
    "/w/app.au"
  ]);
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
    'command script import "/ext/debug/aura_lldb.py"',
    "settings set x y"
  ]);
  const withCwd = adapterConfiguration({ program: "/w/app.au", cwd: "/elsewhere" }, "/o/app", "/f.py");
  assert.equal(withCwd.cwd, "/elsewhere");
  assert.deepEqual(withCwd.args, []);
  assert.deepEqual(withCwd.initCommands, ['command script import "/f.py"']);
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
    const dirty = {
      uri: { fsPath: "/w/app.au" },
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
      program: "/w/app.au"
    });
    const binary = path.join(storage, "debug-builds", "app");
    assert.equal(dirty.saved, 1);
    assert.deepEqual(calls, [
      {
        command: "/opt/aura",
        args: ["build", "--debug", "-o", binary, "/w/app.au"],
        options: { cwd: "/w" }
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

test("a failed debug build stops the launch with the compiler's message", async () => {
  const storage = fs.mkdtempSync(path.join(os.tmpdir(), "aura-debug-store-"));
  try {
    const fake = fakeVscode();
    const { provider } = registerDebugger(fake.vscode, fakeContext(storage), {
      runFile: (command, _args, _options, done) => {
        assert.equal(command, "aura");
        done(new Error("exit 1"), "", "error[AU2001]: unknown name\n");
      }
    });
    const resolved = await provider.resolveDebugConfigurationWithSubstitutedVariables(undefined, {
      program: "/w/app.au"
    });
    assert.equal(resolved, undefined);
    assert.deepEqual(fake.errors, ["Aura: the debug build failed. error[AU2001]: unknown name"]);
    assert.equal(fake.shown(), 1);

    const silent = fakeVscode();
    const { provider: quiet } = registerDebugger(silent.vscode, fakeContext(storage), {
      runFile: (_command, _args, _options, done) => done(new Error("spawn aura ENOENT"))
    });
    assert.equal(
      await quiet.resolveDebugConfigurationWithSubstitutedVariables(undefined, { program: "/w/app.au" }),
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
