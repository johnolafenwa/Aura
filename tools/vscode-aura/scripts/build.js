"use strict";

const fs = require("node:fs");
const path = require("node:path");
const esbuild = require("esbuild");

const extensionRoot = path.resolve(__dirname, "..");
const repoRoot = path.resolve(extensionRoot, "..", "..");
const distDir = path.join(extensionRoot, "dist");
const clientEntry = path.join(extensionRoot, "src", "extension.js");
const serverEntry = path.join(repoRoot, "tools", "aura-language-server", "src", "server.js");

async function main() {
  fs.rmSync(distDir, { recursive: true, force: true });
  fs.mkdirSync(distDir, { recursive: true });

  await esbuild.build({
    entryPoints: [clientEntry],
    bundle: true,
    format: "cjs",
    platform: "node",
    target: "node20",
    outfile: path.join(distDir, "extension.js"),
    external: ["vscode"]
  });

  await esbuild.build({
    entryPoints: [serverEntry],
    bundle: true,
    format: "cjs",
    platform: "node",
    target: "node20",
    outfile: path.join(distDir, "server.js")
  });

  process.stdout.write("built Aura VS Code extension bundles\n");
}

main().catch((error) => {
  process.stderr.write(`${error.stack || error}\n`);
  process.exitCode = 1;
});
