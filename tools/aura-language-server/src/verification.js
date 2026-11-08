"use strict";

// Inline proof results: the server runs `aura verify --format json` on a
// saved document and shows each check the verifier could not prove as a
// diagnostic, and each line whose checks are all proved with a hint.

const { spawn } = require("node:child_process");
const { createPositionMapper } = require("./positions");

// A proof may take a while; a run past this is stopped and shows nothing.
const VERIFY_TIMEOUT_MS = 60_000;

// What the editor shows for each status other than `proved`: the
// diagnostic severity (2 warning, 3 information), the code `aura verify`
// reports, and the message.
const OPEN_STATUSES = {
  failed: { severity: 2, code: "AU5101", text: (message) => `cannot prove ${message}` },
  unknown: {
    severity: 3,
    code: "AU5102",
    text: (message) => `the solver could not decide ${message}`
  },
  unsupported: { severity: 3, code: "AU5104", text: (message) => `cannot check ${message}` }
};

// Runs the verifier on `source`, the text of the file at `filePath`. It
// resolves the report, or null when there is none: no solver, a program
// that does not check, a crash, or a run that took too long.
function runVerifier(
  command,
  filePath,
  source,
  spawnImpl = spawn,
  timeoutMs = VERIFY_TIMEOUT_MS
) {
  return new Promise((resolve) => {
    let child;
    try {
      child = spawnImpl(
        command.cmd,
        [...command.args, "verify", "--format", "json", "--stdin", filePath],
        { cwd: command.cwd, stdio: ["pipe", "pipe", "pipe"] }
      );
    } catch {
      resolve(null);
      return;
    }
    let stdout = "";
    const timer = setTimeout(() => child.kill(), timeoutMs);
    child.stdout.on("data", (chunk) => {
      stdout += chunk.toString();
    });
    child.on("error", () => {
      clearTimeout(timer);
      resolve(null);
    });
    child.on("close", () => {
      clearTimeout(timer);
      resolve(parseReport(stdout));
    });
    child.stdin.on("error", () => {});
    child.stdin.end(source);
  });
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
      code: status.code,
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
    const entry = byLine.get(line) || { proved: 0, open: 0, kinds: new Set() };
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
// before the latest one is dropped, and a report is read only for the
// version it verified.
function createVerificationState() {
  const reports = new Map();
  const runs = new Map();
  return {
    start(uri) {
      const run = (runs.get(uri) || 0) + 1;
      runs.set(uri, run);
      return run;
    },
    finish(uri, run, version, report) {
      if (runs.get(uri) !== run) {
        return false;
      }
      reports.set(uri, { version, report });
      return true;
    },
    report(uri, version) {
      const entry = reports.get(uri);
      return entry && entry.version === version ? entry.report : null;
    },
    delete(uri) {
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
  createVerificationState,
  parseReport,
  runVerifier,
  verificationDiagnostics,
  verificationHints,
  verifyOnSave
};
