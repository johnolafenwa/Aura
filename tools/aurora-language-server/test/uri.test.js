"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");

const { uriToPath } = require("../src/uri");

test("uri helper preserves UNC and local file paths", () => {
  assert.equal(
    uriToPath("file://server/share/project/main.au", "win32"),
    "\\\\server\\share\\project\\main.au"
  );
  assert.equal(uriToPath("file://server/", "win32"), "\\\\server");
  assert.equal(
    uriToPath("file:///C:/aurora/examples/main.au", "win32"),
    "C:\\aurora\\examples\\main.au"
  );
  assert.equal(uriToPath("file:///Users/test/project/main.au", "darwin"), "/Users/test/project/main.au");
  assert.equal(uriToPath("file://server/share/project/main.au", "linux"), "//server/share/project/main.au");
  assert.equal(uriToPath("file:///tmp/project/main.au", "win32"), "\\tmp\\project\\main.au");
  assert.equal(uriToPath(null), null);
  assert.equal(uriToPath("https://example.com/main.au"), null);
  assert.equal(uriToPath("not-a-file-uri"), null);
});
