"use strict";

const vscode = require("vscode");
const path = require("node:path");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");
const { computeAuraNewlineIndent } = require("./indentation");

let client;

// `aura.compilerPath` reaches the server as `AURA_LSP_AURA_PATH`, which wins
// over `aura` on PATH.
function serverEnvironment(configuredPath) {
  const env = { ...process.env };
  if (configuredPath) {
    env.AURA_LSP_AURA_PATH = configuredPath;
  }
  return env;
}

function createClient(context) {
  const serverModule = context.asAbsolutePath(path.join("dist", "server.js"));
  const configuredPath = vscode.workspace.getConfiguration("aura").get("compilerPath", "").trim();
  const env = serverEnvironment(configuredPath);
  const serverOptions = {
    run: { module: serverModule, transport: TransportKind.ipc, options: { env } },
    debug: {
      module: serverModule,
      transport: TransportKind.ipc,
      options: { env, execArgv: ["--nolazy", "--inspect=6010"] }
    }
  };

  const clientOptions = {
    documentSelector: [
      { language: "aura", scheme: "file" },
      { language: "aura", scheme: "untitled" }
    ],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*.au")
    }
  };

  return new LanguageClient("auraLanguageServer", "Aura Language Server", serverOptions, clientOptions);
}

async function restartClient(context) {
  if (client) {
    const previous = client;
    client = null;
    await previous.stop().catch(() => {});
  }
  client = createClient(context);
  await client.start();
}

async function activate(context) {
  client = createClient(context);

  context.subscriptions.push(
    vscode.commands.registerCommand("aura.restartLanguageServer", () => restartClient(context)),
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration("aura.compilerPath")) {
        void restartClient(context);
      }
    }),
    { dispose: () => client && client.stop() }
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("type", async (args) => {
      const activeEditor = vscode.window.activeTextEditor;
      if (!activeEditor || activeEditor.document.languageId !== "aura") {
        await vscode.commands.executeCommand("default:type", args);
        return;
      }

      if (args.text !== "\n" && args.text !== "\r\n") {
        await vscode.commands.executeCommand("default:type", args);
        return;
      }

      if (activeEditor.selections.some((selection) => !selection.isEmpty)) {
        await vscode.commands.executeCommand("default:type", args);
        return;
      }

      const document = activeEditor.document;
      const eol = document.eol === vscode.EndOfLine.CRLF ? "\r\n" : "\n";
      const indentUnit = getIndentUnit(activeEditor);
      const newSelections = activeEditor.selections.map((selection) => {
        const lineText = document.lineAt(selection.active.line).text;
        const precedingLines = [];
        for (let line = 0; line < selection.active.line; line += 1) {
          precedingLines.push(document.lineAt(line).text);
        }
        const indent = computeAuraNewlineIndent(
          lineText,
          selection.active.character,
          indentUnit,
          precedingLines
        );
        return {
          selection,
          indent,
          text: `${eol}${indent}`
        };
      });

      // Every inserted line break moves each later cursor down one more line.
      const documentOrder = [...newSelections].sort(
        (left, right) =>
          left.selection.active.line - right.selection.active.line ||
          left.selection.active.character - right.selection.active.character
      );
      documentOrder.forEach((entry, earlierBreaks) => {
        const newPosition = new vscode.Position(
          entry.selection.active.line + 1 + earlierBreaks,
          entry.indent.length
        );
        entry.cursor = new vscode.Selection(newPosition, newPosition);
      });

      await activeEditor.edit(
        (editBuilder) => {
          for (const entry of newSelections) {
            editBuilder.replace(entry.selection, entry.text);
          }
        },
        { undoStopBefore: true, undoStopAfter: true }
      );

      activeEditor.selections = newSelections.map((entry) => entry.cursor);
    })
  );

  await client.start();
}

function getIndentUnit(editor) {
  const tabSize = Number(editor.options.tabSize) || 4;
  if (editor.options.insertSpaces === false) {
    return "\t";
  }
  return " ".repeat(tabSize);
}

async function deactivate() {
  if (client) {
    const currentClient = client;
    client = null;
    await currentClient.stop();
  }
}

module.exports = {
  activate,
  deactivate,
  serverEnvironment
};
