"use strict";

// Inline proof results: the server runs `aura verify --format json` on a
// saved document and shows each check the verifier could not prove as a
// diagnostic, and each line whose checks are all proved with a hint.

const { spawn } = require("node:child_process");
const path = require("node:path");
const { createPositionMapper } = require("./positions");

// A proof may take a while; a run past this is stopped and shows nothing.
const VERIFY_TIMEOUT_MS = 60_000;
const MAX_VERIFY_STDOUT_BYTES = 16 * 1024 * 1024;
const MAX_CONCURRENT_VERIFIERS = 2;

// Limits how many verifier subprocesses run at once across open documents.
function createConcurrencyLimiter(limit = MAX_CONCURRENT_VERIFIERS) {
  let active = 0;
  const queue = [];
  const release = () => {
    active -= 1;
    const next = queue.shift();
    if (next) {
      active += 1;
      next();
    }
  };
  return {
    acquire(signal) {
      if (signal && signal.aborted) {
        return Promise.resolve(null);
      }
      if (active < limit) {
        active += 1;
        return Promise.resolve(release);
      }
      return new Promise((resolve) => {
        const grant = () => {
          if (signal) {
            signal.removeEventListener("abort", onAbort);
          }
          resolve(release);
        };
        const onAbort = () => {
          queue.splice(queue.indexOf(grant), 1);
          resolve(null);
        };
        if (signal) {
          signal.addEventListener("abort", onAbort, { once: true });
        }
        queue.push(grant);
      });
    }
  };
}

const defaultVerifierLimiter = createConcurrencyLimiter(MAX_CONCURRENT_VERIFIERS);

function terminateVerifierProcess(child, platform, killProcess, spawnImpl, env) {
  if (typeof child.pid === "number" && child.pid > 0) {
    if (platform === "win32") {
      const taskkill = path.win32.join(env.SystemRoot || "C:\\Windows", "System32", "taskkill.exe");
      try {
        spawnImpl(taskkill, ["/T", "/F", "/PID", String(child.pid)], {
          stdio: "ignore",
          windowsHide: true
        });
      } catch {
        // Fall through to direct termination when taskkill is unavailable.
      }
    } else {
      try {
        killProcess(-child.pid, "SIGTERM");
        return;
      } catch {
        // Fall through to direct termination when the process group is gone.
      }
    }
  }
  child.kill();
}

// What the editor shows for each status other than `proved`: the
// diagnostic severity (2 warning, 3 information), the code `aura verify`
// reports, and the message.
const OPEN_STATUSES = Object.assign(Object.create(null), {
  failed: { severity: 2, code: "AU5101", text: (message) => `cannot prove ${message}` },
  unknown: {
    severity: 3,
    code: "AU5102",
    text: (message) => `the solver could not decide ${message}`
  },
  unsupported: { severity: 3, code: "AU5104", text: (message) => `cannot check ${message}` }
});

// Runs the verifier on `source`, the text of the file at `filePath`. It
// resolves the report, or null when there is none: no solver, a program
// that does not check, a crash, a cancelled run, or a run that took too long.
async function runVerifier(
  command,
  filePath,
  source,
  spawnImpl = spawn,
  timeoutMs = VERIFY_TIMEOUT_MS,
  {
    signal = null,
    maxStdoutBytes = MAX_VERIFY_STDOUT_BYTES,
    limiter = defaultVerifierLimiter,
    platform = process.platform,
    killProcess = process.kill,
    env = process.env,
    freestandingTarget = env.AURA_LSP_FREESTANDING_TARGET
  } = {}
) {
  const release = await limiter.acquire(signal);
  if (!release) {
    return null;
  }
  try {
    return await new Promise((resolve) => {
      let child;
      try {
        const targetArgs = [];
        if (freestandingTarget || env.AURA_LSP_FREESTANDING === "1") {
          targetArgs.push("--freestanding");
        }
        if (freestandingTarget) {
          targetArgs.push("--target", freestandingTarget);
        }
        child = spawnImpl(
          command.cmd,
          [
            ...command.args,
            "verify",
            "--offline",
            ...targetArgs,
            "--format",
            "json",
            "--stdin",
            filePath
          ],
          {
            cwd: command.cwd,
            env: { ...env, AURA_OFFLINE: "1" },
            detached: platform !== "win32",
            stdio: ["pipe", "pipe", "ignore"]
          }
        );
      } catch {
        resolve(null);
        return;
      }
      const chunks = [];
      let stdoutBytes = 0;
      let aborted = false;
      const abortChild = () => {
        if (aborted) {
          return;
        }
        aborted = true;
        chunks.length = 0;
        terminateVerifierProcess(child, platform, killProcess, spawnImpl, env);
      };
      const timer = setTimeout(abortChild, timeoutMs);
      if (signal) {
        signal.addEventListener("abort", abortChild, { once: true });
      }
      const cleanup = () => {
        clearTimeout(timer);
        if (signal) {
          signal.removeEventListener("abort", abortChild);
        }
      };
      child.stdout.on("data", (chunk) => {
        if (aborted) {
          return;
        }
        const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(String(chunk), "utf8");
        stdoutBytes += bytes.length;
        if (stdoutBytes > maxStdoutBytes) {
          abortChild();
          return;
        }
        chunks.push(bytes);
      });
      child.on("error", () => {
        cleanup();
        resolve(null);
      });
      child.on("close", () => {
        cleanup();
        resolve(aborted ? null : parseReport(Buffer.concat(chunks).toString("utf8")));
      });
      child.stdin.on("error", () => {});
      child.stdin.end(source);
    });
  } finally {
    release();
  }
}

// The report `aura verify --format json` prints as its last line.
function parseReport(stdout) {
  const last = String(stdout).trim().split("\n").pop();
  try {
    const report = JSON.parse(last);
    return report && Array.isArray(report.obligations) ? report : null;
  } catch {
    return null;
  }
}

// The range a check reported at the 1-based `line` and byte `column` covers:
// from the check to the end of its line.
function checkRange(positions, lines, obligation) {
  const line = Math.max(0, obligation.line - 1);
  const column = Math.max(0, obligation.column - 1);
  const text = lines[line] || "";
  const end = Math.max(column, Buffer.byteLength(text.trimEnd()));
  return positions.range(line, column, end);
}

function verificationDiagnostics(report, source) {
  if (!report) {
    return [];
  }
  const positions = createPositionMapper(source);
  const lines = String(source).split("\n");
  const diagnostics = [];
  for (const obligation of report.obligations) {
    const status = OPEN_STATUSES[obligation.status];
    if (!status) {
      continue;
    }
    let message = status.text(obligation.message);
    let code = status.code;
    // A contract no input meets is its own finding, not a failed check.
    if (obligation.status === "failed" && obligation.kind === "satisfiable contract") {
      message = `${obligation.message} can never hold`;
      code = "AU5120";
    }
    if (obligation.status === "failed" && Array.isArray(obligation.detail)) {
      const values = obligation.detail.map(({ name, value }) => `${name} = ${value}`);
      if (values.length > 0) {
        message += ` (counterexample: ${values.join(", ")})`;
      }
    }
    if (obligation.status === "unsupported" && obligation.detail) {
      message += `: ${obligation.detail}`;
    }
    diagnostics.push({
      severity: status.severity,
      range: checkRange(positions, lines, obligation),
      message: `${message} (in \`${obligation.function}\`, ${obligation.kind} check)`,
      code,
      source: "aura verify"
    });
  }
  return diagnostics;
}

// A hint at the end of each line whose checks are all proved, within the
// 0-based line `range` when one is given.
function verificationHints(report, source, range) {
  if (!report) {
    return [];
  }
  const byLine = new Map();
  for (const obligation of report.obligations) {
    const line = Math.max(0, obligation.line - 1);
    const entry = byLine.get(line) || { proved: 0, kinds: new Set(), open: 0 };
    if (obligation.status === "proved") {
      entry.proved += 1;
      entry.kinds.add(obligation.kind);
    } else {
      entry.open += 1;
    }
    byLine.set(line, entry);
  }
  const positions = createPositionMapper(source);
  const lines = String(source).split("\n");
  const hints = [];
  for (const [line, entry] of [...byLine].sort(([a], [b]) => a - b)) {
    if (entry.open > 0 || (range && (line < range.start.line || line > range.end.line))) {
      continue;
    }
    const text = (lines[line] || "").trimEnd();
    hints.push({
      position: { line, character: positions.toEditorColumn(line, Buffer.byteLength(text)) },
      label: entry.proved === 1 ? "✓ proved" : `✓ ${entry.proved} proved`,
      tooltip: `aura verify proved: ${[...entry.kinds].join(", ")}`,
      paddingLeft: true
    });
  }
  return hints;
}

// The newest verifier report of each document, by version: a run started
// before the latest one is cancelled and dropped, and a report is read only
// for the version it verified.
function createVerificationState() {
  const reports = new Map();
  const runs = new Map();
  const controllers = new Map();
  const cancel = (uri) => {
    const controller = controllers.get(uri);
    if (controller) {
      controller.abort();
      controllers.delete(uri);
    }
  };
  return {
    start(uri) {
      cancel(uri);
      const run = (runs.get(uri) || 0) + 1;
      runs.set(uri, run);
      controllers.set(uri, new AbortController());
      return run;
    },
    signal(uri) {
      const controller = controllers.get(uri);
      return controller ? controller.signal : null;
    },
    finish(uri, run, version, report) {
      if (runs.get(uri) !== run) {
        return false;
      }
      controllers.delete(uri);
      reports.set(uri, { version, report });
      return true;
    },
    report(uri, version) {
      const entry = reports.get(uri);
      return entry && entry.version === version ? entry.report : null;
    },
    delete(uri) {
      cancel(uri);
      reports.delete(uri);
      runs.delete(uri);
    }
  };
}

// Whether the server verifies on save: on unless `AURA_LSP_VERIFY` is `0`.
function verifyOnSave(env = process.env) {
  return env.AURA_LSP_VERIFY !== "0";
}

module.exports = {
  createConcurrencyLimiter,
  createVerificationState,
  parseReport,
  runVerifier,
  verificationDiagnostics,
  verificationHints,
  verifyOnSave
};
