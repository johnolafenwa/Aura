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

async function activateWithEditor(editor) {
  const commands = new Map();
  const executed = [];
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
    window: { activeTextEditor: editor },
    workspace: { createFileSystemWatcher: () => ({ dispose() {} }) }
  };
  class FakeLanguageClient {
    async start() {}
    async stop() {}
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
    asAbsolutePath: (relative) => path.join(__dirname, "..", relative)
  });
  return { type: commands.get("type"), executed };
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
