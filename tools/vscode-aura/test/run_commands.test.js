"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const path = require("node:path");

const {
  buildOutputPath,
  commandLine,
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

test("command lines quote paths for the shell", () => {
  assert.equal(quoteArgument("/tmp/it's here.au", "linux"), "'/tmp/it'\\''s here.au'");
  assert.equal(quoteArgument('C:\\a "b".au', "win32"), '"C:\\a ""b"".au"');
  assert.equal(buildOutputPath("/work/app/main.au", "linux"), path.join("/work/app", "main"));
  assert.equal(buildOutputPath("/work/app/main.au", "win32"), path.join("/work/app", "main.exe"));
  assert.equal(commandLine("run", "aura", "/w/main.au"), "aura run '/w/main.au'");
  assert.equal(
    commandLine("build", "aura", "/w/main.au"),
    `aura build -o ${quoteArgument(buildOutputPath("/w/main.au"))} '/w/main.au'`
  );
  assert.equal(commandLine("test", "aura", "/w/main.au"), "aura test '/w/main.au'");
});

test("main definitions get a Run lens", () => {
  assert.deepEqual(mainDefinitionLines("import io\n\ndef main() -> int32:\n    def main_helper():\n"), [2]);
  const fake = fakeVscode();
  const { codeLensProvider } = registerRunCommands(fake.vscode, { subscriptions: [] });
  assert.deepEqual(fake.codeLensSelector(), { language: "aura", scheme: "file" });
  const lenses = codeLensProvider.provideCodeLenses(document("/w/main.au"));
  assert.equal(lenses.length, 1);
  assert.equal(lenses[0].range.startLine, 0);
  assert.deepEqual(lenses[0].command.arguments, [{ fsPath: "/w/main.au" }]);
  assert.equal(lenses[0].command.command, "aura.runFile");
});

test("run, build, and test commands save the file and use one Aura terminal", async () => {
  const active = document("/w/app.au", { dirty: true });
  const fake = fakeVscode({ activeDocument: active, settings: { compilerPath: " /opt/aura bin/aura " } });
  const context = { subscriptions: [] };
  registerRunCommands(fake.vscode, context);
  assert.equal(context.subscriptions.length, 6);

  assert.equal(await fake.commands.get("aura.runFile")(), "'/opt/aura bin/aura' run '/w/app.au'");
  assert.equal(active.saved, 1);
  assert.match(await fake.commands.get("aura.buildFile")({ fsPath: "/w/other.au" }), / build -o /);
  assert.match(await fake.commands.get("aura.testFile")(), / test '\/w\/app.au'$/);
  assert.equal(fake.terminals.length, 1);
  assert.equal(fake.terminals[0].options.name, "Aura");
  assert.equal(fake.terminals[0].sent.length, 3);

  // A closed or exited terminal is replaced.
  fake.close({ other: true });
  fake.close(fake.terminals[0]);
  await fake.commands.get("aura.runFile")();
  assert.equal(fake.terminals.length, 2);
  fake.terminals[1].exitStatus = { code: 0 };
  await fake.commands.get("aura.runFile")();
  assert.equal(fake.terminals.length, 3);
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
  const withFolder = fakeVscode({ workspaceFolders: [{ uri: { fsPath: "/w" } }] });
  registerRunCommands(withFolder.vscode, { subscriptions: [] });
  assert.equal(await withFolder.commands.get("aura.runTests")(), "aura test '/w'");
  const withoutFolder = fakeVscode();
  registerRunCommands(withoutFolder.vscode, { subscriptions: [] });
  assert.equal(await withoutFolder.commands.get("aura.runTests")(), "aura test");
});
