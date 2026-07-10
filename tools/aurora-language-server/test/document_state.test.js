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

test("document state cache drops closed documents", async () => {
  let analyses = 0;
  const cache = createDocumentStateCache(async (document) => ({
    stamp: ++analyses,
    uri: document.uri
  }));
  const document = {
    uri: "file:///workspace/closed.au",
    version: 1,
    getText() {
      return "print(1)";
    }
  };

  const first = await cache.get(document);
  assert.equal(first.compilerAnalysis.stamp, 1);

  cache.deleteDocument(document.uri);

  const second = await cache.get(document);
  assert.equal(second.compilerAnalysis.stamp, 2);
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

test("document state cache deduplicates in-flight analysis for the same version", async () => {
  let analyses = 0;
  let release;
  const cache = createDocumentStateCache(
    () =>
      new Promise((resolve) => {
        analyses += 1;
        release = resolve;
      })
  );
  const document = {
    uri: "file:///workspace/main.au",
    version: 1,
    getText() {
      return "print(1)";
    }
  };

  const first = cache.get(document);
  const second = cache.get(document);
  assert.equal(analyses, 1);
  release({ version: 1 });
  assert.deepEqual(await first, await second);
});

test("older analysis cannot overwrite a newer document version", async () => {
  const releases = new Map();
  let analyses = 0;
  const cache = createDocumentStateCache(
    (document) =>
      new Promise((resolve) => {
        analyses += 1;
        releases.set(document.version, resolve);
      })
  );
  const versionOne = { uri: "file:///workspace/main.au", version: 1, getText: () => "old" };
  const versionTwo = { uri: "file:///workspace/main.au", version: 2, getText: () => "new" };

  const oldRequest = cache.get(versionOne);
  const newRequest = cache.get(versionTwo);
  releases.get(2)({ version: 2 });
  assert.equal((await newRequest).compilerAnalysis.version, 2);
  releases.get(1)({ version: 1 });
  assert.equal((await oldRequest).compilerAnalysis.version, 1);

  const cachedNew = await cache.get(versionTwo);
  assert.equal(cachedNew.compilerAnalysis.version, 2);
  assert.equal(analyses, 2);
});

test("dependency invalidation touches only a changed document and its dependents", async () => {
  let analyses = 0;
  const cache = createDocumentStateCache(async (document) => ({
    stamp: ++analyses,
    dependencies: document.dependencies || []
  }), (analysis) => analysis.dependencies);
  const util = {
    uri: "file:///workspace/util.au",
    version: 1,
    dependencies: [],
    getText: () => "public def value():\n    return 1"
  };
  const main = {
    uri: "file:///workspace/main.au",
    version: 1,
    dependencies: [util.uri],
    getText: () => "import util"
  };
  const other = {
    uri: "file:///workspace/other.au",
    version: 1,
    dependencies: [],
    getText: () => "print(1)"
  };
  await cache.get(util);
  await cache.get(main);
  const otherBefore = await cache.get(other);

  assert.deepEqual([...cache.invalidate(util.uri)].sort(), [main.uri, util.uri]);
  await cache.get(util);
  await cache.get(main);
  const otherAfter = await cache.get(other);
  assert.equal(otherAfter.compilerAnalysis.stamp, otherBefore.compilerAnalysis.stamp);
  assert.equal(analyses, 5);
});

test("failed analysis is evicted without deleting a newer in-flight version", async () => {
  let rejectOld;
  let analyses = 0;
  const cache = createDocumentStateCache((document) => {
    analyses += 1;
    if (document.version === 1) {
      return new Promise((_resolve, reject) => {
        rejectOld = reject;
      });
    }
    if (document.version === 2) {
      return Promise.resolve({ version: 2 });
    }
    return Promise.reject(new Error("analysis failed"));
  });
  const uri = "file:///workspace/main.au";
  const old = cache.get({ uri, version: 1, getText: () => "old" });
  const current = cache.get({ uri, version: 2, getText: () => "new" });
  rejectOld(new Error("old failed"));
  await assert.rejects(old, /old failed/);
  assert.equal((await current).compilerAnalysis.version, 2);
  assert.equal((await cache.get({ uri, version: 2 })).compilerAnalysis.version, 2);

  const failed = { uri: "file:///workspace/fail.au", version: 3 };
  await assert.rejects(cache.get(failed), /analysis failed/);
  await assert.rejects(cache.get(failed), /analysis failed/);
  assert.equal(analyses, 4);
});

test("dependency invalidation terminates when dependencies form a cycle", async () => {
  const cache = createDocumentStateCache(
    async (document) => ({ dependencies: document.dependencies }),
    (analysis) => analysis.dependencies
  );
  const first = { uri: "file:///first.au", version: 1, dependencies: ["file:///second.au"] };
  const second = { uri: "file:///second.au", version: 1, dependencies: ["file:///first.au"] };
  await cache.get(first);
  await cache.get(second);
  assert.deepEqual([...cache.invalidate(first.uri)].sort(), [first.uri, second.uri].sort());
});
