"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { pathToFileURL } = require("node:url");

const { canonicalFileUri, uriToPath } = require("../src/uri");

test("uri helper preserves UNC and local file paths", () => {
  assert.equal(
    uriToPath("file://server/share/project/main.au", "win32"),
    "\\\\server\\share\\project\\main.au"
  );
  assert.equal(uriToPath("file://server/", "win32"), "\\\\server");
  assert.equal(
    uriToPath("file:///C:/aura/examples/main.au", "win32"),
    "C:\\aura\\examples\\main.au"
  );
  assert.equal(uriToPath("file:///Users/test/project/main.au", "darwin"), "/Users/test/project/main.au");
  assert.equal(uriToPath("file://server/share/project/main.au", "linux"), "//server/share/project/main.au");
  assert.equal(uriToPath("file:///tmp/project/main.au", "win32"), "\\tmp\\project\\main.au");
  assert.equal(uriToPath(null), null);
  assert.equal(uriToPath("https://example.com/main.au"), null);
  assert.equal(uriToPath("not-a-file-uri"), null);
});

test("canonical file URIs resolve links and keep other URIs", { skip: process.platform === "win32" }, () => {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), "aura-uri-canonical-"));
  try {
    const real = fs.realpathSync.native(temp);
    fs.mkdirSync(path.join(real, "src"));
    fs.writeFileSync(path.join(real, "src", "library.au"), "");
    fs.symlinkSync(path.join(real, "src"), path.join(real, "linked"));
    const canonical = pathToFileURL(path.join(real, "src", "library.au")).toString();

    assert.equal(canonicalFileUri(pathToFileURL(path.join(real, "linked", "library.au")).toString()), canonical);
    assert.equal(canonicalFileUri(canonical), canonical);
    assert.equal(
      canonicalFileUri(pathToFileURL(path.join(real, "linked", "missing.au")).toString()),
      pathToFileURL(path.join(real, "linked", "missing.au")).toString()
    );
    assert.equal(canonicalFileUri("untitled:Untitled-1"), "untitled:Untitled-1");
  } finally {
    fs.rmSync(temp, { recursive: true, force: true });
  }
});
