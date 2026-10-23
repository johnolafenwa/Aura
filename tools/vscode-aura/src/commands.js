"use strict";

const path = require("node:path");

const MAIN_DEFINITION = /^def\s+main\s*\(/;

// POSIX single-quote quoting; PowerShell and cmd accept double quotes.
function quoteArgument(value, platform = process.platform) {
  if (platform === "win32") {
    return `"${value.replace(/"/g, '""')}"`;
  }
  return `'${value.replace(/'/g, "'\\''")}'`;
}

function buildOutputPath(filePath, platform = process.platform) {
  const parsed = path.parse(filePath);
  return path.join(parsed.dir, platform === "win32" ? `${parsed.name}.exe` : parsed.name);
}

function compilerCommand(vscode) {
  const configured = vscode.workspace.getConfiguration("aura").get("compilerPath", "").trim();
  return configured ? quoteArgument(configured) : "aura";
}

// The shell command each editor command runs for a saved file.
function commandLine(kind, compiler, filePath) {
  const file = quoteArgument(filePath);
  switch (kind) {
    case "run":
      return `${compiler} run ${file}`;
    case "build":
      return `${compiler} build -o ${quoteArgument(buildOutputPath(filePath))} ${file}`;
    default:
      return `${compiler} test ${file}`;
  }
}

// CodeLens ranges for each module-level `def main(`.
function mainDefinitionLines(text) {
  return text
    .split("\n")
    .map((line, index) => (MAIN_DEFINITION.test(line) ? index : -1))
    .filter((index) => index >= 0);
}

function registerRunCommands(vscode, context) {
  let terminal = null;

  function auraTerminal() {
    if (!terminal || terminal.exitStatus !== undefined) {
      terminal = vscode.window.createTerminal({ name: "Aura" });
    }
    return terminal;
  }

  async function documentFor(uri) {
    if (uri && uri.fsPath) {
      return vscode.workspace.openTextDocument(uri);
    }
    const editor = vscode.window.activeTextEditor;
    return editor && editor.document.languageId === "aura" ? editor.document : null;
  }

  async function runInTerminal(kind, uri) {
    const document = await documentFor(uri);
    if (!document) {
      vscode.window.showWarningMessage("Aura: open an .au file first.");
      return null;
    }
    if (document.isUntitled) {
      vscode.window.showWarningMessage("Aura: save the file before running it.");
      return null;
    }
    if (document.isDirty) {
      await document.save();
    }
    const line = commandLine(kind, compilerCommand(vscode), document.uri.fsPath);
    const target = auraTerminal();
    target.show(true);
    target.sendText(line);
    return line;
  }

  async function runWorkspaceTests() {
    const folder = (vscode.workspace.workspaceFolders || [])[0];
    const target = auraTerminal();
    target.show(true);
    const line = folder
      ? `${compilerCommand(vscode)} test ${quoteArgument(folder.uri.fsPath)}`
      : `${compilerCommand(vscode)} test`;
    target.sendText(line);
    return line;
  }

  const codeLensProvider = {
    provideCodeLenses(document) {
      return mainDefinitionLines(document.getText()).map((line) => {
        const range = new vscode.Range(line, 0, line, 0);
        return new vscode.CodeLens(range, {
          title: "▶ Run",
          command: "aura.runFile",
          arguments: [document.uri]
        });
      });
    }
  };

  context.subscriptions.push(
    vscode.commands.registerCommand("aura.runFile", (uri) => runInTerminal("run", uri)),
    vscode.commands.registerCommand("aura.buildFile", (uri) => runInTerminal("build", uri)),
    vscode.commands.registerCommand("aura.testFile", (uri) => runInTerminal("test", uri)),
    vscode.commands.registerCommand("aura.runTests", () => runWorkspaceTests()),
    vscode.languages.registerCodeLensProvider({ language: "aura", scheme: "file" }, codeLensProvider),
    vscode.window.onDidCloseTerminal((closed) => {
      if (closed === terminal) {
        terminal = null;
      }
    })
  );
  return { codeLensProvider };
}

module.exports = {
  buildOutputPath,
  commandLine,
  mainDefinitionLines,
  quoteArgument,
  registerRunCommands
};
