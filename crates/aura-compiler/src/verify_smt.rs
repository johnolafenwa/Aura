//! The external SMT solver that `aura verify` talks to (checkpoint D1).
//!
//! The verifier writes SMT-LIB 2 text to Z3 or cvc5 on standard input and
//! reads one answer per query. Nothing is linked into the compiler. The
//! solver is `AURA_SMT_SOLVER` when it is set, else `z3`, else `cvc5`, found
//! on `PATH`.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

/// The environment variable that names the solver to run.
pub const SOLVER_ENV: &str = "AURA_SMT_SOLVER";

/// The solver families the verifier knows how to drive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolverKind {
    Z3,
    Cvc5,
}

/// A solver program found on this machine.
#[derive(Clone, Debug)]
pub struct SolverCommand {
    pub kind: SolverKind,
    pub program: PathBuf,
}

impl SolverCommand {
    /// The solver to run: `explicit` when given, else `AURA_SMT_SOLVER`,
    /// else `z3` or `cvc5` on `PATH`. A value is a program name or a path;
    /// its file name decides whether it is driven as Z3 or as cvc5.
    pub fn find(explicit: Option<&str>) -> Result<Self, String> {
        let from_env = std::env::var(SOLVER_ENV).ok();
        let path = std::env::var_os("PATH").unwrap_or_default();
        Self::find_in(explicit, from_env.as_deref(), &path)
    }

    /// `find` with `AURA_SMT_SOLVER` and `PATH` given.
    pub fn find_in(
        explicit: Option<&str>,
        from_env: Option<&str>,
        path: &std::ffi::OsStr,
    ) -> Result<Self, String> {
        let requested = explicit.or(from_env.filter(|value| !value.is_empty()));
        if let Some(requested) = requested {
            let program = resolve_program(requested, path).ok_or_else(|| {
                format!("SMT solver `{requested}` was not found; install Z3 or cvc5, or set {SOLVER_ENV} to its path")
            })?;
            let kind = kind_of(&program).ok_or_else(|| {
                format!("`{requested}` is not a supported SMT solver; use Z3 or cvc5")
            })?;
            return Ok(Self { kind, program });
        }
        [("z3", SolverKind::Z3), ("cvc5", SolverKind::Cvc5)]
            .into_iter()
            .find_map(|(name, kind)| resolve_program(name, path).map(|program| Self { kind, program }))
            .ok_or_else(|| {
                format!("no SMT solver found; install Z3 or cvc5 on PATH, or set {SOLVER_ENV} to its path")
            })
    }

    /// The solver's version line, for reports and the proof cache key.
    pub fn version(&self) -> String {
        let flag = "--version";
        Command::new(&self.program)
            .arg(flag)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .and_then(|text| text.lines().next().map(str::trim).map(str::to_string))
            .unwrap_or_else(|| "unknown version".to_string())
    }
}

fn kind_of(program: &Path) -> Option<SolverKind> {
    let name = program.file_stem()?.to_str()?.to_ascii_lowercase();
    if name.starts_with("z3") {
        Some(SolverKind::Z3)
    } else if name.starts_with("cvc5") {
        Some(SolverKind::Cvc5)
    } else {
        None
    }
}

fn resolve_program(name: &str, path: &std::ffi::OsStr) -> Option<PathBuf> {
    let program = Path::new(name);
    if program.components().count() > 1 {
        return program.is_file().then(|| program.to_path_buf());
    }
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// What the solver said about one query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SatResult {
    Sat,
    Unsat,
    /// The solver gave up or ran out of time.
    Unknown,
}

/// A running solver process.
pub struct Solver {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    marker: u64,
    /// Whether a write to the process or a read from it found it gone.
    stopped: bool,
}

impl Solver {
    /// Starts `command` with a per-query limit of `timeout_ms`.
    pub fn start(command: &SolverCommand, timeout_ms: u64) -> Result<Self, String> {
        let mut process = Command::new(&command.program);
        match command.kind {
            SolverKind::Z3 => {
                process.args(["-in", "-smt2"]);
            }
            SolverKind::Cvc5 => {
                process.args([
                    "--lang=smt2",
                    "--incremental",
                    "--produce-models",
                    &format!("--tlimit-per={timeout_ms}"),
                ]);
            }
        }
        let mut child = process
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| {
                format!(
                    "could not start SMT solver `{}`: {error}",
                    command.program.display()
                )
            })?;
        let stdin = child.stdin.take().expect("piped solver stdin");
        let stdout = BufReader::new(child.stdout.take().expect("piped solver stdout"));
        let mut solver = Self {
            child,
            stdin,
            stdout,
            marker: 0,
            stopped: false,
        };
        let mut setup = String::from("(set-option :print-success false)\n");
        if command.kind == SolverKind::Z3 {
            setup.push_str("(set-option :produce-models true)\n");
            setup.push_str(&format!("(set-option :timeout {timeout_ms})\n"));
        }
        setup.push_str("(set-logic ALL)\n");
        solver.send(&setup)?;
        Ok(solver)
    }

    /// Writes commands that print nothing.
    pub fn send(&mut self, text: &str) -> Result<(), String> {
        let written = self
            .stdin
            .write_all(format!("{text}\n").as_bytes())
            .and_then(|()| self.stdin.flush());
        written.map_err(|error| {
            let ended = self.ended();
            format!("the SMT solver stopped: {error}{ended}")
        })
    }

    /// Whether the process was found gone: it closed its input or its
    /// output. Nothing more can be asked of it, whether or not it has
    /// finished exiting.
    pub fn stopped(&self) -> bool {
        self.stopped
    }

    /// Records that the process is gone, and says how it ended when that
    /// is known: ` (exit status: 3)`, or nothing.
    fn ended(&mut self) -> String {
        self.stopped = true;
        self.exit_status()
            .map_or(String::new(), |status| format!(" ({status})"))
    }

    /// Sends `text`, then reads everything the solver prints for it.
    fn exchange(&mut self, text: &str) -> Result<String, String> {
        self.marker += 1;
        let marker = format!("aura-done-{}", self.marker);
        self.send(&format!("{text}\n(echo \"{marker}\")"))?;
        let mut output = String::new();
        loop {
            let mut line = String::new();
            let read = self
                .stdout
                .read_line(&mut line)
                .map_err(|error| format!("could not read the SMT solver's answer: {error}"))?;
            if read == 0 {
                let ended = self.ended();
                return Err(format!("the SMT solver exited before answering{ended}"));
            }
            if line.trim().trim_matches('"') == marker {
                return Ok(output);
            }
            output.push_str(&line);
        }
    }

    /// How the solver process ended, once it has: its exit code or the
    /// signal that stopped it. `None` while it still runs. A solver that
    /// closed its output is given a moment to finish exiting.
    pub fn exit_status(&mut self) -> Option<std::process::ExitStatus> {
        for _ in 0..50 {
            match self.child.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(10)),
                Err(_) => return None,
            }
        }
        None
    }

    /// Runs `(check-sat)`.
    pub fn check(&mut self) -> Result<SatResult, String> {
        let answer = self.exchange("(check-sat)")?;
        let answer = answer.trim();
        // A query stopped at the time limit answers `unknown`, after an
        // error that says so. Z3's limit can also cancel a `push` or an
        // `assert` sent before the query, under load, and then it answers
        // for a different set of facts: that answer is not trusted either.
        if answer.contains("canceled") {
            return Ok(SatResult::Unknown);
        }
        if answer.contains("(error") {
            return Err(format!("the SMT solver refused a query: {answer}"));
        }
        Ok(match answer.lines().last().map(str::trim) {
            Some("unsat") => SatResult::Unsat,
            Some("sat") => SatResult::Sat,
            _ => SatResult::Unknown,
        })
    }

    /// The model's value of each term, after a `sat` answer, as the solver
    /// printed them. `None` when the solver gives no model.
    pub fn values(&mut self, terms: &[String]) -> Option<Vec<String>> {
        if terms.is_empty() {
            return Some(Vec::new());
        }
        let answer = self
            .exchange(&format!("(get-value ({}))", terms.join(" ")))
            .ok()?;
        if answer.contains("(error") {
            return None;
        }
        parse_values(&answer, terms.len())
    }
}

impl Drop for Solver {
    fn drop(&mut self) {
        let _ = self.stdin.write_all(b"(exit)\n");
        let _ = self.stdin.flush();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Splits a `get-value` answer `((t1 v1) (t2 v2))` into its values.
fn parse_values(answer: &str, count: usize) -> Option<Vec<String>> {
    let items = parse_sexpr_list(answer.trim())?;
    let values = items
        .into_iter()
        .map(|item| {
            let pair = parse_sexpr_list(&item)?;
            (pair.len() == 2).then(|| pair[1].clone())
        })
        .collect::<Option<Vec<_>>>()?;
    (values.len() == count).then_some(values)
}

/// The top-level items of a parenthesized list.
fn parse_sexpr_list(text: &str) -> Option<Vec<String>> {
    let inner = text.strip_prefix('(')?.strip_suffix(')')?;
    let mut items = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    for ch in inner.chars() {
        match ch {
            '(' => {
                depth += 1;
                current.push(ch);
            }
            ')' => {
                depth = depth.checked_sub(1)?;
                current.push(ch);
                if depth == 0 {
                    items.push(std::mem::take(&mut current).trim().to_string());
                }
            }
            ch if ch.is_whitespace() && depth == 0 => {
                if !current.trim().is_empty() {
                    items.push(std::mem::take(&mut current).trim().to_string());
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        items.push(current.trim().to_string());
    }
    Some(items)
}

/// Renders a solver integer value such as `(- 5)` as `-5`, and a
/// floating-point value such as `(fp #b0 #b01111111111 #x0000000000000)`
/// as the float it stands for.
pub fn render_int(value: &str) -> String {
    let value = value.trim();
    if let Some(float) = render_float(value) {
        return float;
    }
    if let Some(inner) = value
        .strip_prefix("(-")
        .and_then(|rest| rest.strip_suffix(')'))
    {
        format!("-{}", inner.trim())
    } else {
        value.to_string()
    }
}

/// A solver `FloatingPoint` value as a float, when it is one.
fn render_float(value: &str) -> Option<String> {
    if let Some(special) = value.strip_prefix("(_ ") {
        let name = special.split_whitespace().next()?;
        return Some(
            match name {
                "+zero" => "0.0",
                "-zero" => "-0.0",
                "+oo" => "inf",
                "-oo" => "-inf",
                "NaN" => "nan",
                _ => return None,
            }
            .to_string(),
        );
    }
    let fields = parse_sexpr_list(value)?;
    let [head, sign, exponent, significand] = fields.as_slice() else {
        return None;
    };
    if head != "fp" {
        return None;
    }
    let bits = |field: &str| -> Option<(u64, u32)> {
        if let Some(binary) = field.strip_prefix("#b") {
            Some((u64::from_str_radix(binary, 2).ok()?, binary.len() as u32))
        } else {
            let hex = field.strip_prefix("#x")?;
            Some((u64::from_str_radix(hex, 16).ok()?, hex.len() as u32 * 4))
        }
    };
    let (sign, _) = bits(sign)?;
    let (exponent, exponent_width) = bits(exponent)?;
    let (significand, significand_width) = bits(significand)?;
    // A very large or very small value reads better with an exponent than
    // as hundreds of digits.
    let plain = |size: f64| size == 0.0 || (1e-5..1e16).contains(&size) || !size.is_finite();
    let rendered = match (exponent_width, significand_width) {
        (11, 52) => {
            let value = f64::from_bits(sign << 63 | exponent << 52 | significand);
            if plain(value.abs()) {
                value.to_string()
            } else {
                format!("{value:e}")
            }
        }
        (8, 23) => {
            let value = f32::from_bits((sign << 31 | exponent << 23 | significand) as u32);
            if plain(f64::from(value.abs())) {
                value.to_string()
            } else {
                format!("{value:e}")
            }
        }
        _ => return None,
    };
    Some(rendered)
}
