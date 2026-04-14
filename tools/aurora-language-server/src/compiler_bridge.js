"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { pathToFileURL } = require("node:url");
const { spawn } = require("node:child_process");

let workspaceRoots = [];

function setWorkspaceRoots(roots) {
  workspaceRoots = Array.isArray(roots) ? roots.filter(Boolean) : [];
}

async function analyzeWithCompiler(uri, text) {
  const command = resolveCompilerCommand();

  try {
    const stdout = await runCommand(
      command.cmd,
      [...command.args, "analyze", "--stdin", uriToPath(uri) || uri],
      text,
      command.cwd
    );
    return JSON.parse(stdout);
  } catch (_error) {
    return null;
  }
}

async function completeWithCompiler(uri, text, line, character, triggerCharacter) {
  const command = resolveCompilerCommand();

  const args = [...command.args, "complete", "--line", String(line), "--character", String(character)];
  if (triggerCharacter) {
    args.push("--trigger", triggerCharacter);
  }
  args.push("--stdin", uriToPath(uri) || uri);

  try {
    const stdout = await runCommand(command.cmd, args, text, command.cwd);
    return JSON.parse(stdout);
  } catch (_error) {
    return null;
  }
}

function findOccurrence(analysis, line, character) {
  if (!analysis || !Array.isArray(analysis.occurrences)) {
    return null;
  }

  return (
    analysis.occurrences.find(
      (occurrence) =>
        occurrence.line === line &&
        character >= occurrence.start_character &&
        character < occurrence.end_character
    ) || null
  );
}

function compilerDiagnosticsToLsp(analysis) {
  return (analysis.diagnostics || []).map((diagnostic) => ({
    severity: diagnostic.severity,
    range: {
      start: { line: diagnostic.line, character: diagnostic.start_character },
      end: { line: diagnostic.line, character: diagnostic.end_character }
    },
    message: diagnostic.message,
    source: "aurora-compiler"
  }));
}

function compilerSymbolsToLsp(analysis) {
  return (analysis.symbols || []).map(toDocumentSymbol);
}

function compilerDefinitionToLspLocation(documentUri, definition) {
  if (!definition) {
    return null;
  }

  const uri = definition.file_path
    ? pathToFileURL(definition.file_path).toString()
    : documentUri;
  return {
    uri,
    range: {
      start: { line: definition.line, character: definition.start_character },
      end: { line: definition.line, character: definition.end_character }
    }
  };
}

function compilerHoverAtPosition(analysis, line, character) {
  const occurrence = findOccurrence(analysis, line, character);
  if (!occurrence) {
    return null;
  }

  return {
    value: occurrence.hover,
    range: {
      start: {
        line: occurrence.line,
        character: occurrence.start_character
      },
      end: {
        line: occurrence.line,
        character: occurrence.end_character
      }
    }
  };
}

function compilerDefinitionAtPosition(documentUri, analysis, line, character) {
  const occurrence = findOccurrence(analysis, line, character);
  if (!occurrence) {
    return null;
  }
  return compilerDefinitionToLspLocation(documentUri, occurrence.definition);
}

function toDocumentSymbol(symbol) {
  const range = {
    start: { line: symbol.line, character: symbol.start_character || 0 },
    end: { line: symbol.line, character: symbol.end_character || symbol.start_character || 0 }
  };

  return {
    name: symbol.name,
    detail: symbol.detail || "",
    kind: symbolKind(symbol.kind),
    range,
    selectionRange: range,
    children: (symbol.children || []).map(toDocumentSymbol)
  };
}

function symbolKind(kind) {
  switch (kind) {
    case "class":
      return 5;
    case "function":
      return 12;
    case "method":
      return 6;
    case "field":
      return 8;
    case "enum":
      return 10;
    case "variant":
      return 22;
    default:
      return 13;
  }
}

function resolveCompilerCommand() {
  const envPath = process.env.AURORA_LSP_AURA_PATH;
  if (envPath && fs.existsSync(envPath)) {
    return { cmd: envPath, args: [], cwd: undefined };
  }

  for (const root of workspaceRoots) {
    if (
      fs.existsSync(path.join(root, "Cargo.toml")) &&
      fs.existsSync(path.join(root, "crates", "aura", "Cargo.toml"))
    ) {
      return { cmd: "cargo", args: ["run", "-q", "-p", "aura", "--"], cwd: root };
    }
  }

  for (const root of workspaceRoots) {
    const debugBinary = path.join(root, "target", "debug", binaryName());
    if (fs.existsSync(debugBinary)) {
      return { cmd: debugBinary, args: [], cwd: root };
    }

    const releaseBinary = path.join(root, "target", "release", binaryName());
    if (fs.existsSync(releaseBinary)) {
      return { cmd: releaseBinary, args: [], cwd: root };
    }
  }

  return { cmd: "aura", args: [], cwd: workspaceRoots[0] };
}

function binaryName(platform = process.platform) {
  return platform === "win32" ? "aura.exe" : "aura";
}

function uriToPath(uri, platform = process.platform) {
  if (typeof uri !== "string" || !uri.startsWith("file://")) {
    return null;
  }

  let value = decodeURIComponent(uri.replace("file://", ""));
  if (platform === "win32" && value.startsWith("/")) {
    value = value.slice(1);
  }
  return value;
}

function runCommand(cmd, args, input, cwd) {
  return new Promise((resolve, reject) => {
    const child = spawn(cmd, args, {
      cwd,
      stdio: ["pipe", "pipe", "pipe"]
    });

    let stdout = "";
    let stderr = "";

    child.stdout.on("data", (chunk) => {
      stdout += chunk.toString();
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk.toString();
    });
    child.on("error", reject);
    child.on("close", (code) => {
      if (code === 0) {
        resolve(stdout);
      } else {
        reject(new Error(stderr || `command exited with status ${code}`));
      }
    });

    child.stdin.write(input);
    child.stdin.end();
  });
}

module.exports = {
  analyzeWithCompiler,
  binaryName,
  completeWithCompiler,
  compilerDefinitionAtPosition,
  compilerDefinitionToLspLocation,
  compilerDiagnosticsToLsp,
  compilerHoverAtPosition,
  compilerSymbolsToLsp,
  findOccurrence,
  resolveCompilerCommand,
  runCommand,
  setWorkspaceRoots,
  uriToPath
};
