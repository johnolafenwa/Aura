"use strict";

const path = require("node:path");

const MAIN_DEFINITION = /^def\s+main\s*\(/;

// Literal single-quote quoting on both POSIX (`'\''`) and PowerShell (`''`),
// preventing `$()`, backtick, and `$env:` expansion in fallback terminal lines.
function quoteArgument(value, platform = process.platform) {
  if (platform === "win32") {
    return `'${value.replace(/'/g, "''")}'`;
  }
  return `'${value.replace(/'/g, "'\\''")}'`;
}

function buildOutputPath(filePath, platform = process.platform) {
  const parsed = path.parse(filePath);
  return path.join(parsed.dir, platform === "win32" ? `${parsed.name}.exe` : parsed.name);
}

function configuredCompilerExecutable(vscode) {
  return vscode.workspace.getConfiguration("aura").get("compilerPath", "").trim() || "aura";
}

function compilerCommand(vscode, platform = process.platform) {
  const configured = vscode.workspace.getConfiguration("aura").get("compilerPath", "").trim();
  if (!configured) {
    return "aura";
  }
  const quoted = quoteArgument(configured, platform);
  return platform === "win32" ? `& ${quoted}` : quoted;
}

function commandArgs(kind, filePath, platform = process.platform) {
  switch (kind) {
    case "run":
      return ["run", filePath];
    case "build":
      return ["build", "-o", buildOutputPath(filePath, platform), filePath];
    default:
      return ["test", filePath];
  }
}

// The shell command each editor command runs for a saved file.
function commandLine(kind, compiler, filePath, platform = process.platform) {
  const args = commandArgs(kind, filePath, platform).map((arg, index) =>
    index === 0 || arg === "-o" ? arg : quoteArgument(arg, platform)
  );
  return `${compiler} ${args.join(" ")}`;
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

  function canExecuteStructuredTask() {
    return Boolean(
      vscode.tasks &&
        typeof vscode.tasks.executeTask === "function" &&
        typeof vscode.Task === "function" &&
        typeof vscode.ShellExecution === "function"
    );
  }

  async function executeAuraCommand(name, args, fallbackLine) {
    if (canExecuteStructuredTask()) {
      const executable = configuredCompilerExecutable(vscode);
      const scope = (vscode.TaskScope && vscode.TaskScope.Workspace) || 2;
      const task = new vscode.Task(
        { type: "aura", command: name },
        scope,
        name,
        "Aura",
        new vscode.ShellExecution(executable, args)
      );
      await vscode.tasks.executeTask(task);
      return fallbackLine;
    }
    const target = auraTerminal();
    target.show(true);
    target.sendText(fallbackLine);
    return fallbackLine;
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
    return executeAuraCommand(
      `aura ${kind}`,
      commandArgs(kind, document.uri.fsPath),
      line
    );
  }

  async function runWorkspaceTests() {
    const folder = (vscode.workspace.workspaceFolders || [])[0];
    const args = folder ? ["test", folder.uri.fsPath] : ["test"];
    const line = folder
      ? `${compilerCommand(vscode)} test ${quoteArgument(folder.uri.fsPath)}`
      : `${compilerCommand(vscode)} test`;
    return executeAuraCommand("aura test", args, line);
  }

  const codeLensProvider = {
    provideCodeLenses(document) {
      return mainDefinitionLines(document.getText()).map((line) => {
        const range = new vscode.Range(line, 0, line, 0);
        return [
          new vscode.CodeLens(range, {
            title: "▶ Run",
            command: "aura.runFile",
            arguments: [document.uri]
          }),
          new vscode.CodeLens(range, {
            title: "Debug",
            command: "aura.debugFile",
            arguments: [document.uri]
          })
        ];
      }).flat();
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
  commandArgs,
  commandLine,
  compilerCommand,
  mainDefinitionLines,
  quoteArgument,
  registerRunCommands
};
