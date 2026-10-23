"use strict";

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { pathToFileURL } = require("node:url");
const { spawn } = require("node:child_process");
const { uriToPath } = require("./uri");
const { createPositionMapper } = require("./positions");

let workspaceRoots = [];
let compilerService = null;
let compilerServiceKey = null;
let compilerSchemaMismatchHandler = null;
let compilerUnavailableHandler = null;
let sourceOverlayProvider = null;
const COMPILER_REQUEST_TIMEOUT_MS = 15_000;
const COMPILER_RESPONSE_LIMIT_BYTES = 16 * 1024 * 1024;
// The compiler owns the canonical identity; this transport declares the one
// compiler interface it can safely decode.
const SUPPORTED_SEMANTIC_INTERFACE_SCHEMA_VERSION = 20;
// The source text each compiler analysis describes, so that its UTF-8 byte
// columns convert to the UTF-16 positions of that same text.
const analyzedSources = new WeakMap();

function setCompilerSchemaMismatchHandler(handler) {
  compilerSchemaMismatchHandler = typeof handler === "function" ? handler : null;
}

// The provider returns `[{ path, source }]` for the open documents other than
// the requested one. The compiler reads those buffers instead of the files.
function setSourceOverlayProvider(provider) {
  sourceOverlayProvider = typeof provider === "function" ? provider : null;
}

function withSourceOverlays(uri, params) {
  const overlays = sourceOverlayProvider ? sourceOverlayProvider(uri) : [];
  return overlays.length > 0 ? { ...params, overlays } : params;
}

function openDocumentOverlays(documentUri, documents) {
  const overlays = [];
  for (const document of documents) {
    const overlayPath = document.uri !== documentUri && uriToPath(document.uri);
    if (overlayPath) {
      overlays.push({ path: overlayPath, source: document.getText() });
    }
  }
  return overlays;
}

// The handler hears `{ command, error }` when the compiler process cannot be
// started at all, such as when `aura` is not installed or not on `PATH`.
function setCompilerUnavailableHandler(handler) {
  compilerUnavailableHandler = typeof handler === "function" ? handler : null;
}

function notifyCompilerUnavailable(details) {
  if (compilerUnavailableHandler) {
    compilerUnavailableHandler(details);
  }
}

function notifyCompilerSchemaMismatch(details) {
  if (compilerSchemaMismatchHandler) {
    compilerSchemaMismatchHandler(details);
  }
}

function setWorkspaceRoots(roots) {
  const nextRoots = Array.isArray(roots) ? roots.filter(Boolean) : [];
  if (JSON.stringify(nextRoots) !== JSON.stringify(workspaceRoots)) {
    disposeCompilerService();
  }
  workspaceRoots = nextRoots;
}

async function analyzeWithCompiler(uri, source) {
  try {
    const params = withSourceOverlays(uri, { path: uriToPath(uri) || uri, source });
    const analysis = await requestCompiler("analyze", params);
    if (analysis && typeof analysis === "object") {
      analyzedSources.set(analysis, { source, overlays: params.overlays || [] });
    }
    return analysis;
  } catch (error) {
    reportCompilerFailure("analysis", error);
    return null;
  }
}

async function completeWithCompiler(
  uri,
  source,
  line,
  character,
  triggerCharacter,
  cancellationToken
) {
  try {
    return await requestCompiler(
      "complete",
      withSourceOverlays(uri, {
        path: uriToPath(uri) || uri,
        source,
        line,
        character: createPositionMapper(source).toCompilerColumn(line, character),
        trigger: triggerCharacter || null
      }),
      cancellationToken
    );
  } catch (error) {
    reportCompilerFailure("completion", error);
    return null;
  }
}

async function editorRequestWithCompiler(method, uri, source, position, cancellationToken) {
  try {
    const params = { path: uriToPath(uri) || uri, source, ...position };
    // Formatting reads only the buffer, so it needs no other files.
    return await requestCompiler(
      method,
      method === "format" ? params : withSourceOverlays(uri, params),
      cancellationToken
    );
  } catch (error) {
    reportCompilerFailure(method, error);
    return null;
  }
}

function requestCompiler(method, params, cancellationToken) {
  const command = resolveCompilerCommand();
  const key = JSON.stringify(command);
  if (!compilerService || compilerService.closed || compilerServiceKey !== key) {
    disposeCompilerService();
    compilerService = new CompilerService(command, {
      onSemanticSchemaMismatch: notifyCompilerSchemaMismatch,
      onUnavailable: notifyCompilerUnavailable
    });
    compilerServiceKey = key;
  }
  return compilerService.request(method, params, cancellationToken);
}

class CompilerService {
  constructor(
    command,
    {
      requestTimeoutMs = COMPILER_REQUEST_TIMEOUT_MS,
      responseLimitBytes = COMPILER_RESPONSE_LIMIT_BYTES,
      onSemanticSchemaMismatch = () => {},
      onUnavailable = () => {}
    } = {}
  ) {
    this.command = command;
    this.nextId = 1;
    this.pending = new Map();
    this.stdoutBuffer = "";
    this.stderrBuffer = "";
    this.closed = false;
    this.requestTimeoutMs = requestTimeoutMs;
    this.responseLimitBytes = responseLimitBytes;
    this.onSemanticSchemaMismatch = onSemanticSchemaMismatch;
    this.child = spawn(command.cmd, [...command.args, "lsp"], {
      cwd: command.cwd,
      stdio: ["pipe", "pipe", "pipe"]
    });
    this.child.stdout.setEncoding("utf8");
    this.child.stderr.setEncoding("utf8");
    this.child.stdout.on("data", (chunk) => this.handleStdout(chunk));
    this.child.stderr.on("data", (chunk) => {
      this.stderrBuffer = (this.stderrBuffer + chunk).slice(-65_536);
    });
    // An `error` event means the process could not be started or signalled.
    this.child.on("error", (error) => {
      onUnavailable({ command, error });
      this.fail(error);
    });
    this.child.on("close", (code) => {
      this.fail(
        new Error(
          this.stderrBuffer || `Aura compiler service exited with status ${code}`
        )
      );
    });
  }

  request(method, params, cancellationToken) {
    if (this.closed) {
      return Promise.reject(new Error("Aura compiler service is closed"));
    }
    if (cancellationToken && cancellationToken.isCancellationRequested) {
      return Promise.reject(new Error("Aura compiler request was cancelled"));
    }

    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.fail(
          new Error(
            `Aura compiler ${method} request timed out after ${this.requestTimeoutMs}ms`
          ),
          true
        );
      }, this.requestTimeoutMs);
      const cancellation = cancellationToken
        ? cancellationToken.onCancellationRequested(() => {
            this.fail(new Error(`Aura compiler ${method} request was cancelled`), true);
          })
        : null;
      this.pending.set(id, { resolve, reject, timer, cancellation });
      this.child.stdin.write(
        `${JSON.stringify({
          id,
          method,
          ...params,
          semantic_interface_version: SUPPORTED_SEMANTIC_INTERFACE_SCHEMA_VERSION
        })}\n`
      );
    });
  }

  handleStdout(chunk) {
    if (this.closed) {
      return;
    }
    this.stdoutBuffer += chunk;
    if (Buffer.byteLength(this.stdoutBuffer, "utf8") > this.responseLimitBytes) {
      this.fail(new Error("Aura compiler response exceeded 16 MiB"), true);
      return;
    }

    let newline = this.stdoutBuffer.indexOf("\n");
    while (newline >= 0) {
      const line = this.stdoutBuffer.slice(0, newline);
      this.stdoutBuffer = this.stdoutBuffer.slice(newline + 1);
      if (line) {
        let response;
        try {
          response = JSON.parse(line);
        } catch (error) {
          this.fail(new Error(`invalid Aura compiler response: ${error.message}`), true);
          return;
        }
        if (
          response.semantic_interface_version !==
          SUPPORTED_SEMANTIC_INTERFACE_SCHEMA_VERSION
        ) {
          const actualSchema = Object.prototype.hasOwnProperty.call(
            response,
            "semantic_interface_version"
          )
            ? response.semantic_interface_version
            : "<missing>";
          const error = new Error(
            `Aura compiler semantic schema mismatch: received \`${actualSchema}\`; expected \`${SUPPORTED_SEMANTIC_INTERFACE_SCHEMA_VERSION}\``
          );
          this.fail(error, true);
          this.onSemanticSchemaMismatch({
            actual: actualSchema,
            expected: SUPPORTED_SEMANTIC_INTERFACE_SCHEMA_VERSION
          });
          return;
        }
        const pending = this.pending.get(response.id);
        if (pending) {
          this.pending.delete(response.id);
          clearTimeout(pending.timer);
          if (pending.cancellation) {
            pending.cancellation.dispose();
          }
          if (Object.prototype.hasOwnProperty.call(response, "error")) {
            pending.reject(new Error(String(response.error)));
          } else {
            pending.resolve(response.result);
          }
        }
      }
      newline = this.stdoutBuffer.indexOf("\n");
    }
  }

  fail(error, kill = false) {
    if (this.closed) {
      return;
    }
    this.closed = true;
    if (kill) {
      this.child.kill();
    }
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timer);
      if (pending.cancellation) {
        pending.cancellation.dispose();
      }
      pending.reject(error);
    }
    this.pending.clear();
  }

  dispose() {
    if (!this.closed) {
      this.fail(new Error("Aura compiler service disposed"), true);
    }
  }
}

function disposeCompilerService() {
  if (compilerService) {
    compilerService.dispose();
  }
  compilerService = null;
  compilerServiceKey = null;
}

function reportCompilerFailure(operation, error) {
  process.stderr.write(
    `[aura-lsp] compiler ${operation} unavailable; using recovery analysis: ${error.message}\n`
  );
}

function analysisPositions(analysis) {
  const analyzed = analyzedSources.get(analysis);
  return createPositionMapper(analyzed && analyzed.source);
}

// The text the compiler read for `filePath`: an overlay it was sent, or the
// file on disk. Compiler paths are canonical, so compare resolved paths too.
function analyzedFileSource(analysis, filePath) {
  const analyzed = analyzedSources.get(analysis);
  const overlays = analyzed ? analyzed.overlays : [];
  const overlay =
    overlays.find((candidate) => candidate.path === filePath) ||
    overlays.find((candidate) => realPath(candidate.path) === filePath);
  return overlay ? overlay.source : readSourceFile(filePath);
}

function realPath(filePath) {
  try {
    return fs.realpathSync.native(filePath);
  } catch (_error) {
    return null;
  }
}

// The file URIs an analysis read, as dependency keys. A failed analysis may
// stop before it reads every import, so it keeps the previous dependencies.
function compilerDependencyUris(analysis, previous = new Set()) {
  const dependencies = new Set();
  const paths = Array.isArray(analysis && analysis.dependencies)
    ? analysis.dependencies
    : ((analysis && analysis.occurrences) || [])
        .map((occurrence) => occurrence.definition && occurrence.definition.file_path)
        .filter(Boolean);
  for (const filePath of paths) {
    dependencies.add(pathToFileURL(filePath).toString());
  }
  if (!analysis || (analysis.diagnostics || []).length > 0) {
    for (const dependency of previous) {
      dependencies.add(dependency);
    }
  }
  return dependencies;
}

function findOccurrence(analysis, line, character) {
  if (!analysis || !Array.isArray(analysis.occurrences)) {
    return null;
  }

  const column = analysisPositions(analysis).toCompilerColumn(line, character);
  return (
    analysis.occurrences.find(
      (occurrence) =>
        occurrence.line === line &&
        column >= occurrence.start_character &&
        column < occurrence.end_character
    ) || null
  );
}

function compilerCompletionToLsp(item, kind) {
  const result = {
    label: item.name,
    kind,
    detail: item.detail || ""
  };
  if (item.documentation) {
    result.documentation = { kind: "markdown", value: item.documentation };
  }
  if (item.kind === "parameter") {
    // Keyword arguments of the call being written rank above other names.
    result.sortText = `0${item.name}`;
  }
  return result;
}

function compilerDiagnosticsToLsp(analysis, documentUri) {
  const positions = analysisPositions(analysis);
  return (analysis.diagnostics || []).map((diagnostic) => {
    const result = {
      severity: diagnostic.severity,
      range: positions.range(
        diagnostic.line,
        diagnostic.start_character,
        diagnostic.end_character
      ),
      message: diagnostic.message,
      source: "aura-compiler"
    };

    if (diagnostic.code) {
      result.code = diagnostic.code;
    }
    if (documentUri && diagnostic.secondary_spans?.length) {
      result.relatedInformation = diagnostic.secondary_spans.map((secondary) => ({
        location: {
          uri: documentUri,
          range: positions.range(
            secondary.line,
            secondary.start_character,
            secondary.end_character
          )
        },
        message: secondary.label
      }));
    }
    result.data = {
      notes: diagnostic.notes || [],
      help: diagnostic.help || [],
      edits: diagnostic.edits || [],
      call_frames: diagnostic.call_frames || [],
      task_ancestry: diagnostic.task_ancestry || []
    };
    if (diagnostic.assertion_operands?.length) {
      result.data.assertion_operands = diagnostic.assertion_operands;
    }
    return result;
  });
}

function compilerSymbolsToLsp(analysis) {
  const positions = analysisPositions(analysis);
  return (analysis.symbols || []).map((symbol) => toDocumentSymbol(symbol, positions));
}

function readSourceFile(filePath) {
  try {
    return fs.readFileSync(filePath, "utf8");
  } catch (_error) {
    return null;
  }
}

// A definition without a file path is in the analyzed document. Another
// file's columns convert with that file's text.
function compilerDefinitionToLspLocation(
  documentUri,
  definition,
  documentPositions = createPositionMapper(null),
  fileSource = readSourceFile
) {
  if (!definition) {
    return null;
  }

  const uri = definition.file_path
    ? pathToFileURL(definition.file_path).toString()
    : documentUri;
  const positions = definition.file_path
    ? createPositionMapper(fileSource(definition.file_path))
    : documentPositions;
  return {
    uri,
    range: positions.range(definition.line, definition.start_character, definition.end_character)
  };
}

function compilerHoverAtPosition(analysis, line, character) {
  const occurrence = findOccurrence(analysis, line, character);
  if (!occurrence) {
    return null;
  }

  return {
    value: occurrence.hover,
    range: analysisPositions(analysis).range(
      occurrence.line,
      occurrence.start_character,
      occurrence.end_character
    )
  };
}

function compilerDefinitionAtPosition(documentUri, analysis, line, character) {
  const occurrence = findOccurrence(analysis, line, character);
  if (!occurrence) {
    return null;
  }
  return compilerDefinitionToLspLocation(
    documentUri,
    occurrence.definition,
    analysisPositions(analysis),
    (filePath) => analyzedFileSource(analysis, filePath)
  );
}

function toDocumentSymbol(symbol, positions) {
  const start = symbol.start_character || 0;
  const range = positions.range(symbol.line, start, symbol.end_character || start);

  return {
    name: symbol.name,
    detail: symbol.detail || "",
    kind: symbolKind(symbol.kind),
    range,
    selectionRange: range,
    children: (symbol.children || []).map((child) => toDocumentSymbol(child, positions))
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
    case "property":
      return 7;
    case "field":
      return 8;
    case "enum":
      return 10;
    case "trait":
      return 11;
    case "variant":
      return 22;
    case "constant":
      return 14;
    default:
      return 13;
  }
}

function expandHome(value) {
  if (value === "~") {
    return os.homedir();
  }
  if (value.startsWith("~/") || value.startsWith("~\\")) {
    return path.join(os.homedir(), value.slice(2));
  }
  return value;
}

function isFile(candidate) {
  try {
    return fs.statSync(candidate).isFile();
  } catch {
    return false;
  }
}

function findOnPath(name, searchPath = process.env.PATH || "") {
  for (const directory of searchPath.split(path.delimiter)) {
    if (directory && isFile(path.join(directory, name))) {
      return path.join(directory, name);
    }
  }
  return null;
}

// Editors started from a desktop launcher often do not see the shell's
// `PATH`, so look where the Aura installer, cargo, and Homebrew put `aura`.
function installedCompiler(home = os.homedir(), platform = process.platform) {
  const name = binaryName(platform);
  const directories = [
    path.join(home, ".local", "bin"),
    path.join(home, ".cargo", "bin"),
    "/opt/homebrew/bin",
    "/usr/local/bin"
  ];
  return directories.map((directory) => path.join(directory, name)).find(isFile) || null;
}

function resolveCompilerCommand() {
  const envPath = process.env.AURA_LSP_AURA_PATH && expandHome(process.env.AURA_LSP_AURA_PATH);
  if (envPath && fs.existsSync(envPath)) {
    return { cmd: envPath, args: [], cwd: undefined };
  }

  // A workspace is untrusted input: running its `target/` binaries or
  // `cargo run` would execute code from any folder the editor opens. Only
  // contributors who ask for it get the checkout's own compiler.
  if (process.env.AURA_LSP_USE_WORKSPACE_COMPILER !== "1") {
    return defaultCompilerCommand();
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

  for (const root of workspaceRoots) {
    if (
      fs.existsSync(path.join(root, "Cargo.toml")) &&
      fs.existsSync(path.join(root, "crates", "aura", "Cargo.toml"))
    ) {
      return { cmd: "cargo", args: ["run", "-q", "-p", "aura", "--"], cwd: root };
    }
  }

  return defaultCompilerCommand();
}

function defaultCompilerCommand() {
  if (!findOnPath(binaryName())) {
    const installed = installedCompiler();
    if (installed) {
      return { cmd: installed, args: [], cwd: workspaceRoots[0] };
    }
  }
  return { cmd: "aura", args: [], cwd: workspaceRoots[0] };
}

function binaryName(platform = process.platform) {
  return platform === "win32" ? "aura.exe" : "aura";
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
  editorRequestWithCompiler,
  analyzeWithCompiler,
  binaryName,
  CompilerService,
  completeWithCompiler,
  compilerCompletionToLsp,
  compilerDefinitionAtPosition,
  compilerDefinitionToLspLocation,
  compilerDependencyUris,
  compilerDiagnosticsToLsp,
  compilerHoverAtPosition,
  compilerSymbolsToLsp,
  findOccurrence,
  disposeCompilerService,
  openDocumentOverlays,
  expandHome,
  findOnPath,
  installedCompiler,
  resolveCompilerCommand,
  runCommand,
  SUPPORTED_SEMANTIC_INTERFACE_SCHEMA_VERSION,
  setCompilerSchemaMismatchHandler,
  setCompilerUnavailableHandler,
  setSourceOverlayProvider,
  setWorkspaceRoots,
  uriToPath
};
