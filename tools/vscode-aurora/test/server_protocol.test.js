"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { spawn } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const REQUEST_TIMEOUT_MS = 10_000;

function startLanguageServer(serverPath) {
  const env = { ...process.env, PATH: "" };
  if (process.platform === "win32") {
    env.Path = "";
  }
  const child = spawn(process.execPath, [serverPath, "--stdio"], {
    cwd: path.resolve(__dirname, ".."),
    env,
    stdio: ["pipe", "pipe", "pipe"]
  });
  const pending = new Map();
  let nextId = 1;
  let stdout = Buffer.alloc(0);
  let stderr = "";
  let exited = false;

  child.stderr.on("data", (chunk) => {
    stderr = (stderr + chunk.toString()).slice(-65_536);
  });
  child.stdout.on("data", (chunk) => {
    stdout = Buffer.concat([stdout, chunk]);
    try {
      readMessages();
    } catch (error) {
      rejectPending(error);
      child.kill();
    }
  });
  child.on("error", rejectPending);
  child.on("exit", (code, signal) => {
    exited = true;
    if (pending.size > 0) {
      rejectPending(
        new Error(
          `language server exited before replying (code=${code}, signal=${signal})\n${stderr}`
        )
      );
    }
  });

  function rejectPending(error) {
    for (const { reject, timer } of pending.values()) {
      clearTimeout(timer);
      reject(error);
    }
    pending.clear();
  }

  function readMessages() {
    while (true) {
      const headerEnd = stdout.indexOf("\r\n\r\n");
      if (headerEnd < 0) {
        return;
      }
      const header = stdout.subarray(0, headerEnd).toString("ascii");
      const lengthMatch = /(?:^|\r\n)Content-Length:\s*(\d+)/i.exec(header);
      if (!lengthMatch) {
        throw new Error(`language server response omitted Content-Length: ${header}`);
      }
      const bodyLength = Number(lengthMatch[1]);
      const bodyStart = headerEnd + 4;
      const messageEnd = bodyStart + bodyLength;
      if (stdout.length < messageEnd) {
        return;
      }
      const message = JSON.parse(stdout.subarray(bodyStart, messageEnd).toString("utf8"));
      stdout = stdout.subarray(messageEnd);
      if (!Object.prototype.hasOwnProperty.call(message, "id")) {
        continue;
      }
      const request = pending.get(message.id);
      if (!request) {
        continue;
      }
      pending.delete(message.id);
      clearTimeout(request.timer);
      request.resolve(message);
    }
  }

  function send(message) {
    const body = JSON.stringify(message);
    child.stdin.write(`Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`);
  }

  function request(method, params) {
    const id = nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        pending.delete(id);
        reject(new Error(`language server ${method} request timed out\n${stderr}`));
      }, REQUEST_TIMEOUT_MS);
      pending.set(id, { resolve, reject, timer });
      send({ jsonrpc: "2.0", id, method, params });
    });
  }

  function notify(method, params) {
    send({ jsonrpc: "2.0", method, params });
  }

  function dispose() {
    rejectPending(new Error("language server test client disposed"));
    if (!exited) {
      child.kill();
    }
  }

  return {
    request,
    notify,
    dispose,
    stderr: () => stderr
  };
}

test("bundled language server completes safely while a function header is incomplete", async (t) => {
  const serverPath = process.env.AURORA_EXTENSION_SERVER_PATH
    ? path.resolve(process.env.AURORA_EXTENSION_SERVER_PATH)
    : path.resolve(__dirname, "..", "dist", "server.js");
  assert.equal(fs.existsSync(serverPath), true, `language server bundle not found: ${serverPath}`);

  const client = startLanguageServer(serverPath);
  t.after(() => client.dispose());

  const initialize = await client.request("initialize", {
    processId: null,
    rootUri: null,
    capabilities: {},
    workspaceFolders: null
  });
  assert.equal(initialize.error, undefined, JSON.stringify(initialize.error));
  client.notify("initialized", {});

  const source = [
    "import fs",
    "import io",
    "",
    "class Path:",
    "    filepath: str",
    "",
    "",
    "def write_to_path(f_path:)",
    ""
  ].join("\n");
  const uri = "file:///incomplete-function.au";
  client.notify("textDocument/didOpen", {
    textDocument: {
      uri,
      languageId: "aurora",
      version: 1,
      text: source
    }
  });

  const completion = await client.request("textDocument/completion", {
    textDocument: { uri },
    position: {
      line: 7,
      character: source.split("\n")[7].indexOf(":") + 1
    },
    context: { triggerKind: 1 }
  });
  assert.equal(
    completion.error,
    undefined,
    `completion request failed: ${JSON.stringify(completion.error)}\n${client.stderr()}`
  );
  assert.ok(Array.isArray(completion.result), "completion should return a list");
  const labels = new Set(completion.result.map((item) => item.label));
  for (const expected of ["String", "Path", "write_to_path"]) {
    assert.ok(labels.has(expected), `recovery completion should include ${expected}`);
  }
});
