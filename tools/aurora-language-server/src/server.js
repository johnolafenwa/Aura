"use strict";

const {
  createConnection,
  ProposedFeatures,
  TextDocuments,
  CompletionItemKind,
  SymbolKind,
  TextDocumentSyncKind
} = require("vscode-languageserver/node");
const { TextDocument } = require("vscode-languageserver-textdocument");
const { completionsForDocument, documentSymbols } = require("./analysis");

const connection = createConnection(ProposedFeatures.all);
const documents = new TextDocuments(TextDocument);

connection.onInitialize(() => {
  return {
    capabilities: {
      textDocumentSync: TextDocumentSyncKind.Incremental,
      completionProvider: {
        triggerCharacters: ["."]
      },
      documentSymbolProvider: true
    }
  };
});

connection.onCompletion((params) => {
  const document = documents.get(params.textDocument.uri);
  if (!document) {
    return [];
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

connection.onDocumentSymbol((params) => {
  const document = documents.get(params.textDocument.uri);
  if (!document) {
    return [];
  }

  return documentSymbols(document.getText()).map((symbol) =>
    toDocumentSymbol(document, symbol)
  );
});

documents.listen(connection);
connection.listen();

function completionKind(kind) {
  switch (kind) {
    case "class":
      return CompletionItemKind.Class;
    case "function":
      return CompletionItemKind.Function;
    case "method":
      return CompletionItemKind.Method;
    case "field":
      return CompletionItemKind.Field;
    case "keyword":
      return CompletionItemKind.Keyword;
    default:
      return CompletionItemKind.Text;
  }
}

function toDocumentSymbol(document, symbol) {
  const line = Math.max(0, Math.min(symbol.line, document.lineCount - 1));
  const lineText = document.getText({
    start: { line, character: 0 },
    end: { line, character: Number.MAX_SAFE_INTEGER }
  });
  const range = {
    start: { line, character: 0 },
    end: { line, character: lineText.length }
  };

  return {
    name: symbol.name,
    detail: "",
    kind: symbolKind(symbol.kind),
    range,
    selectionRange: range,
    children: (symbol.children || []).map((child) => toDocumentSymbol(document, child))
  };
}

function symbolKind(kind) {
  switch (kind) {
    case "class":
      return SymbolKind.Class;
    case "function":
      return SymbolKind.Function;
    case "method":
      return SymbolKind.Method;
    case "field":
      return SymbolKind.Field;
    default:
      return SymbolKind.Variable;
  }
}
