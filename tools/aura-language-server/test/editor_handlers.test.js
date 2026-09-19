"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const { registerEditorHandlers, editorCapabilities } = require("../src/editor_handlers");

function harness(response) {
  const handlers = {};
  const calls = [];
  const document = { version: 1, getText: () => "source" };
  const documents = new Map([["file:///tmp/main.au", document]]);
  const connection = Object.fromEntries(["onSignatureHelp", "onReferences", "onPrepareRename", "onRenameRequest"].map(name => [name, handler => { handlers[name] = handler; }]));
  registerEditorHandlers(connection, documents, async (...args) => { calls.push(args); return typeof response === "function" ? response() : response; });
  return { handlers, calls, document, documents };
}
const params = { textDocument: { uri: "file:///tmp/main.au" }, position: { line: 2, character: 4 } };
const range = { line: 2, start_character: 3, end_character: 8 };
const lspRange = { start: { line: 2, character: 3 }, end: { line: 2, character: 8 } };

test("compiler-owned editor capabilities and signature conversion", async () => {
  assert.deepEqual(editorCapabilities, { signatureHelpProvider: { triggerCharacters: ["(", ",", "="] }, referencesProvider: true, renameProvider: { prepareProvider: true } });
  const h = harness({ label: "def(value: int64) -> int64", parameters: ["value: int64"], active_parameter: 0 });
  assert.deepEqual(await h.handlers.onSignatureHelp(params), { signatures: [{ label: "def(value: int64) -> int64", parameters: [{ label: "value: int64" }] }], activeSignature: 0, activeParameter: 0 });
  assert.equal(h.calls[0][0], "signature-help");
});

test("reference locations and versioned rename edits preserve compiler ranges", async () => {
  let h = harness([range, { ...range, file_path: "/tmp/library.au" }]);
  assert.deepEqual(await h.handlers.onReferences({ ...params, context: { includeDeclaration: true } }), [{ uri: params.textDocument.uri, range: lspRange }, { uri: "file:///tmp/library.au", range: lspRange }]);
  assert.equal(h.calls[0][3].include_declaration, true);
  h = harness(range);
  assert.deepEqual(await h.handlers.onPrepareRename(params), lspRange);
  h = harness({ range, edits: [{ ...range, replacement: "answer" }] });
  assert.deepEqual(await h.handlers.onRenameRequest({ ...params, newName: "answer" }), { documentChanges: [{ textDocument: { uri: params.textDocument.uri, version: 1 }, edits: [{ range: lspRange, newText: "answer" }] }] });
  assert.equal(h.calls[0][3].new_name, "answer");
});

test("each handler discards missing, cancelled, stale, closed, and refused requests", async () => {
  for (const name of ["onSignatureHelp", "onReferences", "onPrepareRename", "onRenameRequest"]) {
    const empty = name === "onReferences" ? [] : null;
    let h = harness(null);
    assert.deepEqual(await h.handlers[name](params), empty);
    h.documents.clear();
    assert.deepEqual(await h.handlers[name](params), empty);
    h = harness(null);
    assert.deepEqual(await h.handlers[name](params, { isCancellationRequested: true }), empty);
    assert.equal(h.calls.length, 0);
    let token = { isCancellationRequested: false };
    h = harness(() => { token.isCancellationRequested = true; return {}; });
    assert.deepEqual(await h.handlers[name](params, token), empty);
    h = harness(() => { h.document.version += 1; return {}; });
    assert.deepEqual(await h.handlers[name](params), empty);
    h = harness(() => { h.documents.clear(); return {}; });
    assert.deepEqual(await h.handlers[name](params), empty);
  }
});
