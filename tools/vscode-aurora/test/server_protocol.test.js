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

test("bundled language server preserves owned slice intelligence and reserved diagnostics", async (t) => {
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
    "def take_slice(values: Vec[String], start: int32, end: int32) -> Vec[String]:",
    "    selected = values[start:end]",
    "    print(values[start:end].len())",
    "    return selected",
    ""
  ];
  const source = lines.join("\n");
  const uri = `file://${path.join(repoRoot, "owned-slice-protocol.au")}`;
  const initialDiagnostics = client.waitForNotification(
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
  assert.deepEqual((await initialDiagnostics).params.diagnostics, []);

  const endpointStart = lines[1].indexOf("start");
  const hover = await client.request("textDocument/hover", {
    textDocument: { uri },
    position: { line: 1, character: endpointStart + 1 }
  });
  assert.equal(hover.error, undefined, JSON.stringify(hover.error));
  assert.equal(hover.result?.contents?.value, "```aurora\nparam start: int32\n```");

  const definition = await client.request("textDocument/definition", {
    textDocument: { uri },
    position: { line: 1, character: endpointStart + 1 }
  });
  assert.equal(definition.error, undefined, JSON.stringify(definition.error));
  const declarationStart = lines[0].indexOf("start");
  assert.deepEqual(definition.result?.range, {
    start: { line: 0, character: declarationStart },
    end: { line: 0, character: declarationStart + "start".length }
  });

  const dot = lines[2].indexOf(".len");
  const completion = await client.request("textDocument/completion", {
    textDocument: { uri },
    position: { line: 2, character: dot + 1 },
    context: { triggerKind: 2, triggerCharacter: "." }
  });
  assert.equal(completion.error, undefined, JSON.stringify(completion.error));
  const labels = new Set(completion.result.map((item) => item.label));
  assert.ok(labels.has("push"));
  assert.ok(labels.has("len"));

  const receiverLines = [
    "def make_values() -> Vec[String]:",
    "    return [\"Ada\", \"Grace\"]",
    "",
    "def endpoint(text: String) -> int32:",
    "    return 0",
    "",
    "def inspect(values: Vec[String]):",
    "    print(make_values()[1:].len())",
    "    print(values[endpoint(\"]\"):].len())",
    ""
  ];
  const receiverUri = `file://${path.join(repoRoot, "owned-slice-receiver-protocol.au")}`;
  const receiverDiagnostics = client.waitForNotification(
    "textDocument/publishDiagnostics",
    (message) => message.params?.uri === receiverUri
  );
  client.notify("textDocument/didOpen", {
    textDocument: {
      uri: receiverUri,
      languageId: "aurora",
      version: 1,
      text: receiverLines.join("\n")
    }
  });
  assert.deepEqual((await receiverDiagnostics).params.diagnostics, []);

  for (const line of [7, 8]) {
    const receiverDot = receiverLines[line].indexOf(".len");
    const receiverCompletion = await client.request("textDocument/completion", {
      textDocument: { uri: receiverUri },
      position: { line, character: receiverDot + 1 },
      context: { triggerKind: 2, triggerCharacter: "." }
    });
    assert.equal(
      receiverCompletion.error,
      undefined,
      JSON.stringify(receiverCompletion.error)
    );
    const receiverLabels = new Set(
      receiverCompletion.result.map((item) => item.label)
    );
    assert.ok(receiverLabels.has("push"), receiverLines[line]);
    assert.ok(receiverLabels.has("len"), receiverLines[line]);
  }

  const steppedLines = [
    lines[0],
    "    selected = values[1:3:2]",
    "    return values[:]",
    ""
  ];
  const steppedSource = steppedLines.join("\n");
  const steppedDiagnostics = client.waitForNotification(
    "textDocument/publishDiagnostics",
    (message) =>
      message.params?.uri === uri &&
      message.params?.diagnostics?.some((diagnostic) => diagnostic.code === "AU2005")
  );
  client.notify("textDocument/didChange", {
    textDocument: { uri, version: 2 },
    contentChanges: [{ text: steppedSource }]
  });
  const diagnostic = (await steppedDiagnostics).params.diagnostics.at(0);
  assert.deepEqual(
    {
      code: diagnostic.code,
      message: diagnostic.message,
      range: diagnostic.range,
      source: diagnostic.source
    },
    {
      code: "AU2005",
      message: "slice steps are unavailable; use an explicit loop to select a stride",
      range: {
        start: { line: 1, character: steppedLines[1].lastIndexOf(":") },
        end: { line: 1, character: steppedLines[1].lastIndexOf(":") + 1 }
      },
      source: "aurora-compiler"
    }
  );

  const recovery = await client.request("textDocument/completion", {
    textDocument: { uri },
    position: { line: 1, character: steppedLines[1].lastIndexOf(":") + 1 },
    context: { triggerKind: 1 }
  });
  assert.equal(recovery.error, undefined, JSON.stringify(recovery.error));
  assert.ok(Array.isArray(recovery.result));

  const endpointLines = [
    "def reject(values: Vec[int32], endpoint: int64):",
    "    print(values[endpoint:])",
    ""
  ];
  const endpointDiagnostics = client.waitForNotification(
    "textDocument/publishDiagnostics",
    (message) =>
      message.params?.uri === uri &&
      message.params?.diagnostics?.some((item) => item.code === "AU2002")
  );
  client.notify("textDocument/didChange", {
    textDocument: { uri, version: 3 },
    contentChanges: [{ text: endpointLines.join("\n") }]
  });
  const endpointDiagnostic = (await endpointDiagnostics).params.diagnostics.find(
    (item) => item.code === "AU2002"
  );
  assert.deepEqual(
    {
      code: endpointDiagnostic.code,
      message: endpointDiagnostic.message,
      range: endpointDiagnostic.range,
      source: endpointDiagnostic.source
    },
    {
      code: "AU2002",
      message: "slice endpoints must have type `int32`, found `int64`",
      range: {
        start: { line: 1, character: 17 },
        end: { line: 1, character: 18 }
      },
      source: "aurora-compiler"
    }
  );

  const assignmentLines = [
    "def replace(values: Vec[int32]):",
    "    values[1:3] = values",
    ""
  ];
  const assignmentMessage =
    "slice assignment is unavailable because slices are owned copies; mutate the source by index or build a new value";
  const assignmentDiagnostics = client.waitForNotification(
    "textDocument/publishDiagnostics",
    (message) =>
      message.params?.uri === uri &&
      message.params?.diagnostics?.some(
        (item) => item.code === "AU2005" && item.message === assignmentMessage
      )
  );
  client.notify("textDocument/didChange", {
    textDocument: { uri, version: 4 },
    contentChanges: [{ text: assignmentLines.join("\n") }]
  });
  const assignmentDiagnostic = (await assignmentDiagnostics).params.diagnostics.find(
    (item) => item.code === "AU2005" && item.message === assignmentMessage
  );
  assert.deepEqual(
    {
      code: assignmentDiagnostic.code,
      message: assignmentDiagnostic.message,
      range: assignmentDiagnostic.range,
      source: assignmentDiagnostic.source
    },
    {
      code: "AU2005",
      message: assignmentMessage,
      range: {
        start: { line: 1, character: 12 },
        end: { line: 1, character: 13 }
      },
      source: "aurora-compiler"
    }
  );

  const incompleteLines = [
    lines[0],
    "    selected = values[:",
    "    return values[:]",
    ""
  ];
  const incompleteDiagnostics = client.waitForNotification(
    "textDocument/publishDiagnostics",
    (message) =>
      message.params?.uri === uri &&
      message.params?.diagnostics?.some((item) => item.code === "AU1001")
  );
  client.notify("textDocument/didChange", {
    textDocument: { uri, version: 5 },
    contentChanges: [{ text: incompleteLines.join("\n") }]
  });
  await incompleteDiagnostics;
  const incompleteCompletion = await client.request("textDocument/completion", {
    textDocument: { uri },
    position: { line: 1, character: incompleteLines[1].length },
    context: { triggerKind: 1 }
  });
  assert.equal(
    incompleteCompletion.error,
    undefined,
    JSON.stringify(incompleteCompletion.error)
  );
  assert.ok(Array.isArray(incompleteCompletion.result));
  assert.doesNotMatch(
    client.stderr(),
    /Cannot read properties|TypeError|UnhandledPromiseRejection/,
    "malformed slice editor requests must not crash the bundled server"
  );
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
