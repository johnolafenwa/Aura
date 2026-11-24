"use strict";

// The `aura` debug type. A launch builds the program with
// `aura build --debug`, which writes DWARF, and hands the binary to LLVM's
// `lldb-dap`, which speaks the Debug Adapter Protocol. `debug/aura_lldb.py`
// teaches lldb to show Aura values as the program prints them.

const childProcess = require("node:child_process");
const crypto = require("node:crypto");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const DEBUG_TYPE = "aura";

// Versioned names some Linux distributions install `lldb-dap` under, newest
// first, and the name it had before LLVM 18.
const VERSIONED_ADAPTERS = Array.from({ length: 10 }, (_, index) => `lldb-dap-${24 - index}`);
const ADAPTER_NAMES = ["lldb-dap", ...VERSIONED_ADAPTERS, "lldb-vscode"];

const LLDB_DAP_MISSING =
  "Aura: the debugger needs LLVM's lldb-dap. Install Xcode's command line tools on macOS or the lldb package on Linux, or set aura.lldbDapPath.";

function isExecutable(file) {
  try {
    fs.accessSync(file, fs.constants.X_OK);
    return fs.statSync(file).isFile();
  } catch {
    return false;
  }
}

function isRegularFile(file) {
  try {
    return fs.statSync(file).isFile();
  } catch {
    return false;
  }
}

// The `lldb-dap` to start: the configured one, Xcode's on macOS, then the
// first adapter name found on PATH.
function findLldbDap({
  configured = "",
  platform = process.platform,
  pathValue = process.env.PATH || "",
  home = os.homedir(),
  executable = isExecutable,
  runFile = childProcess.execFileSync
} = {}) {
  const pathApi = platform === "win32" ? path.win32 : path.posix;
  const delimiter = platform === "win32" ? ";" : ":";
  if (configured) {
    const expanded = expandHome(configured, home, platform);
    return isAbsoluteForPlatform(expanded, platform) ? expanded : null;
  }
  if (platform === "darwin") {
    try {
      const found = String(runFile("/usr/bin/xcrun", ["-f", "lldb-dap"], { encoding: "utf8" })).trim();
      if (found) {
        return found;
      }
    } catch {
      // No Xcode tools; PATH may still have one.
    }
  }
  const directories = pathValue
    .split(delimiter)
    .filter((entry) => entry && isAbsoluteForPlatform(entry, platform));
  for (const name of ADAPTER_NAMES) {
    for (const directory of directories) {
      const candidate = pathApi.join(directory, platform === "win32" ? `${name}.exe` : name);
      if (executable(candidate)) {
        return candidate;
      }
    }
  }
  return null;
}

// Where a debug build of `program` goes: the extension's own storage
// directory, so the workspace stays clean.
function debugBinaryPath(program, directory, platform = process.platform) {
  const pathApi = platform === "win32" ? path.win32 : path.posix;
  let canonical;
  try {
    canonical = fs.realpathSync(program);
  } catch {
    canonical = pathApi.resolve(program);
  }
  const digest = crypto.createHash("sha256").update(canonical).digest("hex").slice(0, 16);
  const parsed = path.parse(program);
  const stem = parsed.name.replace(/[^A-Za-z0-9_-]+/g, "_").replace(/^_+|_+$/g, "") || "program";
  const name = platform === "win32" ? `${stem}-${digest}.exe` : `${stem}-${digest}`;
  return path.join(directory, "debug-builds", name);
}

// The `aura build` arguments of a debug build.
function debugBuildArguments(program, output) {
  return ["build", "--debug", "-o", output, "--", program];
}

// lldb-dap reads `env` as `NAME=value` strings in every version.
function environmentList(env) {
  if (Array.isArray(env)) {
    return env.map(String);
  }
  return Object.entries(env || {}).map(([name, value]) => `${name}=${value}`);
}

function escapeLldbPath(value) {
  return String(value).replace(/\\/g, "\\\\").replace(/"/g, '\\"');
}

// The launch request lldb-dap receives for an Aura configuration whose
// program was built at `binary`. Only documented Aura fields are forwarded.
function adapterConfiguration(configuration, binary, formatterPath) {
  const adapter = {
    type: DEBUG_TYPE,
    request: "launch",
    program: binary,
    args: Array.isArray(configuration.args) ? configuration.args.map(String) : [],
    cwd:
      typeof configuration.cwd === "string" && configuration.cwd
        ? configuration.cwd
        : path.dirname(configuration.program),
    env: environmentList(configuration.env),
    initCommands: [`command script import "${escapeLldbPath(formatterPath)}"`],
    auraProgram: configuration.program
  };
  if (typeof configuration.name === "string" && configuration.name) {
    adapter.name = configuration.name;
  }
  if (typeof configuration.stopOnEntry === "boolean") {
    adapter.stopOnEntry = configuration.stopOnEntry;
  }
  return adapter;
}

// The configuration F5 starts with no launch.json: the open Aura file.
function defaultConfiguration(document) {
  if (!document || document.languageId !== "aura") {
    return null;
  }
  return {
    type: DEBUG_TYPE,
    request: "launch",
    name: "Debug Aura file",
    program: "${file}"
  };
}

function isAbsoluteForPlatform(value, platform = process.platform) {
  return (platform === "win32" ? path.win32 : path.posix).isAbsolute(value);
}

function expandHome(value, home = os.homedir(), platform = process.platform) {
  const pathApi = platform === "win32" ? path.win32 : path.posix;
  if (value === "~") {
    return home;
  }
  if (value.startsWith("~/") || value.startsWith("~\\")) {
    return pathApi.join(home, value.slice(2));
  }
  return value;
}

function findDebugCompiler({
  configured = "",
  platform = process.platform,
  pathValue = process.env.PATH || "",
  home = os.homedir(),
  systemDirs = ["/opt/homebrew/bin", "/usr/local/bin"],
  executable = isExecutable
} = {}) {
  const pathApi = platform === "win32" ? path.win32 : path.posix;
  const delimiter = platform === "win32" ? ";" : ":";
  if (configured) {
    const expanded = expandHome(configured, home, platform);
    return isAbsoluteForPlatform(expanded, platform) ? expanded : null;
  }
  const name = platform === "win32" ? "aura.exe" : "aura";
  const pathDirs = pathValue
    .split(delimiter)
    .filter((entry) => entry && isAbsoluteForPlatform(entry, platform));
  const installDirs = [
    pathApi.join(home, ".local", "bin"),
    pathApi.join(home, ".cargo", "bin"),
    ...(platform === "win32" ? [] : systemDirs)
  ];
  for (const directory of [...pathDirs, ...installDirs]) {
    const candidate = pathApi.join(directory, name);
    if (executable(candidate)) {
      return candidate;
    }
  }
  return null;
}

function registerDebugger(
  vscode,
  context,
  {
    runFile = childProcess.execFile,
    platform = process.platform,
    pathValue = process.env.PATH || "",
    home = os.homedir(),
    systemDirs = ["/opt/homebrew/bin", "/usr/local/bin"],
    isExistingFile = isRegularFile
  } = {}
) {
  const formatterPath = context.asAbsolutePath(path.join("debug", "aura_lldb.py"));
  const storage = context.globalStorageUri.fsPath;
  const output = vscode.window.createOutputChannel("Aura Debug");

  function build(program, binary) {
    fs.mkdirSync(path.dirname(binary), { recursive: true });
    const configured = vscode.workspace.getConfiguration("aura").get("compilerPath", "").trim();
    const resolvedCompiler = findDebugCompiler({
      configured,
      platform,
      pathValue,
      home,
      systemDirs
    });
    if (!resolvedCompiler && platform === "win32") {
      return Promise.resolve(
        'Could not find `aura.exe` on PATH. Install Aura or set "aura.compilerPath" to the full path of aura.exe.'
      );
    }
    const command = resolvedCompiler || "aura";
    const options = resolvedCompiler ? { cwd: path.dirname(program) } : {};
    return new Promise((resolve) => {
      runFile(
        command,
        debugBuildArguments(program, binary),
        options,
        (error, _stdout, stderr) => {
          output.append(String(stderr || ""));
          resolve(error ? String(stderr || error.message) : null);
        }
      );
    });
  }

  const provider = {
    resolveDebugConfiguration(_folder, configuration) {
      if (!configuration.type && !configuration.request && !configuration.name) {
        const editor = vscode.window.activeTextEditor;
        const fallback = defaultConfiguration(editor && editor.document);
        if (!fallback) {
          return configuration;
        }
        return fallback;
      }
      return configuration;
    },
    async resolveDebugConfigurationWithSubstitutedVariables(_folder, configuration) {
      if (!configuration.program) {
        vscode.window.showErrorMessage("Aura: the debug configuration needs a `program` .au file.");
        return undefined;
      }
      if (
        typeof configuration.program !== "string" ||
        configuration.program.startsWith("-") ||
        !configuration.program.endsWith(".au") ||
        !isExistingFile(configuration.program)
      ) {
        vscode.window.showErrorMessage("Aura: the debug `program` must be an existing .au file.");
        return undefined;
      }
      const document = vscode.workspace.textDocuments.find(
        (open) => open.uri && open.uri.fsPath === configuration.program
      );
      if (document && document.isDirty) {
        await document.save();
      }
      const binary = debugBinaryPath(configuration.program, storage);
      output.appendLine(`aura build --debug -o ${binary} ${configuration.program}`);
      const failure = await build(configuration.program, binary);
      if (failure) {
        output.show(true);
        vscode.window.showErrorMessage(`Aura: the debug build failed. ${failure.trim()}`);
        return undefined;
      }
      return adapterConfiguration(configuration, binary, formatterPath);
    }
  };

  const factory = {
    createDebugAdapterDescriptor() {
      const configured = vscode.workspace.getConfiguration("aura").get("lldbDapPath", "").trim();
      const adapter = findLldbDap({ configured });
      if (!adapter) {
        vscode.window.showErrorMessage(LLDB_DAP_MISSING);
        return undefined;
      }
      return new vscode.DebugAdapterExecutable(adapter, []);
    }
  };

  async function debugFile(uri) {
    const editor = vscode.window.activeTextEditor;
    const file = uri && uri.fsPath ? uri.fsPath : editor && editor.document.uri.fsPath;
    if (!file) {
      vscode.window.showWarningMessage("Aura: open an .au file first.");
      return false;
    }
    const folder = vscode.workspace.getWorkspaceFolder(vscode.Uri.file(file));
    return vscode.debug.startDebugging(folder, {
      type: DEBUG_TYPE,
      request: "launch",
      name: `Debug ${path.basename(file)}`,
      program: file
    });
  }

  context.subscriptions.push(
    output,
    vscode.debug.registerDebugConfigurationProvider(DEBUG_TYPE, provider),
    vscode.debug.registerDebugAdapterDescriptorFactory(DEBUG_TYPE, factory),
    vscode.commands.registerCommand("aura.debugFile", debugFile)
  );
  return { provider, factory, debugFile };
}

module.exports = {
  ADAPTER_NAMES,
  LLDB_DAP_MISSING,
  adapterConfiguration,
  debugBinaryPath,
  debugBuildArguments,
  defaultConfiguration,
  environmentList,
  expandHome,
  findDebugCompiler,
  findLldbDap,
  registerDebugger
};
