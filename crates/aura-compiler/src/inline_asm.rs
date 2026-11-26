//! The contextual assembly forms and their target-specific LLVM constraints.
use std::collections::BTreeSet;

use crate::ast::{Argument, Expr, ExprKind};
use crate::diag::{Diagnostic, Result, Span};
use crate::FreestandingTarget;
use serde::{Deserialize, Serialize};

pub(crate) fn target_at(span: Span) -> Result<FreestandingTarget> {
    crate::freestanding_check_target().ok_or_else(|| {
        Diagnostic::coded_at(
            "AU2090",
            span,
            "machine assembly requires a freestanding target",
        )
    })
}

fn call(expr: &Expr) -> Option<(&str, &[Argument])> {
    let ExprKind::Call { callee, args } = &expr.kind else {
        return None;
    };
    let ExprKind::Name(name) = &callee.kind else {
        return None;
    };
    Some((name.as_str(), args))
}

pub(crate) fn is_asm(expr: &Expr) -> bool {
    matches!(call(expr), Some(("asm", _)))
}

fn syntax(span: Span, message: &str) -> Diagnostic {
    Diagnostic::coded_at("AU2093", span, message)
}

fn literal(arg: &Argument) -> Result<&str> {
    if let ExprKind::String(text) = &arg.value.kind {
        if arg.name.is_none() && !text.contains('\0') {
            return Ok(text);
        }
    }
    Err(syntax(
        arg.span,
        "expected a literal string without a NUL byte",
    ))
}

pub(crate) fn module_template(expr: &Expr) -> Result<&str> {
    match call(expr) {
        Some(("module_asm", [arg])) => literal(arg),
        _ => Err(syntax(expr.span, "module_asm takes one literal template")),
    }
}

#[derive(Default, Debug)]
pub(crate) struct AsmStatement<'a> {
    pub template: &'a str,
    pub inputs: Vec<(&'a str, &'a Expr)>,
    pub outputs: Vec<(&'a str, &'a Expr)>,
    pub clobbers: Vec<&'a str>,
    pub pure: bool,
    pub preserves_flags: bool,
    pub noreturn: bool,
}

impl<'a> AsmStatement<'a> {
    pub fn parse(expr: &'a Expr) -> Result<Option<Self>> {
        let Some(("asm", args)) = call(expr) else {
            return Ok(None);
        };
        let Some((template, clauses)) = args.split_first() else {
            return Err(syntax(expr.span, "asm needs a literal template"));
        };
        let mut asm = Self {
            template: literal(template)?,
            ..Self::default()
        };
        for clause in clauses {
            let span = clause.span;
            let Some((name, args)) = call(&clause.value).filter(|_| clause.name.is_none()) else {
                return Err(syntax(span, "expected input, output, clobber or options"));
            };
            match (name, args) {
                ("input" | "output", [reg, value]) if value.name.is_none() => {
                    let operands = if name == "input" {
                        &mut asm.inputs
                    } else {
                        &mut asm.outputs
                    };
                    operands.push((literal(reg)?, &value.value));
                }
                ("clobber", [_, ..]) => {
                    for arg in args {
                        asm.clobbers.push(literal(arg)?);
                    }
                }
                ("options", [_, ..]) => {
                    for arg in args {
                        let name = literal(arg)?;
                        let flag = match name {
                            "pure" => &mut asm.pure,
                            "noreturn" => &mut asm.noreturn,
                            "preserves_flags" => &mut asm.preserves_flags,
                            _ => {
                                return Err(Diagnostic::coded_at(
                                    "AU2093",
                                    arg.span,
                                    format!("unknown asm option `{name}`"),
                                ))
                            }
                        };
                        if std::mem::replace(flag, true) {
                            return Err(Diagnostic::coded_at(
                                "AU2093",
                                arg.span,
                                format!("duplicate asm option `{name}`"),
                            ));
                        }
                    }
                }
                _ => return Err(syntax(span, "wrong asm clause or operand count")),
            }
        }
        if (asm.pure && (asm.outputs.is_empty() || asm.clobbers.contains(&"memory")))
            || (asm.noreturn && (!asm.outputs.is_empty() || asm.pure))
        {
            return Err(Diagnostic::coded_at("AU2093", expr.span, "pure assembly needs an output and cannot touch memory; noreturn assembly cannot be pure or return an output"));
        }
        Ok(Some(asm))
    }
}

/// The instruction contract carried into the native backend. LLVM constraints
/// are formed only from checked operand classes, fixed registers and clobbers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MachineAsm {
    pub arch: String,
    pub noreturn: bool,
    pub template: String,
    pub constraints: String,
    pub pure: bool,
    pub input_widths: Vec<u8>,
    pub output_widths: Vec<u8>,
}

pub(crate) fn scalar_width(ty: &crate::sema::Type) -> Option<u8> {
    let crate::sema::Type::Named(name, args) = ty else {
        return None;
    };
    if !args.is_empty() {
        return None;
    }
    match name.as_str() {
        "int8" | "uint8" | "bool" => Some(8),
        "int16" | "uint16" => Some(16),
        "int32" | "uint32" | "float32" => Some(32),
        "int64" | "uint64" | "float64" => Some(64),
        _ => None,
    }
}

pub(crate) fn profile(target: FreestandingTarget) -> &'static str {
    if target == FreestandingTarget::X86_64Kernel {
        "x86_64-kernel"
    } else {
        target.arch()
    }
}

pub(crate) fn operand_width(
    target: FreestandingTarget,
    name: &str,
    ty: &crate::sema::Type,
    span: Span,
) -> Result<u8> {
    let fp = match name {
        "fpreg" => true,
        "reg" => false,
        _ => {
            register(target.arch(), name)
                .ok_or_else(|| {
                    Diagnostic::coded_at(
                        "AU2092",
                        span,
                        format!(
                            "unknown or reserved {} assembly register `{name}`",
                            target.arch()
                        ),
                    )
                })?
                .number
                >= 32
        }
    };
    let float = matches!(ty,crate::sema::Type::Named(name,_) if matches!(name.as_str(),"float32"|"float64"));
    if fp && target == FreestandingTarget::X86_64Kernel {
        return Err(Diagnostic::coded_at(
            "AU2091",
            span,
            "floating-point register operands are unavailable with --kernel",
        ));
    }
    scalar_width(ty).filter(|_| fp == float).ok_or_else(|| {
        Diagnostic::coded_at(
            "AU2091",
            span,
            format!("assembly register `{name}` cannot hold a `{ty}` operand"),
        )
    })
}

impl AsmStatement<'_> {
    pub fn machine(
        &self,
        target: FreestandingTarget,
        input_widths: Vec<u8>,
        output_widths: Vec<u8>,
        span: Span,
    ) -> Result<MachineAsm> {
        let arch = target.arch();
        validate_placeholders(self.template, self, arch, span)?;
        let mut occupied = BTreeSet::new();
        let mut constraints = Vec::new();
        let mut fixed = |name: &str, width: Option<u8>| {
            let reg = register(arch, name).ok_or_else(|| {
                Diagnostic::coded_at(
                    "AU2092",
                    span,
                    format!("unknown or reserved {arch} assembly register `{name}`"),
                )
            })?;
            if target == FreestandingTarget::X86_64Kernel && reg.number >= 32 {
                return Err(Diagnostic::coded_at(
                    "AU2092",
                    span,
                    "floating-point registers are unavailable with --kernel",
                ));
            }
            if width.is_some_and(|width| reg.width.is_some_and(|bits| width != bits)) {
                return Err(Diagnostic::coded_at(
                    "AU2091",
                    span,
                    format!("operand width does not match register `{name}`"),
                ));
            }
            if !occupied.insert(reg.number) {
                return Err(Diagnostic::coded_at(
                    "AU2092",
                    span,
                    format!("overlapping assembly register `{name}`"),
                ));
            }
            Ok(format!("{{{name}}}"))
        };
        for (operands, widths, prefix) in [
            (&self.outputs, &output_widths, "=&"),
            (&self.inputs, &input_widths, ""),
        ] {
            for ((name, _), width) in operands.iter().zip(widths) {
                let constraint = if *name == "reg" {
                    if arch == "x86_64" && *width == 8 {
                        "q"
                    } else {
                        "r"
                    }
                    .to_owned()
                } else if *name == "fpreg" {
                    match arch {
                        "x86_64" => "x",
                        "aarch64" => "w",
                        _ => "f",
                    }
                    .to_owned()
                } else {
                    fixed(name, Some(*width))?
                };
                constraints.push(format!("{prefix}{constraint}"));
            }
        }
        let mut memory = false;
        for name in &self.clobbers {
            if *name == "memory" {
                if std::mem::replace(&mut memory, true) {
                    return Err(Diagnostic::coded_at(
                        "AU2092",
                        span,
                        "duplicate memory clobber",
                    ));
                }
            } else {
                constraints.push(format!("~{}", fixed(name, None)?));
            }
        }
        if !self.pure {
            constraints.push("~{memory}".into());
        }
        if !self.preserves_flags {
            match arch {
                "x86_64" => constraints.push("~{flags}".into()),
                "aarch64" => constraints.push("~{cc}".into()),
                _ => {}
            }
        }
        Ok(MachineAsm {
            arch: arch.into(),
            noreturn: self.noreturn,
            template: self.template.into(),
            constraints: constraints.join(","),
            pure: self.pure,
            input_widths,
            output_widths,
        })
    }
}

struct Register {
    number: usize,
    width: Option<u8>,
}

fn register(arch: &str, name: &str) -> Option<Register> {
    fn indexed(name: &str, prefix: &str, limit: usize) -> Option<usize> {
        let digits = name.strip_prefix(prefix)?;
        let n = digits.parse::<usize>().ok()?;
        (n < limit && digits == n.to_string()).then_some(n)
    }
    let fp = match arch {
        "x86_64" => indexed(name, "xmm", 16).map(|n| (n, None)),
        "aarch64" => indexed(name, "s", 32)
            .map(|n| (n, Some(32)))
            .or_else(|| indexed(name, "d", 32).map(|n| (n, Some(64))))
            .or_else(|| indexed(name, "v", 32).map(|n| (n, None))),
        "riscv64" => {
            const FP: [&str; 32] = [
                "ft0", "ft1", "ft2", "ft3", "ft4", "ft5", "ft6", "ft7", "fs0", "fs1", "fa0", "fa1",
                "fa2", "fa3", "fa4", "fa5", "fa6", "fa7", "fs2", "fs3", "fs4", "fs5", "fs6", "fs7",
                "fs8", "fs9", "fs10", "fs11", "ft8", "ft9", "ft10", "ft11",
            ];
            indexed(name, "f", 32)
                .or_else(|| FP.iter().position(|reg| *reg == name))
                .map(|n| (n, None))
        }
        _ => None,
    };
    if let Some((n, width)) = fp {
        return Some(Register {
            number: n + 32,
            width,
        });
    }
    match arch {
        "x86_64" => {
            const GPRS: [[&str; 4]; 14] = [
                ["al", "ax", "eax", "rax"],
                ["bl", "bx", "ebx", "rbx"],
                ["cl", "cx", "ecx", "rcx"],
                ["dl", "dx", "edx", "rdx"],
                ["sil", "si", "esi", "rsi"],
                ["dil", "di", "edi", "rdi"],
                ["r8b", "r8w", "r8d", "r8"],
                ["r9b", "r9w", "r9d", "r9"],
                ["r10b", "r10w", "r10d", "r10"],
                ["r11b", "r11w", "r11d", "r11"],
                ["r12b", "r12w", "r12d", "r12"],
                ["r13b", "r13w", "r13d", "r13"],
                ["r14b", "r14w", "r14d", "r14"],
                ["r15b", "r15w", "r15d", "r15"],
            ];
            GPRS.iter().enumerate().find_map(|(number, regs)| {
                regs.iter().position(|reg| *reg == name).map(|i| Register {
                    number,
                    width: Some([8, 16, 32, 64][i]),
                })
            })
        }
        "aarch64" => {
            let (digits, width) = name
                .strip_prefix('x')
                .map(|n| (n, 64))
                .or_else(|| name.strip_prefix('w').map(|n| (n, 32)))?;
            let number = digits.parse::<usize>().ok()?;
            (digits == number.to_string() && number < 29 && number != 18).then_some(Register {
                number,
                width: Some(width),
            })
        }
        "riscv64" => {
            const ABI: [&str; 32] = [
                "zero", "ra", "sp", "gp", "tp", "t0", "t1", "t2", "s0", "s1", "a0", "a1", "a2",
                "a3", "a4", "a5", "a6", "a7", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9",
                "s10", "s11", "t3", "t4", "t5", "t6",
            ];
            let number = if let Some(s) = name.strip_prefix('x') {
                let n = s.parse::<usize>().ok()?;
                (n.to_string() == s).then_some(n)?
            } else {
                ABI.iter().position(|reg| *reg == name)?
            };
            ((5..32).contains(&number) && number != 8).then_some(Register {
                number,
                width: None,
            })
        }
        _ => None,
    }
}

fn validate_placeholders(
    template: &str,
    asm: &AsmStatement<'_>,
    arch: &str,
    span: Span,
) -> Result<()> {
    let invalid = || {
        syntax(
            span,
            "invalid assembly placeholder: use $N, ${N:modifier} or $$ for a literal dollar",
        )
    };
    let mut rest = template;
    while let Some((_, tail)) = rest.split_once('$') {
        if let Some(tail) = tail.strip_prefix('$') {
            rest = tail;
            continue;
        }
        let (digits, modifier) = if let Some(braced) = tail.strip_prefix('{') {
            let (field, next) = braced.split_once('}').ok_or_else(invalid)?;
            rest = next;
            let (digits, modifier) = field.split_once(':').ok_or_else(invalid)?;
            (digits, Some(modifier))
        } else {
            let end = tail.bytes().take_while(u8::is_ascii_digit).count();
            rest = &tail[end..];
            (&tail[..end], None)
        };
        if !digits.bytes().all(|n| n.is_ascii_digit()) {
            return Err(invalid());
        }
        let index = digits.parse::<usize>().map_err(|_| invalid())?;
        let (reg, _) = if index < asm.outputs.len() {
            asm.outputs.get(index)
        } else {
            asm.inputs.get(index - asm.outputs.len())
        }
        .ok_or_else(invalid)?;
        if let Some(modifier) = modifier {
            let fp = *reg == "fpreg" || register(arch, reg).is_some_and(|r| r.number >= 32);
            if !matches!(
                (arch, fp, modifier),
                ("x86_64", false, "b" | "w" | "k" | "q")
                    | ("aarch64", false, "w" | "x")
                    | ("aarch64", true, "s" | "d")
            ) {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

pub(crate) fn append_module(program: &crate::Program, plan: &mut crate::ForeignLinkPlan) {
    if program.module.asm_span.is_some() {
        plan.module_asm
            .extend_from_slice(&program.module.module_asm);
        plan.asm_arch = program
            .machine_target
            .map(|target| profile(target).to_owned());
        plan.asm_externs.extend(
            program
                .extern_functions
                .values()
                .map(|f| f.decl.link_name().to_owned()),
        );
    }
}
