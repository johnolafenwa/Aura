"use strict";
const { pathToFileURL } = require("node:url");

const editorCapabilities = {
  signatureHelpProvider: { triggerCharacters: ["(", ",", "="] },
  referencesProvider: true,
  renameProvider: { prepareProvider: true },
  documentFormattingProvider: true
};

function wholeDocumentRange(source) {
  const lines = source.split("\n");
  return {
    start: { line: 0, character: 0 },
    end: { line: lines.length - 1, character: lines[lines.length - 1].length }
  };
}

function range(item) {
  return {
    start: { line: item.line, character: item.start_character },
    end: { line: item.line, character: item.end_character }
  };
}

// Docstring text from the compiler is normalized and sanitized Markdown.
function markdownDocumentation(value) {
  return value ? { documentation: { kind: "markdown", value } } : {};
}

function registerEditorHandlers(connection, documents, request) {
  function register(handler, method, empty, convert) {
    connection[handler](async (params, cancellationToken) => {
      const uri = params.textDocument.uri;
      const document = documents.get(uri);
      if (!document || cancellationToken?.isCancellationRequested) return empty;
      const version = document.version;
      const result = await request(method, uri, document.getText(), {
        line: params.position.line,
        character: params.position.character,
        include_declaration: params.context?.includeDeclaration || false,
        new_name: params.newName
      }, cancellationToken);
      if (!result || cancellationToken?.isCancellationRequested || documents.get(uri)?.version !== version) return empty;
      return convert(result, uri, version);
    });
  }
  register("onSignatureHelp", "signature-help", null, result => ({
    signatures: [{
      label: result.label,
      ...markdownDocumentation(result.documentation),
      parameters: result.parameters.map((label, index) => ({
        label,
        ...markdownDocumentation((result.parameter_documentation || [])[index])
      }))
    }],
    activeSignature: 0,
    activeParameter: result.active_parameter
  }));
  register("onReferences", "references", [], (result, uri) => result.map(item => ({
    uri: item.file_path ? pathToFileURL(item.file_path).toString() : uri,
    range: range(item)
  })));
  register("onPrepareRename", "prepare-rename", null, result => range(result));
  register("onRenameRequest", "rename", null, (result, uri, version) => ({
    documentChanges: [{
      textDocument: { uri, version },
      edits: result.edits.map(item => ({ range: range(item), newText: item.replacement }))
    }]
  }));
  connection.onDocumentFormatting(async (params, cancellationToken) => {
    const uri = params.textDocument.uri;
    const document = documents.get(uri);
    if (!document || cancellationToken?.isCancellationRequested) return [];
    const version = document.version;
    const source = document.getText();
    const result = await request("format", uri, source, {}, cancellationToken);
    if (
      typeof result?.formatted !== "string" ||
      result.formatted === source ||
      cancellationToken?.isCancellationRequested ||
      documents.get(uri)?.version !== version
    ) {
      return [];
    }
    return [{ range: wholeDocumentRange(source), newText: result.formatted }];
  });
}

module.exports = { registerEditorHandlers, editorCapabilities, markdownDocumentation };
