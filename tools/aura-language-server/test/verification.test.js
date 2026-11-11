"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { EventEmitter } = require("node:events");

const {
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
  assert.deepEqual(calls[0].args, ["--x", "verify", "--format", "json", "--stdin", "/work/a.au"]);
  assert.equal(calls[0].options.cwd, "/work");
  assert.equal(calls[0].options.env.AURA_OFFLINE, "1");
  assert.equal(calls[0].input, "source");
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
