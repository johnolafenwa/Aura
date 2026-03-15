"use strict";

const vscode = require("vscode");
const path = require("node:path");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");

let client;

function activate(context) {
  const serverModule = context.asAbsolutePath(path.join("dist", "server.js"));
  const serverOptions = {
    run: { module: serverModule, transport: TransportKind.ipc },
    debug: {
      module: serverModule,
      transport: TransportKind.ipc,
      options: { execArgv: ["--nolazy", "--inspect=6010"] }
    }
  };

  const clientOptions = {
    documentSelector: [{ language: "aurora", scheme: "file" }],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*.au")
    }
  };

  client = new LanguageClient(
    "auroraLanguageServer",
    "Aurora Language Server",
    serverOptions,
    clientOptions
  );

  context.subscriptions.push(client.start());
}

async function deactivate() {
  if (client) {
    const currentClient = client;
    client = null;
    await currentClient.stop();
  }
}

module.exports = {
  activate,
  deactivate
};
