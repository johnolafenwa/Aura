"use strict";

// Debugs a real program the way VS Code does: `aura build --debug`, then
// the Debug Adapter Protocol with lldb-dap. It runs where both the repo's
// `target/debug/aura` and an lldb-dap exist, and is skipped elsewhere.

const test = require("node:test");
const assert = require("node:assert/strict");
const childProcess = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const { adapterConfiguration, debugBuildArguments, findLldbDap } = require("../src/debug");

const repoRoot = path.resolve(__dirname, "..", "..", "..");
const aura = path.join(repoRoot, "target", "debug", "aura");
const formatter = path.join(__dirname, "..", "debug", "aura_lldb.py");
const adapter = findLldbDap({ configured: process.env.AURA_TEST_LLDB_DAP || "" });
const skip = !fs.existsSync(aura)
  ? "needs target/debug/aura"
  : !adapter
    ? "needs lldb-dap"
    : false;

const PROGRAM = `class Point:
    x: int64
    y: int64

def area(width: int64, height: int64) -> int64:
    product = width * height
    return product

def main() -> None:
    word = "hello"
    p = Point(x=3, y=4)
    names = ["a", "b"]
    total = area(p.x, p.y)
    print(word)
    print(names)
    print(total)
`;

// A Debug Adapter Protocol client over the adapter's stdio.
class DapClient {
  constructor(command) {
    this.process = childProcess.spawn(command, [], { stdio: ["pipe", "pipe", "inherit"] });
    this.sequence = 1;
    this.pending = new Map();
    this.events = [];
    this.waiters = [];
    this.buffer = Buffer.alloc(0);
    this.process.stdout.on("data", (chunk) => this.receive(chunk));
  }

  receive(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    for (;;) {
      const headerEnd = this.buffer.indexOf("\r\n\r\n");
      if (headerEnd < 0) {
        return;
      }
      const length = Number(/Content-Length: (\d+)/i.exec(this.buffer.slice(0, headerEnd))[1]);
      const start = headerEnd + 4;
      if (this.buffer.length < start + length) {
        return;
      }
      const message = JSON.parse(this.buffer.slice(start, start + length).toString("utf8"));
      this.buffer = this.buffer.slice(start + length);
      if (message.type === "response") {
        const resolve = this.pending.get(message.request_seq);
        this.pending.delete(message.request_seq);
        resolve(message);
      } else if (message.type === "event") {
        this.events.push(message);
        this.waiters = this.waiters.filter(({ name, resolve }) => {
          if (message.event === name) {
            resolve(message);
            return false;
          }
          return true;
        });
      }
    }
  }

  request(command, args = {}) {
    const seq = this.sequence++;
    const body = JSON.stringify({ seq, type: "request", command, arguments: args });
    this.process.stdin.write(`Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`);
    return new Promise((resolve) => this.pending.set(seq, resolve));
  }

  event(name) {
    const seen = this.events.findIndex((event) => event.event === name);
    if (seen >= 0) {
      return Promise.resolve(this.events.splice(seen, 1)[0]);
    }
    return new Promise((resolve) => this.waiters.push({ name, resolve }));
  }

  // Continues `threadId` and forgets the stops reported before, since
  // lldb-dap reports a stop once for each stopped thread.
  async resume(threadId) {
    await this.request("continue", { threadId });
    this.events = this.events.filter((event) => event.event !== "stopped");
  }

  close() {
    this.process.kill();
  }
}

async function variablesAt(client, threadId) {
  const stack = await client.request("stackTrace", { threadId, levels: 1 });
  const frame = stack.body.stackFrames[0];
  const scopes = await client.request("scopes", { frameId: frame.id });
  const locals = scopes.body.scopes[0];
  const variables = await client.request("variables", {
    variablesReference: locals.variablesReference
  });
  return { frame, variables: variables.body.variables };
}

test("a debug session stops at breakpoints and shows Aura values", { skip, timeout: 120_000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "aura-debug-session-"));
  const client = new DapClient(adapter);
  try {
    const program = path.join(directory, "app.au");
    const binary = path.join(directory, "app");
    fs.writeFileSync(program, PROGRAM);
    childProcess.execFileSync(aura, debugBuildArguments(program, binary), { stdio: "pipe" });

    const initialized = await client.request("initialize", {
      adapterID: "aura",
      linesStartAt1: true,
      columnsStartAt1: true,
      pathFormat: "path"
    });
    assert.equal(initialized.success, true);
    const launch = client.request(
      "launch",
      adapterConfiguration({ type: "aura", request: "launch", program }, binary, formatter)
    );
    await client.event("initialized");
    const breakpoints = await client.request("setBreakpoints", {
      source: { path: program },
      breakpoints: [{ line: 7 }, { line: 14 }]
    });
    assert.deepEqual(
      breakpoints.body.breakpoints.map((breakpoint) => breakpoint.verified),
      [true, true]
    );
    await client.request("configurationDone");
    assert.equal((await launch).success, true);

    const first = await client.event("stopped");
    const inArea = await variablesAt(client, first.body.threadId);
    assert.equal(inArea.frame.name, "area");
    assert.equal(inArea.frame.line, 7);
    assert.equal(inArea.frame.source.path, program);
    const areaValues = Object.fromEntries(inArea.variables.map((v) => [v.name, v.value]));
    assert.equal(areaValues.width, "3");
    assert.equal(areaValues.height, "4");
    assert.equal(areaValues.product, "12");

    await client.resume(first.body.threadId);
    const second = await client.event("stopped");
    const inMain = await variablesAt(client, second.body.threadId);
    assert.equal(inMain.frame.name, "main");
    assert.equal(inMain.frame.line, 14);
    const mainValues = Object.fromEntries(inMain.variables.map((v) => [v.name, v]));
    assert.equal(mainValues.word.value, '"hello"');
    assert.equal(mainValues.names.value, "[a, b]");
    assert.equal(mainValues.total.value, "12");
    assert.equal(mainValues.p.type, "Point");
    const fields = await client.request("variables", {
      variablesReference: mainValues.p.variablesReference
    });
    assert.deepEqual(
      fields.body.variables.map((field) => [field.name, field.value]),
      [
        ["x", "3"],
        ["y", "4"]
      ]
    );

    await client.resume(second.body.threadId);
    await client.event("terminated");
  } finally {
    client.close();
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
