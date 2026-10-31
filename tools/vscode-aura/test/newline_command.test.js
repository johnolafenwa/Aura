"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const Module = require("node:module");
const path = require("node:path");

const extensionPath = path.resolve(__dirname, "..", "src", "extension.js");

class Position {
  constructor(line, character) {
    this.line = line;
    this.character = character;
  }
}

class Selection {
  constructor(anchor, active) {
    this.anchor = anchor;
    this.active = active;
  }

  get isEmpty() {
    return (
      this.anchor.line === this.active.line && this.anchor.character === this.active.character
    );
  }
}

function createFakeEditor(lines, cursors) {
  const document = {
    languageId: "aura",
    eol: 1,
    lines: [...lines],
    lineAt(line) {
      return { text: this.lines[line] };
    },
    getText() {
      return this.lines.join("\n");
    }
  };
  const editor = {
    document,
    options: { tabSize: 4, insertSpaces: true },
    selections: cursors.map(
      ([line, character]) =>
        new Selection(new Position(line, character), new Position(line, character))
    ),
    async edit(callback) {
      const replacements = [];
      callback({
        replace(selection, text) {
          replacements.push({ position: selection.active, text });
        }
      });
      // Apply every replacement against the original document, last first,
      // the way VS Code applies one multi-range edit.
      replacements.sort(
        (left, right) =>
          right.position.line - left.position.line ||
          right.position.character - left.position.character
      );
      for (const { position, text } of replacements) {
        const current = document.lines[position.line];
        const inserted = `${current.slice(0, position.character)}${text}${current.slice(
          position.character
        )}`;
        document.lines.splice(position.line, 1, ...inserted.split("\n"));
      }
      return true;
    }
  };
  return editor;
}

async function activateWithEditor(editor, settings = {}) {
  const commands = new Map();
  const executed = [];
  const clients = [];
  const configurationListeners = [];
  const fakeVscode = {
    Position,
    Selection,
    EndOfLine: { LF: 1, CRLF: 2 },
    commands: {
      registerCommand(name, handler) {
        commands.set(name, handler);
        return { dispose() {} };
      },
      async executeCommand(...args) {
        executed.push(args);
      }
    },
    window: {
      activeTextEditor: editor,
      onDidCloseTerminal: () => ({ dispose() {} }),
      createOutputChannel: () => ({ dispose() {} })
    },
    languages: { registerCodeLensProvider: () => ({ dispose() {} }) },
    debug: {
      registerDebugConfigurationProvider: () => ({ dispose() {} }),
      registerDebugAdapterDescriptorFactory: () => ({ dispose() {} })
    },
    workspace: {
      createFileSystemWatcher: () => ({ dispose() {} }),
      getConfiguration: () => ({
        get: (key, fallback) => (key in settings ? settings[key] : fallback)
      }),
      onDidChangeConfiguration(listener) {
        configurationListeners.push(listener);
        return { dispose() {} };
      }
    }
  };
  class FakeLanguageClient {
    constructor(id, name, serverOptions, clientOptions) {
      Object.assign(this, { serverOptions, clientOptions, started: false, stopped: false });
      clients.push(this);
    }
    async start() {
      this.started = true;
    }
    async stop() {
      this.stopped = true;
    }
    dispose() {}
  }
  const originalLoad = Module._load;
  delete require.cache[extensionPath];
  Module._load = function load(request, parent, isMain) {
    if (request === "vscode") {
      return fakeVscode;
    }
    if (request === "vscode-languageclient/node") {
      return { LanguageClient: FakeLanguageClient, TransportKind: { ipc: 1 } };
    }
    return originalLoad.call(this, request, parent, isMain);
  };
  let extension;
  try {
    extension = require(extensionPath);
  } finally {
    Module._load = originalLoad;
  }
  await extension.activate({
    subscriptions: [],
    globalStorageUri: { fsPath: path.join(__dirname, "..", ".storage") },
    asAbsolutePath: (relative) => path.join(__dirname, "..", relative)
  });
  return { type: commands.get("type"), commands, executed, clients, configurationListeners };
}

function cursorPositions(editor) {
  return editor.selections.map((selection) => [selection.active.line, selection.active.character]);
}

test("Enter with several cursors places each cursor on its own inserted line", async () => {
  const editor = createFakeEditor(["x = 1", "y = 2"], [
    [0, 5],
    [1, 5]
  ]);
  const { type, executed } = await activateWithEditor(editor);

  await type({ text: "\n" });

  assert.deepEqual(executed, []);
  assert.deepEqual(editor.document.lines, ["x = 1", "", "y = 2", ""]);
  assert.deepEqual(cursorPositions(editor), [
    [1, 0],
    [3, 0]
  ]);
});

test("Enter keeps cursor order and indentation when cursors were added bottom-up", async () => {
  const editor = createFakeEditor(["def first():", "    pass", "def second():", "    pass"], [
    [2, 13],
    [0, 12]
  ]);
  const { type } = await activateWithEditor(editor);

  await type({ text: "\n" });

  assert.deepEqual(editor.document.lines, [
    "def first():",
    "    ",
    "    pass",
    "def second():",
    "    ",
    "    pass"
  ]);
  assert.deepEqual(cursorPositions(editor), [
    [4, 4],
    [1, 4]
  ]);
});

test("Enter with two cursors on one line splits the line at both cursors", async () => {
  const editor = createFakeEditor(["abc"], [
    [0, 1],
    [0, 2]
  ]);
  const { type } = await activateWithEditor(editor);

  await type({ text: "\n" });

  assert.deepEqual(editor.document.lines, ["a", "b", "c"]);
  assert.deepEqual(cursorPositions(editor), [
    [1, 0],
    [2, 0]
  ]);
});

test("aura.compilerPath reaches the server and a change restarts it", async () => {
  const settings = { compilerPath: "~/.local/bin/aura" };
  const { commands, clients, configurationListeners } = await activateWithEditor(
    createFakeEditor(["x"], [[0, 1]]),
    settings
  );
  assert.equal(clients.length, 1);
  assert.equal(clients[0].serverOptions.run.options.env.AURA_LSP_AURA_PATH, "~/.local/bin/aura");
  assert.deepEqual(
    clients[0].clientOptions.documentSelector.map((selector) => selector.scheme),
    ["file", "untitled"]
  );

  settings.compilerPath = "/opt/aura/bin/aura";
  configurationListeners[0]({ affectsConfiguration: (section) => section === "aura.compilerPath" });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(clients.length, 2);
  assert.equal(clients[0].stopped, true);
  assert.equal(clients[1].started, true);
  assert.equal(clients[1].serverOptions.run.options.env.AURA_LSP_AURA_PATH, "/opt/aura/bin/aura");

  configurationListeners[0]({ affectsConfiguration: () => false });
  await commands.get("aura.restartLanguageServer")();
  assert.equal(clients.length, 3);
  assert.equal(clients[1].stopped, true);
});
