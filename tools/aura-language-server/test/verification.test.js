"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { EventEmitter } = require("node:events");

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const {
  createConcurrencyLimiter,
  createVerificationState,
  parseReport,
  runVerifier,
  verificationDiagnostics,
  verificationHints,
  verifyOnSave
} = require("../src/verification");

// A child process that prints `stdout` and closes, or fails with `error`,
// or runs until it is killed.
function fakeSpawn({ stdout = "", error = null, hang = false, stdinError = false } = {}) {
  const calls = [];
  const spawnImpl = (cmd, args, options) => {
    calls.push({ cmd, args, options });
    const child = new EventEmitter();
    child.stdout = new EventEmitter();
    child.stdin = new EventEmitter();
    child.stdin.end = (input) => {
      calls[calls.length - 1].input = input;
      if (stdinError) {
        child.stdin.emit("error", new Error("EPIPE"));
      }
      setImmediate(() => {
        if (error) {
          child.emit("error", error);
        } else if (!hang) {
          child.stdout.emit("data", Buffer.from(stdout));
          child.emit("close", 1);
        }
      });
    };
    child.kill = () => child.emit("close", null);
    return child;
  };
  return { spawnImpl, calls };
}

const REPORT = {
  schema_version: 1,
  obligations: [
    { kind: "overflow", function: "f", line: 2, column: 12, message: "integer result in range", status: "proved", detail: null },
    { kind: "assert", function: "f", line: 3, column: 5, message: "`assert` condition", status: "proved", detail: null },
    { kind: "overflow", function: "f", line: 3, column: 12, message: "integer result in range", status: "proved", detail: null },
    {
      kind: "ensures",
      function: "f",
      line: 4,
      column: 5,
      message: "`ensures` clause `result > 0`",
      status: "failed",
      detail: [{ name: "n", value: "0" }]
    },
    { kind: "assert", function: "g", line: 5, column: 5, message: "`assert` condition", status: "failed", detail: [] },
    { kind: "assert", function: "g", line: 6, column: 5, message: "`assert` condition", status: "failed", detail: null },
    { kind: "assert", function: "g", line: 7, column: 7, message: "`assert` condition", status: "unknown", detail: null },
    {
      kind: "requires",
      function: "g",
      line: 8,
      column: 5,
      message: "`requires` of `h`",
      status: "unsupported",
      detail: "`str` is not modeled yet"
    },
    { kind: "requires", function: "g", line: 9, column: 1, message: "`requires` of `h`", status: "unsupported", detail: null },
    { kind: "overflow", function: "g", line: 40, column: 1, message: "integer result in range", status: "proved", detail: null }
  ]
};

const SOURCE = [
  "def f(n: int64) -> int64:",
  "    return n + 1  ",
  "    assert n + 1 > n",
  "    ensures result > 0",
  "    assert False",
  "    assert x",
  "    é assert y",
  "    h(s)",
  "h(t)"
].join("\n");

test("the verifier runs on the document text and its report is read", async () => {
  const report = JSON.stringify(REPORT);
  const { spawnImpl, calls } = fakeSpawn({ stdout: `noise\n${report}\n` });
  const command = { cmd: "aura", args: ["--x"], cwd: "/work" };
  const result = await runVerifier(command, "/work/a.au", "source", spawnImpl);
  assert.deepEqual(result, REPORT);
  assert.equal(calls[0].cmd, "aura");
  assert.deepEqual(calls[0].args, ["--x", "verify", "--offline", "--format", "json", "--stdin", "/work/a.au"]);
  assert.equal(calls[0].options.cwd, "/work");
  assert.equal(calls[0].options.env.AURA_OFFLINE, "1");
  assert.equal(calls[0].input, "source");
});

test("the verifier keeps AURA_OFFLINE=1 and passes --offline when AURA_OFFLINE is 0 in the environment", async () => {
  const previousOffline = process.env.AURA_OFFLINE;
  process.env.AURA_OFFLINE = "0";
  try {
    const { spawnImpl, calls } = fakeSpawn({ stdout: `${JSON.stringify(REPORT)}\n` });
    const result = await runVerifier({ cmd: "aura", args: [], cwd: "/work" }, "/work/a.au", "", spawnImpl);
    assert.deepEqual(result, REPORT);
    assert.equal(calls[0].options.env.AURA_OFFLINE, "1");
    assert.ok(calls[0].args.includes("--offline"));
  } finally {
    if (previousOffline === undefined) {
      delete process.env.AURA_OFFLINE;
    } else {
      process.env.AURA_OFFLINE = previousOffline;
    }
  }
});

test("a verifier that cannot start, fails, exceeds stdout cap, aborts, or runs too long gives no report", async () => {
  const throwing = () => {
    throw new Error("no such file");
  };
  assert.equal(await runVerifier({ cmd: "aura", args: [] }, "a.au", "", throwing), null);
  const failing = fakeSpawn({ error: new Error("ENOENT"), stdinError: true });
  assert.equal(await runVerifier({ cmd: "aura", args: [] }, "a.au", "", failing.spawnImpl), null);
  const hanging = fakeSpawn({ hang: true });
  assert.equal(
    await runVerifier({ cmd: "aura", args: [] }, "a.au", "", hanging.spawnImpl, 5),
    null
  );

  const oversized = fakeSpawn({ stdout: `${"x".repeat(64)}\n${JSON.stringify(REPORT)}\n` });
  assert.equal(
    await runVerifier({ cmd: "aura", args: [] }, "a.au", "", oversized.spawnImpl, 1000, {
      maxStdoutBytes: 32
    }),
    null
  );

  const preAborted = new AbortController();
  preAborted.abort();
  const neverCalled = fakeSpawn({ stdout: `${JSON.stringify(REPORT)}\n` });
  assert.equal(
    await runVerifier({ cmd: "aura", args: [] }, "a.au", "", neverCalled.spawnImpl, 1000, {
      signal: preAborted.signal
    }),
    null
  );
  assert.equal(neverCalled.calls.length, 0);

  const midFlight = new AbortController();
  const cancellable = fakeSpawn({ hang: true });
  const pending = runVerifier({ cmd: "aura", args: [] }, "a.au", "", cancellable.spawnImpl, 5000, {
    signal: midFlight.signal
  });
  setImmediate(() => midFlight.abort());
  assert.equal(await pending, null);
});

test("only a JSON report with obligations counts", () => {
  assert.equal(parseReport("not json"), null);
  assert.equal(parseReport("null"), null);
  assert.equal(parseReport('{"diagnostics": []}'), null);
  assert.deepEqual(parseReport('{"obligations": []}'), { obligations: [] });
});

test("a contract that can never hold has its own code", () => {
  const report = {
    obligations: [
      {
        kind: "satisfiable contract",
        function: "bad",
        line: 1,
        column: 1,
        message: "the contract `bad` assumes on entry",
        status: "failed",
        detail: []
      }
    ]
  };
  const diagnostics = verificationDiagnostics(report, "def bad(x: int64):\n    requires x > 0 and x < 0\n");
  assert.deepEqual(
    diagnostics.map((diagnostic) => [diagnostic.severity, diagnostic.code, diagnostic.message]),
    [
      [
        2,
        "AU5120",
        "the contract `bad` assumes on entry can never hold (in `bad`, satisfiable contract check)"
      ]
    ]
  );
});

test("open checks become diagnostics at the check", () => {
  assert.deepEqual(verificationDiagnostics(null, SOURCE), []);
  const diagnostics = verificationDiagnostics(REPORT, SOURCE);
  assert.deepEqual(
    diagnostics.map((diagnostic) => [diagnostic.severity, diagnostic.code, diagnostic.message]),
    [
      [2, "AU5101", "cannot prove `ensures` clause `result > 0` (counterexample: n = 0) (in `f`, ensures check)"],
      [2, "AU5101", "cannot prove `assert` condition (in `g`, assert check)"],
      [2, "AU5101", "cannot prove `assert` condition (in `g`, assert check)"],
      [3, "AU5102", "the solver could not decide `assert` condition (in `g`, assert check)"],
      [3, "AU5104", "cannot check `requires` of `h`: `str` is not modeled yet (in `g`, requires check)"],
      [3, "AU5104", "cannot check `requires` of `h` (in `g`, requires check)"]
    ]
  );
  assert.ok(diagnostics.every((diagnostic) => diagnostic.source === "aura verify"));
  assert.deepEqual(diagnostics[0].range, {
    start: { line: 3, character: 4 },
    end: { line: 3, character: 22 }
  });
  // Byte columns become UTF-16 ones.
  assert.deepEqual(diagnostics[3].range, {
    start: { line: 6, character: 5 },
    end: { line: 6, character: 14 }
  });
  // A check past the end of the text still has a range.
  const past = verificationDiagnostics(
    { obligations: [{ ...REPORT.obligations[3], line: 99, column: 3 }] },
    SOURCE
  );
  assert.deepEqual(past[0].range, {
    start: { line: 98, character: 2 },
    end: { line: 98, character: 2 }
  });
  assert.deepEqual(
    verificationDiagnostics(
      {
        obligations: [
          { ...REPORT.obligations[3], status: "toString" },
          { ...REPORT.obligations[3], status: "constructor" }
        ]
      },
      SOURCE
    ),
    []
  );
});

test("a line whose checks are all proved gets a hint at its end", () => {
  assert.deepEqual(verificationHints(null, SOURCE), []);
  const hints = verificationHints(REPORT, SOURCE);
  assert.deepEqual(
    hints.map((hint) => [hint.position, hint.label, hint.tooltip]),
    [
      [{ line: 1, character: 16 }, "✓ proved", "aura verify proved: overflow"],
      [{ line: 2, character: 20 }, "✓ 2 proved", "aura verify proved: assert, overflow"],
      [{ line: 39, character: 0 }, "✓ proved", "aura verify proved: overflow"]
    ]
  );
  const within = verificationHints(REPORT, SOURCE, {
    start: { line: 2, character: 0 },
    end: { line: 5, character: 0 }
  });
  assert.deepEqual(
    within.map((hint) => hint.position.line),
    [2]
  );
});

test("a report is kept for the version it verified, from the newest run, cancelling older runs", () => {
  const state = createVerificationState();
  assert.equal(state.report("a", 1), null);
  assert.equal(state.signal("a"), null);
  const first = state.start("a");
  const firstSignal = state.signal("a");
  assert.equal(firstSignal.aborted, false);
  const second = state.start("a");
  const secondSignal = state.signal("a");
  assert.equal(firstSignal.aborted, true);
  assert.equal(secondSignal.aborted, false);
  assert.equal(state.finish("a", first, 1, REPORT), false);
  assert.equal(state.report("a", 1), null);
  assert.equal(state.finish("a", second, 2, REPORT), true);
  assert.equal(state.signal("a"), null);
  assert.equal(state.report("a", 2), REPORT);
  assert.equal(state.report("a", 3), null);
  state.delete("a");
  assert.equal(state.report("a", 2), null);

  state.start("b");
  const activeSignal = state.signal("b");
  assert.equal(activeSignal.aborted, false);
  state.delete("b");
  assert.equal(activeSignal.aborted, true);
  assert.equal(state.signal("b"), null);
});

test("verifying on save is on unless turned off", () => {
  assert.equal(verifyOnSave({}), true);
  assert.equal(verifyOnSave({ AURA_LSP_VERIFY: "1" }), true);
  assert.equal(verifyOnSave({ AURA_LSP_VERIFY: "0" }), false);
  assert.equal(typeof verifyOnSave(), "boolean");
});

test("verifier completes without deadlocking when subprocess writes over 128 KiB to stderr", async () => {
  const report = JSON.stringify({ schema_version: 1, obligations: [] });
  const script = `process.stderr.write("e".repeat(200000)); process.stdout.write(${JSON.stringify(`${report}\n`)});`;
  const result = await runVerifier(
    { cmd: process.execPath, args: ["-e", script, "--"] },
    "a.au",
    "",
    undefined,
    5000
  );
  assert.deepEqual(result, { schema_version: 1, obligations: [] });
});

function isZombie(pid) {
  try {
    const stat = fs.readFileSync(`/proc/${pid}/stat`, "utf8");
    return stat.slice(stat.lastIndexOf(")") + 2).startsWith("Z");
  } catch {
    return false;
  }
}

test("aborting a verifier terminates its child process group cleanly on POSIX", async () => {
  if (process.platform === "win32") {
    return;
  }
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "aura-verify-pg-"));
  const pidFile = path.join(dir, "grandchild.pid");
  try {
    const parentScript = [
      "const { spawn } = require('node:child_process');",
      "const fs = require('node:fs');",
      "const grandchild = spawn(process.execPath, ['-e', 'setTimeout(() => {}, 30000)'], { stdio: 'ignore' });",
      `fs.writeFileSync(${JSON.stringify(pidFile)}, String(grandchild.pid));`,
      "setTimeout(() => {}, 30000);"
    ].join("\n");
    const controller = new AbortController();
    const pending = runVerifier(
      { cmd: process.execPath, args: ["-e", parentScript, "--"] },
      "a.au",
      "",
      undefined,
      5000,
      { signal: controller.signal }
    );
    for (let attempt = 0; attempt < 80 && !fs.existsSync(pidFile); attempt += 1) {
      await new Promise((resolve) => setTimeout(resolve, 25));
    }
    const grandchildPid = Number(fs.readFileSync(pidFile, "utf8"));
    assert.ok(Number.isInteger(grandchildPid) && grandchildPid > 0);
    controller.abort();
    assert.equal(await pending, null);
    let alive = false;
    for (let attempt = 0; attempt < 20; attempt += 1) {
      try {
        process.kill(grandchildPid, 0);
        // A killed process that nobody has reaped yet still answers signal 0.
        if (isZombie(grandchildPid)) {
          alive = false;
          break;
        }
        alive = true;
        await new Promise((resolve) => setTimeout(resolve, 25));
      } catch {
        alive = false;
        break;
      }
    }
    if (alive) {
      try {
        process.kill(grandchildPid, "SIGKILL");
      } catch {
        // Already exited.
      }
    }
    assert.equal(alive, false, "grandchild process should be terminated with the verifier group");
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("verifier enforces concurrency limit, handles queued cancellation, and supports cross-platform termination", async () => {
  const limiter = createConcurrencyLimiter(1);
  let active = 0;
  let maxActive = 0;
  const finishQueue = [];
  const spawnControlled = (cmd, args, options) => {
    active += 1;
    maxActive = Math.max(maxActive, active);
    const child = new EventEmitter();
    child.pid = 4242;
    child.stdout = new EventEmitter();
    child.stdin = new EventEmitter();
    child.stdin.end = () => {};
    child.kill = () => {
      active -= 1;
      child.emit("close", null);
    };
    finishQueue.push({
      child,
      options,
      finish(stdout) {
        active -= 1;
        child.stdout.emit("data", stdout);
        child.emit("close", 0);
      }
    });
    return child;
  };

  const first = runVerifier({ cmd: "aura", args: [] }, "a.au", "", spawnControlled, 5000, { limiter });
  const queuedCancel = new AbortController();
  const second = runVerifier({ cmd: "aura", args: [] }, "b.au", "", spawnControlled, 5000, {
    limiter,
    signal: queuedCancel.signal
  });
  const thirdSignal = new AbortController();
  const third = runVerifier({ cmd: "aura", args: [] }, "c.au", "", spawnControlled, 5000, {
    limiter,
    signal: thirdSignal.signal
  });
  const fourth = runVerifier({ cmd: "aura", args: [] }, "d.au", "", spawnControlled, 5000, { limiter });

  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(maxActive, 1);
  assert.equal(finishQueue.length, 1);

  queuedCancel.abort();
  assert.equal(await second, null);

  finishQueue[0].finish(`${JSON.stringify(REPORT)}\n`);
  assert.deepEqual(await first, REPORT);
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(maxActive, 1);
  assert.equal(finishQueue.length, 2);

  // Also check string stdout chunk handling + queued entry without signal.
  finishQueue[1].finish(`${JSON.stringify(REPORT)}\n`);
  assert.deepEqual(await third, REPORT);
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(finishQueue.length, 3);
  finishQueue[2].finish(`${JSON.stringify(REPORT)}\n`);
  assert.deepEqual(await fourth, REPORT);

  // Exercise default limiter construction as well.
  const defaultLimiter = createConcurrencyLimiter();
  const release = await defaultLimiter.acquire();
  release();

  // Exercise POSIX process-group kill success, ESRCH fallback, duplicate abort, and post-abort data chunk.
  const killedGroups = [];
  const posixController = new AbortController();
  let posixChild;
  const posixSpawn = () => {
    posixChild = new EventEmitter();
    posixChild.pid = 777;
    posixChild.stdout = new EventEmitter();
    posixChild.stdin = new EventEmitter();
    posixChild.stdin.end = () => {};
    posixChild.kill = () => {};
    return posixChild;
  };
  const posixRun = runVerifier({ cmd: "aura", args: [] }, "p.au", "", posixSpawn, 5000, {
    signal: posixController.signal,
    platform: "linux",
    killProcess: (pid, sig) => {
      killedGroups.push([pid, sig]);
      posixChild.stdout.emit("data", Buffer.from("ignored after abort"));
      posixChild.emit("close", null);
    }
  });
  await new Promise((resolve) => setImmediate(resolve));
  posixController.abort();
  assert.equal(await posixRun, null);
  assert.deepEqual(killedGroups, [[-777, "SIGTERM"]]);

  // POSIX process-group kill error falls back to child.kill(), and duplicate abort is ignored.
  let directKills = 0;
  let fallbackChild;
  const fallbackSpawn = () => {
    fallbackChild = new EventEmitter();
    fallbackChild.pid = 888;
    fallbackChild.stdout = new EventEmitter();
    fallbackChild.stdin = new EventEmitter();
    fallbackChild.stdin.end = () => {};
    fallbackChild.kill = () => {
      directKills += 1;
    };
    return fallbackChild;
  };
  const fallbackController = new AbortController();
  const fallbackRun = runVerifier({ cmd: "aura", args: [] }, "f.au", "", fallbackSpawn, 5, {
    signal: fallbackController.signal,
    maxStdoutBytes: 2,
    platform: "linux",
    killProcess: () => {
      throw new Error("ESRCH");
    }
  });
  await new Promise((resolve) => setImmediate(resolve));
  fallbackChild.stdout.emit("data", Buffer.from("too-big"));
  fallbackController.abort();
  fallbackChild.emit("close", null);
  assert.equal(await fallbackRun, null);
  assert.equal(directKills, 1);

  // Windows termination invokes taskkill.exe (with SystemRoot set and default fallback, plus spawn throw fallback).
  const winTaskkills = [];
  for (const [env, throwTaskkill] of [[{ SystemRoot: "D:\\Win" }, false], [{}, true]]) {
    const winController = new AbortController();
    let childKills = 0;
    const winSpawn = (cmd, args) => {
      if (cmd.endsWith("taskkill.exe")) {
        winTaskkills.push([cmd, args]);
        if (throwTaskkill) {
          throw new Error("ENOENT");
        }
        return {};
      }
      const child = new EventEmitter();
      child.pid = 999;
      child.stdout = new EventEmitter();
      child.stdin = new EventEmitter();
      child.stdin.end = () => {};
      child.kill = () => {
        childKills += 1;
        child.emit("close", null);
      };
      return child;
    };
    const winRun = runVerifier({ cmd: "aura.exe", args: [] }, "w.au", "", winSpawn, 5000, {
      signal: winController.signal,
      platform: "win32",
      env
    });
    await new Promise((resolve) => setImmediate(resolve));
    winController.abort();
    assert.equal(await winRun, null);
    assert.equal(childKills, 1);
  }
  assert.deepEqual(winTaskkills, [
    ["D:\\Win\\System32\\taskkill.exe", ["/T", "/F", "/PID", "999"]],
    ["C:\\Windows\\System32\\taskkill.exe", ["/T", "/F", "/PID", "999"]]
  ]);
});

