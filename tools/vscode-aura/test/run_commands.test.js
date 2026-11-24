"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const path = require("node:path");

const {
  buildOutputPath,
  commandArgs,
  commandLine,
  compilerCommand,
  mainDefinitionLines,
  quoteArgument,
  registerRunCommands
} = require("../src/commands");

function fakeVscode({ activeDocument = null, settings = {}, workspaceFolders } = {}) {
  const commands = new Map();
  const warnings = [];
  const terminals = [];
  let closeListener = null;
  let codeLensSelector = null;
  const vscode = {
    Range: class {
      constructor(startLine, startCharacter, endLine, endCharacter) {
        Object.assign(this, { startLine, startCharacter, endLine, endCharacter });
      }
    },
    CodeLens: class {
      constructor(range, command) {
        Object.assign(this, { range, command });
      }
    },
    commands: {
      registerCommand(name, handler) {
        commands.set(name, handler);
        return { dispose() {} };
      }
    },
    languages: {
      registerCodeLensProvider(selector) {
        codeLensSelector = selector;
        return { dispose() {} };
      }
    },
    window: {
      activeTextEditor: activeDocument ? { document: activeDocument } : null,
      showWarningMessage(message) {
        warnings.push(message);
      },
      createTerminal(options) {
        const terminal = {
          options,
          sent: [],
          shown: 0,
          exitStatus: undefined,
          show() {
            this.shown += 1;
          },
          sendText(text) {
            this.sent.push(text);
          }
        };
        terminals.push(terminal);
        return terminal;
      },
      onDidCloseTerminal(listener) {
        closeListener = listener;
        return { dispose() {} };
      }
    },
    workspace: {
      workspaceFolders,
      getConfiguration: () => ({
        get: (key, fallback) => (key in settings ? settings[key] : fallback)
      }),
      openTextDocument: async (uri) => document(uri.fsPath)
    }
  };
  return {
    vscode,
    commands,
    warnings,
    terminals,
    close: (terminal) => closeListener(terminal),
    codeLensSelector: () => codeLensSelector
  };
}

function document(fsPath, { untitled = false, dirty = false, text = "def main() -> int32:\n    return 0\n" } = {}) {
  return {
    uri: { fsPath },
    languageId: "aura",
    isUntitled: untitled,
    isDirty: dirty,
    saved: 0,
    getText: () => text,
    async save() {
      this.saved += 1;
      this.isDirty = false;
      return true;
    }
  };
}

test("command lines quote paths for the shell without PowerShell expansion", () => {
  assert.equal(quoteArgument("/tmp/it's here.au", "linux"), "'/tmp/it'\\''s here.au'");
  assert.equal(quoteArgument('C:\\a "b".au', "win32"), '\'C:\\a "b".au\'');
  assert.equal(
    quoteArgument("C:\\it's $(calc) $env:SECRET `n %TEMP%.au", "win32"),
    "'C:\\it''s $(calc) $env:SECRET `n %TEMP%.au'"
  );
  const winVscode = fakeVscode({ settings: { compilerPath: "C:\\Program Files\\Aura\\aura.exe" } }).vscode;
  assert.equal(compilerCommand(winVscode, "win32"), "& 'C:\\Program Files\\Aura\\aura.exe'");
  assert.equal(compilerCommand(fakeVscode().vscode, "win32"), "aura");
  assert.deepEqual(commandArgs("run", "/w/main.au", "linux"), ["run", "/w/main.au"]);
  assert.deepEqual(commandArgs("build", "/w/main.au", "win32"), [
    "build",
    "-o",
    path.join("/w", "main.exe"),
    "/w/main.au"
  ]);
  assert.deepEqual(commandArgs("test", "/w/main.au", "linux"), ["test", "/w/main.au"]);
  assert.equal(buildOutputPath("/work/app/main.au", "linux"), path.join("/work/app", "main"));
  assert.equal(buildOutputPath("/work/app/main.au", "win32"), path.join("/work/app", "main.exe"));
  assert.equal(commandLine("run", "aura", "/w/main.au"), "aura run '/w/main.au'");
  assert.equal(
    commandLine("build", "aura", "/w/main.au"),
    `aura build -o ${quoteArgument(buildOutputPath("/w/main.au"))} '/w/main.au'`
  );
  assert.equal(commandLine("test", "aura", "/w/main.au"), "aura test '/w/main.au'");
});

test("commands prefer structured VS Code ProcessExecution tasks when available", async () => {
  const executed = [];
  const active = document("/w/it's $(safe).au");
  const fake = fakeVscode({
    activeDocument: active,
    settings: { compilerPath: "/opt/aura/bin/aura" },
    workspaceFolders: [{ uri: { fsPath: "/w" } }]
  });
  fake.vscode.TaskScope = { Workspace: 2 };
  fake.vscode.ProcessExecution = class {
    constructor(processPath, args) {
      Object.assign(this, { process: processPath, args });
    }
  };
  fake.vscode.Task = class {
    constructor(definition, scope, name, source, execution) {
      Object.assign(this, { definition, scope, name, source, execution });
    }
  };
  fake.vscode.tasks = {
    async executeTask(task) {
      executed.push(task);
    }
  };
  registerRunCommands(fake.vscode, { subscriptions: [] });
  await fake.commands.get("aura.runFile")();
  await fake.commands.get("aura.runTests")();
  assert.equal(fake.terminals.length, 0);
  assert.equal(executed.length, 2);
  assert.equal(executed[0].execution.process, "/opt/aura/bin/aura");
  assert.deepEqual(executed[0].execution.args, ["run", "/w/it's $(safe).au"]);
  assert.equal(executed[1].execution.process, "/opt/aura/bin/aura");
  assert.deepEqual(executed[1].execution.args, ["test", "/w"]);
});

test("main definitions get Run and Debug lenses", () => {
  assert.deepEqual(mainDefinitionLines("import io\n\ndef main() -> int32:\n    def main_helper():\n"), [2]);
  const fake = fakeVscode();
  const { codeLensProvider } = registerRunCommands(fake.vscode, { subscriptions: [] });
  assert.deepEqual(fake.codeLensSelector(), { language: "aura", scheme: "file" });
  const lenses = codeLensProvider.provideCodeLenses(document("/w/main.au"));
  assert.equal(lenses.length, 2);
  assert.equal(lenses[0].range.startLine, 0);
  assert.deepEqual(lenses[0].command.arguments, [{ fsPath: "/w/main.au" }]);
  assert.equal(lenses[0].command.command, "aura.runFile");
  assert.equal(lenses[1].range.startLine, 0);
  assert.deepEqual(lenses[1].command.arguments, [{ fsPath: "/w/main.au" }]);
  assert.equal(lenses[1].command.command, "aura.debugFile");
});

test("run, build, and test commands save the file and launch direct Aura terminals", async () => {
  const active = document("/w/app.au", { dirty: true });
  const fake = fakeVscode({ activeDocument: active, settings: { compilerPath: " /opt/aura bin/aura " } });
  const context = { subscriptions: [] };
  registerRunCommands(fake.vscode, context);
  assert.equal(context.subscriptions.length, 6);

  assert.equal(await fake.commands.get("aura.runFile")(), "'/opt/aura bin/aura' run '/w/app.au'");
  assert.equal(active.saved, 1);
  assert.match(await fake.commands.get("aura.buildFile")({ fsPath: "/w/other.au" }), / build -o /);
  assert.match(await fake.commands.get("aura.testFile")(), / test '\/w\/app.au'$/);
  assert.equal(fake.terminals.length, 3);
  assert.equal(fake.terminals[0].options.name, "Aura");
  assert.equal(fake.terminals[0].options.shellPath, "/opt/aura bin/aura");
  assert.deepEqual(fake.terminals[0].options.shellArgs, ["run", "/w/app.au"]);
  assert.deepEqual(fake.terminals[0].sent, []);
  assert.deepEqual(fake.terminals[1].options.shellArgs, [
    "build",
    "-o",
    path.join("/w", "other"),
    "/w/other.au"
  ]);
  assert.deepEqual(fake.terminals[2].options.shellArgs, ["test", "/w/app.au"]);

  fake.close({ other: true });
  fake.close(fake.terminals[2]);
});

test("run commands explain what is missing instead of running", async () => {
  const none = fakeVscode();
  registerRunCommands(none.vscode, { subscriptions: [] });
  assert.equal(await none.commands.get("aura.runFile")(), null);
  assert.deepEqual(none.warnings, ["Aura: open an .au file first."]);

  const other = fakeVscode({ activeDocument: { ...document("/w/x.txt"), languageId: "plaintext" } });
  registerRunCommands(other.vscode, { subscriptions: [] });
  assert.equal(await other.commands.get("aura.runFile")(), null);

  const untitled = fakeVscode({ activeDocument: document("Untitled-1", { untitled: true }) });
  registerRunCommands(untitled.vscode, { subscriptions: [] });
  assert.equal(await untitled.commands.get("aura.runFile")(), null);
  assert.deepEqual(untitled.warnings, ["Aura: save the file before running it."]);
});

test("workspace tests run in the first folder, or in the current directory", async () => {
  const resolver = {
    platform: "linux",
    pathValue: "/usr/local/bin",
    home: "/empty-home",
    systemDirs: [],
    executable: (candidate) => candidate === "/usr/local/bin/aura"
  };
  const withFolder = fakeVscode({ workspaceFolders: [{ uri: { fsPath: "/w" } }] });
  registerRunCommands(withFolder.vscode, { subscriptions: [] }, resolver);
  assert.equal(await withFolder.commands.get("aura.runTests")(), "'/usr/local/bin/aura' test '/w'");
  const withoutFolder = fakeVscode();
  registerRunCommands(withoutFolder.vscode, { subscriptions: [] }, resolver);
  assert.equal(await withoutFolder.commands.get("aura.runTests")(), "'/usr/local/bin/aura' test");
});

test("commands use ProcessExecution with an absolute compiler path instead of ShellExecution", async () => {
  const executed = [];
  const active = document("/w/it's $(safe).au");
  const fake = fakeVscode({
    activeDocument: active,
    workspaceFolders: [{ uri: { fsPath: "/w" } }]
  });
  fake.vscode.TaskScope = { Workspace: 2 };
  fake.vscode.ShellExecution = class {
    constructor(command, args) {
      Object.assign(this, { kind: "shell", command, args });
    }
  };
  fake.vscode.ProcessExecution = class {
    constructor(processPath, args) {
      Object.assign(this, { kind: "process", process: processPath, args });
    }
  };
  fake.vscode.Task = class {
    constructor(definition, scope, name, source, execution) {
      Object.assign(this, { definition, scope, name, source, execution });
    }
  };
  fake.vscode.tasks = {
    async executeTask(task) {
      executed.push(task);
    }
  };
  registerRunCommands(fake.vscode, { subscriptions: [] }, {
    platform: "linux",
    pathValue: "/usr/local/bin",
    home: "/empty-home",
    systemDirs: [],
    executable: (candidate) => candidate === "/usr/local/bin/aura"
  });
  await fake.commands.get("aura.runFile")();
  assert.equal(executed.length, 1);
  assert.equal(executed[0].execution.kind, "process");
  assert.equal(executed[0].execution.process, "/usr/local/bin/aura");
  assert.deepEqual(executed[0].execution.args, ["run", "/w/it's $(safe).au"]);
});

test("commands refuse bare or relative compiler paths when no absolute aura executable is found", async () => {
  const executed = [];
  const errors = [];
  const active = document("/w/main.au");
  const fake = fakeVscode({ activeDocument: active });
  fake.vscode.window.showErrorMessage = (message) => errors.push(message);
  fake.vscode.TaskScope = { Workspace: 2 };
  fake.vscode.ProcessExecution = class {
    constructor(processPath, args) {
      Object.assign(this, { process: processPath, args });
    }
  };
  fake.vscode.Task = class {
    constructor(definition, scope, name, source, execution) {
      Object.assign(this, { definition, scope, name, source, execution });
    }
  };
  fake.vscode.tasks = {
    async executeTask(task) {
      executed.push(task);
    }
  };
  registerRunCommands(fake.vscode, { subscriptions: [] }, {
    platform: "win32",
    pathValue: ".",
    home: "C:\\empty-home",
    systemDirs: [],
    executable: () => false
  });
  assert.equal(await fake.commands.get("aura.runFile")(), null);
  assert.equal(executed.length, 0);
  assert.equal(fake.terminals.length, 0);
  assert.equal(errors.length, 1);

  const relative = fakeVscode({
    activeDocument: active,
    settings: { compilerPath: "aura" }
  });
  relative.vscode.window.showErrorMessage = (message) => errors.push(message);
  registerRunCommands(relative.vscode, { subscriptions: [] }, {
    platform: "linux",
    pathValue: "",
    home: "/empty-home",
    systemDirs: [],
    executable: () => false
  });
  assert.equal(await relative.commands.get("aura.runFile")(), null);
  assert.equal(relative.terminals.length, 0);
  assert.equal(errors.length, 2);
});

test("commands reject ASCII control characters in file paths, workspace folders, and compiler paths", async () => {
  for (const badPath of ["/w/a\x03b.au", "/w/a\rb.au", "/w/a\nb.au", "/w/a\0b.au", "/w/a\x1bb.au", "/w/a\x7fb.au"]) {
    assert.throws(() => quoteArgument(badPath, "linux"));
    const errors = [];
    const fake = fakeVscode({
      activeDocument: document(badPath),
      settings: { compilerPath: "/opt/aura/bin/aura" }
    });
    fake.vscode.window.showErrorMessage = (message) => errors.push(message);
    registerRunCommands(fake.vscode, { subscriptions: [] });
    assert.equal(await fake.commands.get("aura.buildFile")(), null);
    assert.equal(fake.terminals.length, 0);
    assert.equal(errors.length, 1);
  }
  const badCompiler = fakeVscode({
    activeDocument: document("/w/main.au"),
    settings: { compilerPath: "/opt/aura\n/aura" }
  });
  registerRunCommands(badCompiler.vscode, { subscriptions: [] });
  assert.equal(await badCompiler.commands.get("aura.runFile")(), null);
  assert.equal(badCompiler.terminals.length, 0);
  assert.equal(badCompiler.warnings.length, 1);

  const badFolder = fakeVscode({
    workspaceFolders: [{ uri: { fsPath: "/w/bad\x03dir" } }],
    settings: { compilerPath: "/opt/aura/bin/aura" }
  });
  registerRunCommands(badFolder.vscode, { subscriptions: [] });
  assert.equal(await badFolder.commands.get("aura.runTests")(), null);
  assert.equal(badFolder.terminals.length, 0);
  assert.equal(badFolder.warnings.length, 1);
});

test("fallback terminal launches the compiler directly with shellPath and shellArgs instead of sendText", async () => {
  const active = document("C:\\work\\a & b.au");
  const fake = fakeVscode({
    activeDocument: active,
    settings: { compilerPath: "C:\\Program Files\\Aura\\aura.exe" }
  });
  // Only ShellExecution is mocked (no ProcessExecution): must fall back to direct process terminal.
  fake.vscode.TaskScope = { Workspace: 2 };
  fake.vscode.ShellExecution = class {
    constructor(command, args) {
      Object.assign(this, { command, args });
    }
  };
  fake.vscode.Task = class {
    constructor(definition, scope, name, source, execution) {
      Object.assign(this, { definition, scope, name, source, execution });
    }
  };
  const executedTasks = [];
  fake.vscode.tasks = {
    async executeTask(task) {
      executedTasks.push(task);
    }
  };
  registerRunCommands(fake.vscode, { subscriptions: [] }, { platform: "win32" });
  const result = await fake.commands.get("aura.runFile")();
  assert.ok(result);
  assert.equal(executedTasks.length, 0);
  assert.equal(fake.terminals.length, 1);
  assert.equal(fake.terminals[0].options.shellPath, "C:\\Program Files\\Aura\\aura.exe");
  assert.deepEqual(fake.terminals[0].options.shellArgs, ["run", "C:\\work\\a & b.au"]);
  assert.deepEqual(fake.terminals[0].sent, []);
});

