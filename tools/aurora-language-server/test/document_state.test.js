"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");

const {
  createDocumentStateCache,
  validateOpenDocuments
} = require("../src/document_state");

test("document state cache re-analyzes unchanged versions after invalidation", async () => {
  let analyses = 0;
  const cache = createDocumentStateCache(async (document) => ({
    stamp: ++analyses,
    uri: document.uri,
    text: document.getText()
  }));
  const document = {
    uri: "file:///workspace/main.au",
    version: 1,
    getText() {
      return "import util";
    }
  };

  const first = await cache.get(document);
  assert.equal(first.compilerAnalysis.stamp, 1);

  const second = await cache.get(document);
  assert.equal(second.compilerAnalysis.stamp, 1);
  assert.equal(analyses, 1);

  cache.invalidateAll();

  const third = await cache.get(document);
  assert.equal(third.compilerAnalysis.stamp, 2);
  assert.equal(analyses, 2);
});

test("validateOpenDocuments revalidates every open document", async () => {
  const visited = [];
  const documents = {
    all() {
      return [{ uri: "file:///workspace/main.au" }, { uri: "file:///workspace/util.au" }];
    }
  };

  await validateOpenDocuments(documents, async (document) => {
    visited.push(document.uri);
  });

  assert.deepEqual(visited, [
    "file:///workspace/main.au",
    "file:///workspace/util.au"
  ]);
});
