"use strict";
const { pathToFileURL } = require("node:url");

const editorCapabilities = {
  signatureHelpProvider: { triggerCharacters: ["(", ",", "="] },
  referencesProvider: true,
  renameProvider: { prepareProvider: true }
};

function range(item) {
  return {
    start: { line: item.line, character: item.start_character },
    end: { line: item.line, character: item.end_character }
  };
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
    signatures: [{ label: result.label, parameters: result.parameters.map(label => ({ label })) }],
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
}

module.exports = { registerEditorHandlers, editorCapabilities };
