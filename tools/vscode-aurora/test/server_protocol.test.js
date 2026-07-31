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
  const notificationWaiters = new Map();
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
        const waiters = notificationWaiters.get(message.method) || [];
        const waiterIndex = waiters.findIndex((waiter) => waiter.predicate(message));
        if (waiterIndex >= 0) {
          const [waiter] = waiters.splice(waiterIndex, 1);
          clearTimeout(waiter.timer);
          waiter.resolve(message);
          if (waiters.length === 0) {
            notificationWaiters.delete(message.method);
          }
        }
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

  function waitForNotification(method, predicate = () => true) {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        const waiters = notificationWaiters.get(method) || [];
        const waiterIndex = waiters.findIndex((waiter) => waiter.resolve === resolve);
        if (waiterIndex >= 0) {
          waiters.splice(waiterIndex, 1);
        }
        if (waiters.length === 0) {
          notificationWaiters.delete(method);
        }
        reject(new Error(`language server ${method} notification timed out\n${stderr}`));
      }, REQUEST_TIMEOUT_MS);
      const waiters = notificationWaiters.get(method) || [];
      waiters.push({ predicate, resolve, timer });
      notificationWaiters.set(method, waiters);
    });
  }

  function dispose() {
    rejectPending(new Error("language server test client disposed"));
    for (const waiters of notificationWaiters.values()) {
      for (const waiter of waiters) {
        clearTimeout(waiter.timer);
      }
    }
    notificationWaiters.clear();
    if (!exited) {
      child.kill();
    }
  }

  return {
    request,
    notify,
    waitForNotification,
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
  for (const expected of ["String", "Path", "write_to_path", "yield_now"]) {
    assert.ok(labels.has(expected), `recovery completion should include ${expected}`);
  }
});

test("bundled language server preserves comprehension hover definition and scope", async (t) => {
  const serverPath = process.env.AURORA_EXTENSION_SERVER_PATH
    ? path.resolve(process.env.AURORA_EXTENSION_SERVER_PATH)
    : path.resolve(__dirname, "..", "dist", "server.js");
  assert.equal(fs.existsSync(serverPath), true, `language server bundle not found: ${serverPath}`);

  const client = startLanguageServer(serverPath);
  t.after(() => client.dispose());
  const repoRoot = path.resolve(__dirname, "../../..");
  const repoUri = `file://${repoRoot}`;

  const initialize = await client.request("initialize", {
    processId: null,
    rootUri: repoUri,
    capabilities: {},
    workspaceFolders: [{ uri: repoUri, name: "Aurora" }]
  });
  assert.equal(initialize.error, undefined, JSON.stringify(initialize.error));
  client.notify("initialized", {});

  const lines = [
    "def collect_lengths(groups: Vec[Vec[String]]) -> Vec[int64]:",
    "    lengths = [entry.len() for group in groups if group.len() > 0 for entry in group if entry.contains(\"a\")]",
    "    print(lengths)",
    "    return lengths",
    ""
  ];
  const source = lines.join("\n");
  const uri = `file://${path.join(repoRoot, "comprehension-protocol.au")}`;
  client.notify("textDocument/didOpen", {
    textDocument: {
      uri,
      languageId: "aurora",
      version: 1,
      text: source
    }
  });

  const outputEntryStart = lines[1].indexOf("entry");
  const targetEntryStart = lines[1].indexOf("entry", outputEntryStart + 1);
  const hover = await client.request("textDocument/hover", {
    textDocument: { uri },
    position: { line: 1, character: outputEntryStart + 1 }
  });
  assert.equal(hover.error, undefined, JSON.stringify(hover.error));
  assert.equal(hover.result?.contents?.value, "```aurora\nlocal entry: String\n```");

  const definition = await client.request("textDocument/definition", {
    textDocument: { uri },
    position: { line: 1, character: outputEntryStart + 1 }
  });
  assert.equal(definition.error, undefined, JSON.stringify(definition.error));
  assert.deepEqual(definition.result?.range, {
    start: { line: 1, character: targetEntryStart },
    end: { line: 1, character: targetEntryStart + "entry".length }
  });

  const outputCompletion = await client.request("textDocument/completion", {
    textDocument: { uri },
    position: { line: 1, character: lines[1].indexOf(".") + 1 },
    context: { triggerKind: 2, triggerCharacter: "." }
  });
  assert.equal(outputCompletion.error, undefined, JSON.stringify(outputCompletion.error));
  const outputLabels = new Set(outputCompletion.result.map((item) => item.label));
  assert.ok(outputLabels.has("len"));
  assert.ok(outputLabels.has("contains"));

  const resultHover = await client.request("textDocument/hover", {
    textDocument: { uri },
    position: { line: 3, character: lines[3].indexOf("lengths") + 1 }
  });
  assert.equal(resultHover.error, undefined, JSON.stringify(resultHover.error));
  assert.equal(
    resultHover.result?.contents?.value,
    "```aurora\nbinding lengths: Vec[int64]\n```"
  );

  const afterCompletion = await client.request("textDocument/completion", {
    textDocument: { uri },
    position: { line: 2, character: lines[2].length },
    context: { triggerKind: 1 }
  });
  assert.equal(afterCompletion.error, undefined, JSON.stringify(afterCompletion.error));
  const afterLabels = new Set(afterCompletion.result.map((item) => item.label));
  assert.equal(afterLabels.has("group"), false);
  assert.equal(afterLabels.has("entry"), false);
});

test("bundled language server recovers safely while comprehension clauses are incomplete", async (t) => {
  const serverPath = process.env.AURORA_EXTENSION_SERVER_PATH
    ? path.resolve(process.env.AURORA_EXTENSION_SERVER_PATH)
    : path.resolve(__dirname, "..", "dist", "server.js");
  assert.equal(fs.existsSync(serverPath), true, `language server bundle not found: ${serverPath}`);

  const client = startLanguageServer(serverPath);
  t.after(() => client.dispose());
  const repoRoot = path.resolve(__dirname, "../../..");
  const repoUri = `file://${repoRoot}`;

  const initialize = await client.request("initialize", {
    processId: null,
    rootUri: repoUri,
    capabilities: {},
    workspaceFolders: [{ uri: repoUri, name: "Aurora" }]
  });
  assert.equal(initialize.error, undefined, JSON.stringify(initialize.error));
  client.notify("initialized", {});

  const cases = [
    {
      name: "iterable",
      line: "    result = [value for value in ]",
      message: "expected an iterable expression after `in` in comprehension",
      range: {
        start: { line: 1, character: 33 },
        end: { line: 1, character: 34 }
      }
    },
    {
      name: "filter",
      line: "    result = [value for value in values if ]",
      message: "expected a filter expression after `if` in comprehension",
      range: {
        start: { line: 1, character: 43 },
        end: { line: 1, character: 44 }
      }
    }
  ];

  for (const edit of cases) {
    const lines = [
      "def collect(values: Vec[int64]) -> Vec[int64]:",
      edit.line,
      "    return result",
      ""
    ];
    const source = lines.join("\n");
    const uri = `file://${path.join(repoRoot, `incomplete-comprehension-${edit.name}.au`)}`;
    const diagnosticsPromise = client.waitForNotification(
      "textDocument/publishDiagnostics",
      (message) => message.params?.uri === uri
    );
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
      position: { line: 1, character: lines[1].indexOf("]") },
      context: { triggerKind: 1 }
    });
    assert.equal(
      completion.error,
      undefined,
      `${edit.name} completion failed: ${JSON.stringify(completion.error)}\n${client.stderr()}`
    );
    assert.ok(Array.isArray(completion.result), `${edit.name} completion should return a list`);
    const labels = new Set(completion.result.map((item) => item.label));
    for (const expected of ["collect", "Vec", "if", "yield_now"]) {
      assert.ok(labels.has(expected), `${edit.name} recovery should complete ${expected}`);
    }

    const hover = await client.request("textDocument/hover", {
      textDocument: { uri },
      position: { line: 0, character: lines[0].indexOf("values") + 1 }
    });
    assert.equal(
      hover.error,
      undefined,
      `${edit.name} hover failed: ${JSON.stringify(hover.error)}\n${client.stderr()}`
    );
    assert.equal(
      hover.result,
      null,
      "an incomplete comprehension must not advertise stale checked hover metadata"
    );

    const diagnostics = await diagnosticsPromise;
    assert.equal(diagnostics.params.diagnostics.length, 1);
    assert.deepEqual(
      {
        code: diagnostics.params.diagnostics[0].code,
        message: diagnostics.params.diagnostics[0].message,
        range: diagnostics.params.diagnostics[0].range,
        source: diagnostics.params.diagnostics[0].source
      },
      {
        code: "AU1101",
        message: edit.message,
        range: edit.range,
        source: "aurora-compiler"
      }
    );
  }

  assert.doesNotMatch(
    client.stderr(),
    /Cannot read properties|TypeError|UnhandledPromiseRejection/,
    "incomplete comprehension editor requests must not crash the bundled server"
  );
});
