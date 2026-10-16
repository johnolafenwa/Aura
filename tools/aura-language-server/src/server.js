"use strict";

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
  setSourceOverlayProvider,
  setWorkspaceRoots
} = require("./compiler_bridge");
const { createDocumentStateCache } = require("./document_state");
const { canonicalFileUri, uriToPath } = require("./uri");

const { registerEditorHandlers, editorCapabilities } = require("./editor_handlers");

const connection = createConnection(ProposedFeatures.all);
const documents = new TextDocuments(TextDocument);
const documentStateCache = createDocumentStateCache(
  (document) => analyzeWithCompiler(document.uri, document.getText()),
  compilerDependencyUris
);
const validationTimers = new Map();

// The compiler checks each document against the unsaved text of the others.
setSourceOverlayProvider((uri) => openDocumentOverlays(uri, documents.all()));

setCompilerSchemaMismatchHandler(() => {
  documentStateCache.invalidateAll();
});

connection.onInitialize((params) => {
  setWorkspaceRoots(extractWorkspaceRoots(params));
  return {
    serverInfo: {
      name: "aura-language-server",
      version: "0.4.1"
    },
    capabilities: {
      textDocumentSync: TextDocumentSyncKind.Incremental,
      completionProvider: {
        triggerCharacters: ["."]
      },
      documentSymbolProvider: true,
      hoverProvider: true,
      definitionProvider: true,
      ...editorCapabilities
    }
  };
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
