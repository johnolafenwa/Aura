"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

test("extension bundle contains built extension and language server files", () => {
  const extensionRoot = path.resolve(__dirname, "..");
  const distFiles = ["extension.js", "server.js"];

  for (const filename of distFiles) {
    const fullPath = path.join(extensionRoot, "dist", filename);
    assert.equal(fs.existsSync(fullPath), true, `${filename} should exist in extension/dist`);
  }
});
