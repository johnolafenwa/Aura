"use strict";

const fs = require("node:fs");
const {
  createConnection,
  DiagnosticSeverity,
  Location,
  MarkupKind,
  ProposedFeatures,
  TextDocuments,
  CompletionItemKind,
  SymbolKind,
  TextDocumentSyncKind
} = require("vscode-languageserver/node");
const { TextDocument } = require("vscode-languageserver-textdocument");
const {
  completionsForDocument,
  definitionForPosition,
  diagnosticsForDocument,
  documentSymbols,
  hoverForPosition
} = require("./recovery");
const {
  analyzeWithCompiler,
  completeWithCompiler,
  editorRequestWithCompiler,
  compilerCompletionToLsp,
  compilerDefinitionAtPosition,
  compilerDependencyUris,
  compilerDiagnosticsToLsp,
  compilerHoverAtPosition,
  compilerSymbolsToLsp,
  openDocumentOverlays,
  setCompilerSchemaMismatchHandler,
  setCompilerUnavailableHandler,
  expandHome,
  setSourceOverlayProvider,
  setWorkspaceRoots
} = require("./compiler_bridge");
const { createDocumentStateCache } = require("./document_state");
const { canonicalFileUri, uriToPath } = require("./uri");

const { registerEditorHandlers, editorCapabilities } = require("./editor_handlers");
const {
  SEMANTIC_TOKEN_MODIFIERS,
  SEMANTIC_TOKEN_TYPES,
  codeActions,
  documentHighlights,
  implementations,
  inlayHints,
  semanticTokens,
  wordAt,
  workspaceSources,
  workspaceSymbols
} = require("./editor_features");

const connection = createConnection(ProposedFeatures.all);
const documents = new TextDocuments(TextDocument);
const documentStateCache = createDocumentStateCache(
  (document) => analyzeWithCompiler(document.uri, document.getText()),
  compilerDependencyUris
);
const validationTimers = new Map();

// The compiler checks each document against the unsaved text of the others.
setSourceOverlayProvider((uri) => openDocumentOverlays(uri, documents.all()));

// Without the compiler the server can only offer keywords and an outline, so
// say why once instead of leaving completion and hover quietly empty.
const shownNotices = new Set();
function showNoticeOnce(key, message) {
  if (shownNotices.has(key)) {
    return;
  }
  shownNotices.add(key);
  void connection.window.showWarningMessage(message);
}

setCompilerSchemaMismatchHandler(({ actual, expected }) => {
  documentStateCache.invalidateAll();
  showNoticeOnce(
    "schema",
    `Aura: the installed compiler speaks editor interface ${actual}, but this extension needs ${expected}. ` +
      "Update both to the same release (rerun the Aura installer and update the extension); until then completion and hover are limited."
  );
});

setCompilerUnavailableHandler(({ command }) => {
  showNoticeOnce(
    "unavailable",
    `Aura: could not start the compiler (\`${command.cmd}\`), so completion, hover, and diagnostics are limited. ` +
      "Install Aura, or set \"aura.compilerPath\" to the full path of the aura executable, then run \"Aura: Restart Language Server\"."
  );
});

let workspaceRoots = [];

connection.onInitialize((params) => {
  workspaceRoots = extractWorkspaceRoots(params);
  setWorkspaceRoots(workspaceRoots);
  return {
    serverInfo: {
      name: "aura-language-server",
      version: "0.6.0"
    },
    capabilities: {
      textDocumentSync: TextDocumentSyncKind.Incremental,
      completionProvider: {
        triggerCharacters: ["."]
      },
      documentSymbolProvider: true,
      documentHighlightProvider: true,
      semanticTokensProvider: {
        legend: { tokenTypes: SEMANTIC_TOKEN_TYPES, tokenModifiers: SEMANTIC_TOKEN_MODIFIERS },
        full: true
      },
      inlayHintProvider: true,
      codeActionProvider: { codeActionKinds: ["quickfix"] },
      workspaceSymbolProvider: true,
      implementationProvider: true,
      hoverProvider: true,
      definitionProvider: true,
      ...editorCapabilities
    }
  };
});

connection.onInitialized(() => {
  const configuredPath = process.env.AURA_LSP_AURA_PATH;
  if (configuredPath && !fs.existsSync(expandHome(configuredPath))) {
    showNoticeOnce(
      "configured-path",
      `Aura: the configured compiler path \`${configuredPath}\` does not exist; looking for \`aura\` on PATH instead.`
    );
  }
});

registerEditorHandlers(connection, documents, editorRequestWithCompiler);

connection.onCompletion(async (params, cancellationToken) => {
  const document = documents.get(params.textDocument.uri);
  if (!document) {
    return [];
  }

  const compilerItems = await completeWithCompiler(
    params.textDocument.uri,
    document.getText(),
    params.position.line,
    params.position.character,
    params.context ? params.context.triggerCharacter || null : null,
    cancellationToken
  );
  if (compilerItems) {
    return compilerItems.map((item) => compilerCompletionToLsp(item, completionKind(item.kind)));
  }

  const items = completionsForDocument(
    document.getText(),
    params.position.line,
    params.position.character,
    params.context ? params.context.triggerCharacter || null : null
  );

  return items.map((item) => ({
    label: item.name,
    kind: completionKind(item.kind),
    detail: item.detail || ""
  }));
});

async function analysisFor(uri) {
  const document = documents.get(uri);
  if (!document) {
    return null;
  }
  const state = await getDocumentState(document);
  return { document, analysis: state.compilerAnalysis };
}

function openSources() {
  return documents.all().map((document) => ({ uri: document.uri, text: document.getText() }));
}

connection.onDocumentHighlight(async (params) => {
  const found = await analysisFor(params.textDocument.uri);
  return found && found.analysis
    ? documentHighlights(
        found.analysis,
        found.document.getText(),
        params.position.line,
        params.position.character
      )
    : [];
});

connection.languages.semanticTokens.on(async (params) => {
  const found = await analysisFor(params.textDocument.uri);
  return found && found.analysis
    ? semanticTokens(found.analysis, found.document.getText())
    : { data: [] };
});

connection.languages.inlayHint.on(async (params) => {
  const found = await analysisFor(params.textDocument.uri);
  return found && found.analysis
    ? inlayHints(found.analysis, found.document.getText(), params.range)
    : [];
});

connection.onCodeAction((params) => {
  const document = documents.get(params.textDocument.uri);
  return document
    ? codeActions(params.textDocument.uri, document.getText(), params.context.diagnostics)
    : [];
});

connection.onWorkspaceSymbol((params) =>
  workspaceSymbols(params.query, workspaceSources(workspaceRoots, openSources()))
);

connection.onImplementation((params) => {
  const document = documents.get(params.textDocument.uri);
  if (!document) {
    return [];
  }
  const name = wordAt(document.getText(), params.position.line, params.position.character);
  return implementations(name, workspaceSources(workspaceRoots, openSources()));
});

connection.onDocumentSymbol(async (params) => {
  const document = documents.get(params.textDocument.uri);
  if (!document) {
    return [];
  }

  const state = await getDocumentState(document);
  if (state.compilerAnalysis) {
    return compilerSymbolsToLsp(state.compilerAnalysis);
  }

  return documentSymbols(document.getText()).map((symbol) =>
    toDocumentSymbol(document, symbol)
  );
});

connection.onHover(async (params) => {
  const document = documents.get(params.textDocument.uri);
  if (!document) {
    return null;
  }

  const state = await getDocumentState(document);
  if (state.compilerAnalysis) {
    const compilerHover = compilerHoverAtPosition(
      state.compilerAnalysis,
      params.position.line,
      params.position.character
    );
    if (!compilerHover) {
      return null;
    }
    return {
      contents: {
        kind: MarkupKind.Markdown,
        value: compilerHover.value
      },
      range: compilerHover.range
    };
  }

  const hover = hoverForPosition(
    document.getText(),
    params.position.line,
    params.position.character
  );
  if (!hover) {
    return null;
  }

  return {
    contents: {
      kind: MarkupKind.Markdown,
      value: hover.value
    },
    range: hover.range
  };
});

connection.onDefinition(async (params) => {
  const document = documents.get(params.textDocument.uri);
  if (!document) {
    return null;
  }

  const state = await getDocumentState(document);
  if (state.compilerAnalysis) {
    const location = compilerDefinitionAtPosition(
      params.textDocument.uri,
      state.compilerAnalysis,
      params.position.line,
      params.position.character
    );
    if (!location) {
      return null;
    }
    return Location.create(location.uri, location.range);
  }

  const definition = definitionForPosition(
    document.getText(),
    params.position.line,
    params.position.character
  );
  if (!definition) {
    return null;
  }

  return Location.create(params.textDocument.uri, {
    start: {
      line: definition.line,
      character: definition.startCharacter
    },
    end: {
      line: definition.line,
      character: definition.endCharacter
    }
  });
});

documents.onDidOpen((event) => {
  scheduleValidation(invalidateDocument(event.document.uri));
});

documents.onDidChangeContent((event) => {
  scheduleValidation(invalidateDocument(event.document.uri));
});

documents.onDidClose((event) => {
  const affected = invalidateDocument(event.document.uri);
  affected.delete(event.document.uri);
  documentStateCache.deleteDocument(event.document.uri);
  clearValidationTimer(event.document.uri);
  connection.sendDiagnostics({ uri: event.document.uri, diagnostics: [] });
  scheduleValidation(affected);
});

// Saved, created, and deleted files reach the server as watched-file events.
// A document whose analysis failed or is in flight may not know every file it
// imports yet, so it is checked again too.
connection.onDidChangeWatchedFiles((params) => {
  const affected = documentStateCache.invalidateWhere(
    (state) =>
      !state || !state.compilerAnalysis || (state.compilerAnalysis.diagnostics || []).length > 0
  );
  for (const change of params.changes || []) {
    for (const uri of invalidateDocument(change.uri)) {
      affected.add(uri);
    }
  }
  scheduleValidation(affected);
});

documents.listen(connection);
connection.listen();

// Dependencies are recorded by canonical file URI, which can differ from the
// editor's URI for the same file.
function invalidateDocument(uri) {
  const affected = documentStateCache.invalidate(uri);
  const canonical = canonicalFileUri(uri);
  if (canonical !== uri) {
    for (const affectedUri of documentStateCache.invalidate(canonical)) {
      affected.add(affectedUri);
    }
  }
  return affected;
}

function completionKind(kind) {
  switch (kind) {
    case "class":
      return CompletionItemKind.Class;
    case "module":
      return CompletionItemKind.Module;
    case "function":
      return CompletionItemKind.Function;
    case "method":
      return CompletionItemKind.Method;
    case "property":
      return CompletionItemKind.Property;
    case "field":
      return CompletionItemKind.Field;
    case "enum":
      return CompletionItemKind.Enum;
    case "variant":
      return CompletionItemKind.EnumMember;
    case "constant":
      return CompletionItemKind.Constant;
    case "keyword":
      return CompletionItemKind.Keyword;
    case "variable":
    case "parameter":
      return CompletionItemKind.Variable;
    default:
      return CompletionItemKind.Text;
  }
}

function toDocumentSymbol(document, symbol) {
  const line = Math.max(0, Math.min(symbol.line, document.lineCount - 1));
  const range = {
    start: { line, character: symbol.startCharacter || 0 },
    end: { line, character: symbol.endCharacter || symbol.startCharacter || 0 }
  };

  return {
    name: symbol.name,
    detail: symbol.detail || "",
    kind: symbolKind(symbol.kind),
    range,
    selectionRange: range,
    children: (symbol.children || []).map((child) => toDocumentSymbol(document, child))
  };
}

function validateDocument(document) {
  const requestedVersion = document.version;
  return getDocumentState(document).then((state) => {
    const current = documents.get(document.uri);
    if (!current || current.version !== requestedVersion || state.version !== requestedVersion) {
      return;
    }
    const diagnostics = state.compilerAnalysis
      ? compilerDiagnosticsToLsp(state.compilerAnalysis, document.uri)
      : diagnosticsForDocument(document.getText()).map((diagnostic) => ({
          severity: mapSeverity(diagnostic.severity),
          range: {
            start: { line: diagnostic.line, character: diagnostic.startCharacter },
            end: { line: diagnostic.line, character: diagnostic.endCharacter }
          },
          message: diagnostic.message,
          source: "aura-lsp"
        }));

    connection.sendDiagnostics({
      uri: document.uri,
      diagnostics
    });
  });
}

async function getDocumentState(document) {
  return documentStateCache.get(document);
}

function clearValidationTimer(uri) {
  const timer = validationTimers.get(uri);
  if (timer) {
    clearTimeout(timer);
    validationTimers.delete(uri);
  }
}

function scheduleValidation(uris) {
  for (const uri of uris) {
    clearValidationTimer(uri);
    const timer = setTimeout(() => {
      validationTimers.delete(uri);
      const document = documents.get(uri);
      if (document) {
        void validateDocument(document);
      }
    }, 150);
    validationTimers.set(uri, timer);
  }
}

function extractWorkspaceRoots(params) {
  if (Array.isArray(params.workspaceFolders) && params.workspaceFolders.length > 0) {
    return params.workspaceFolders
      .map((folder) => uriToPath(folder.uri))
      .filter(Boolean);
  }
  if (params.rootUri) {
    const root = uriToPath(params.rootUri);
    return root ? [root] : [];
  }
  return [];
}

function mapSeverity(severity) {
  switch (severity) {
    case 1:
      return DiagnosticSeverity.Error;
    case 2:
      return DiagnosticSeverity.Warning;
    case 3:
      return DiagnosticSeverity.Information;
    default:
      return DiagnosticSeverity.Hint;
  }
}

function symbolKind(kind) {
  switch (kind) {
    case "class":
      return SymbolKind.Class;
    case "function":
      return SymbolKind.Function;
    case "method":
      return SymbolKind.Method;
    case "property":
      return SymbolKind.Property;
    case "field":
      return SymbolKind.Field;
    case "trait":
      return SymbolKind.Interface;
    default:
      return SymbolKind.Variable;
  }
}

