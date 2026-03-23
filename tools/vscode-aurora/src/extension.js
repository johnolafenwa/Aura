"use strict";

const vscode = require("vscode");
const path = require("node:path");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");
const { computeAuroraNewlineIndent } = require("./indentation");

let client;

function activate(context) {
  const serverModule = context.asAbsolutePath(path.join("dist", "server.js"));
  const serverOptions = {
    run: { module: serverModule, transport: TransportKind.ipc },
    debug: {
      module: serverModule,
      transport: TransportKind.ipc,
      options: { execArgv: ["--nolazy", "--inspect=6010"] }
    }
  };

  const clientOptions = {
    documentSelector: [{ language: "aurora", scheme: "file" }],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*.au")
    }
  };

  client = new LanguageClient(
    "auroraLanguageServer",
    "Aurora Language Server",
    serverOptions,
    clientOptions
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("type", async (args) => {
      const activeEditor = vscode.window.activeTextEditor;
      if (!activeEditor || activeEditor.document.languageId !== "aurora") {
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
        const indent = computeAuroraNewlineIndent(lineText, selection.active.character, indentUnit);
        const newPosition = new vscode.Position(selection.active.line + 1, indent.length);
        return {
          selection,
          text: `${eol}${indent}`,
          cursor: new vscode.Selection(newPosition, newPosition)
        };
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

  context.subscriptions.push(client.start());
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
  deactivate
};
