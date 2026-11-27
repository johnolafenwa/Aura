"use strict";

const os = require("node:os");
const path = require("node:path");
const { expandHome, findDebugCompiler } = require("./debug");

const MAIN_DEFINITION = /^def\s+main\s*\(/;
const ASCII_CONTROL_CHARS = /[\x00-\x1f\x7f]/;

function hasControlCharacters(value) {
  return typeof value !== "string" || ASCII_CONTROL_CHARS.test(value);
}

// Literal single-quote quoting for display strings; execution always passes
// argument arrays via ProcessExecution or direct terminal shellPath/shellArgs.
function quoteArgument(value, platform = process.platform) {
  if (hasControlCharacters(value)) {
    throw new Error("Aura: command argument contains control characters.");
  }
  if (platform === "win32") {
    return `'${value.replace(/'/g, "''")}'`;
  }
  return `'${value.replace(/'/g, "'\\''")}'`;
}

function buildOutputPath(filePath, platform = process.platform) {
  const parsed = path.parse(filePath);
  const safeName =
    !parsed.name || parsed.name === "." || parsed.name === ".." || /[/\\]/.test(parsed.name)
      ? "aura_out"
      : parsed.name;
  return path.join(parsed.dir, platform === "win32" ? `${safeName}.exe` : safeName);
}

function resolveCompilerExecutable(vscode, options = {}) {
  const configured = vscode.workspace.getConfiguration("aura").get("compilerPath", "").trim();
  const resolveOptions = {
    configured,
    platform: options.platform ?? process.platform,
    pathValue: options.pathValue ?? (process.env.PATH || ""),
    home: options.home ?? os.homedir(),
    systemDirs: options.systemDirs ?? ["/opt/homebrew/bin", "/usr/local/bin"]
  };
  if (options.executable) {
    resolveOptions.executable = options.executable;
  }
  return findDebugCompiler(resolveOptions);
}

function compilerCommand(vscode, platform = process.platform, options = {}) {
  const configured = vscode.workspace.getConfiguration("aura").get("compilerPath", "").trim();
  if (!configured) {
    return "aura";
  }
  const expanded = expandHome(configured, options.home ?? os.homedir(), platform);
  const quoted = quoteArgument(expanded, platform);
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

function formatExecutable(executable, platform = process.platform) {
  const quoted = quoteArgument(executable, platform);
  return platform === "win32" ? `& ${quoted}` : quoted;
}

function formatCommandLine(executable, args, platform = process.platform) {
  const formattedArgs = args.map((arg, index) =>
    index === 0 || arg === "-o" ? arg : quoteArgument(arg, platform)
  );
  return `${formatExecutable(executable, platform)} ${formattedArgs.join(" ")}`;
}

// Formatted command preview for a saved file.
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

function notifyError(vscode, message) {
  if (vscode.window && typeof vscode.window.showErrorMessage === "function") {
    vscode.window.showErrorMessage(message);
  } else if (vscode.window && typeof vscode.window.showWarningMessage === "function") {
    vscode.window.showWarningMessage(message);
  }
}

function registerRunCommands(vscode, context, options = {}) {
  const platform = options.platform ?? process.platform;
  let terminal = null;

  function canExecuteStructuredTask() {
    return Boolean(
      vscode.tasks &&
        typeof vscode.tasks.executeTask === "function" &&
        typeof vscode.Task === "function" &&
        typeof vscode.ProcessExecution === "function"
    );
  }

  async function executeAuraCommand(name, args) {
    const executable = resolveCompilerExecutable(vscode, { ...options, platform });
    if (!executable) {
      notifyError(
        vscode,
        'Aura: could not find the `aura` compiler. Install Aura or set "aura.compilerPath" to an absolute executable path.'
      );
      return null;
    }
    if (hasControlCharacters(executable) || args.some(hasControlCharacters)) {
      notifyError(
        vscode,
        "Aura: refusing to run a command whose compiler path or arguments contain control characters."
      );
      return null;
    }
    const displayLine = formatCommandLine(executable, args, platform);
    if (canExecuteStructuredTask()) {
      const scope = (vscode.TaskScope && vscode.TaskScope.Workspace) || 2;
      const task = new vscode.Task(
        { type: "aura", command: name },
        scope,
        name,
        "Aura",
        new vscode.ProcessExecution(executable, args)
      );
      await vscode.tasks.executeTask(task);
      return displayLine;
    }
    const target = vscode.window.createTerminal({
      name: "Aura",
      shellPath: executable,
      shellArgs: args
    });
    terminal = target;
    target.show(true);
    return displayLine;
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
    if (hasControlCharacters(document.uri.fsPath)) {
      notifyError(
        vscode,
        "Aura: refusing to run a command whose compiler path or arguments contain control characters."
      );
      return null;
    }
    if (document.isDirty) {
      await document.save();
    }
    return executeAuraCommand(`aura ${kind}`, commandArgs(kind, document.uri.fsPath, platform));
  }

  async function runWorkspaceTests() {
    const folder = (vscode.workspace.workspaceFolders || [])[0];
    const args = folder ? ["test", folder.uri.fsPath] : ["test"];
    return executeAuraCommand("aura test", args);
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
  hasControlCharacters,
  mainDefinitionLines,
  quoteArgument,
  registerRunCommands,
  resolveCompilerExecutable
};
