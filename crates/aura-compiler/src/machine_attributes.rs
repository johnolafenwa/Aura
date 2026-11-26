//! Freestanding function attributes, from declaration through object placement.
use std::collections::{BTreeMap, BTreeSet};

use crate::ast::{Decorator, ExprKind, FunctionDecl, ParamMode, Stmt};
use crate::diag::{Diagnostic, Result, Span};
use crate::sema::{FunctionInfo, ModuleNamespace, Program};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineAttributes {
    pub section: Option<String>,
    pub link_name: Option<String>,
    pub align: Option<u32>,
    pub inline: Option<bool>,
    pub used: bool,
    pub interrupt: Option<Interrupt>,
    pub naked: Option<String>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Interrupt {
    X86,
    X86Error,
    Aarch64,
    Riscv,
}

impl Interrupt {
    pub fn arch(self) -> &'static str {
        match self {
            Self::X86 | Self::X86Error => "x86_64",
            Self::Aarch64 => "aarch64",
            Self::Riscv => "riscv64",
        }
    }
}

pub(crate) fn attribute(decorator: &Decorator) -> Option<(&str, &[crate::ast::Argument])> {
    let (head, args) = match &decorator.expr.kind {
        ExprKind::Call { callee, args } => (&callee.kind, args.as_slice()),
        other => (other, &[][..]),
    };
    match head {
        ExprKind::Member { object, field } if matches!(&object.kind, ExprKind::Name(name) if name == "machine") => {
            Some((field, args))
        }
        _ => None,
    }
}

pub(crate) fn valid_symbol(s: &str) -> bool {
    let mut bytes = s.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b"._".contains(&b))
        && bytes.all(|b| b.is_ascii_alphanumeric() || b"._$".contains(&b))
}

fn invalid(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::coded_at("AU2095", span, message)
}

/// Split checked machine attributes from ordinary, callable decorators.
pub(crate) fn attach(decl: &mut FunctionDecl, decorators: Vec<Decorator>) -> Result<()> {
    let Some(first) = decorators.iter().find(|d| attribute(d).is_some()) else {
        decl.decorators = decorators;
        return Ok(());
    };
    let span = first.span;
    let mut attrs = MachineAttributes::default();
    let mut seen = BTreeSet::new();
    for decorator in &decorators {
        let at = decorator.span;
        let (name, args) = attribute(decorator)
            .ok_or_else(|| invalid(at, "a machine function cannot have ordinary decorators"))?;
        if !seen.insert(name) {
            return Err(invalid(at, format!("duplicate machine attribute `{name}`")));
        }
        let flag = args.is_empty() && !matches!(decorator.expr.kind, ExprKind::Call { .. });
        let literal = match args {
            [arg] if arg.name.is_none() => Some(&arg.value.kind),
            _ => None,
        };
        let string = match literal {
            Some(ExprKind::String(s)) => Some(s.as_str()),
            _ => None,
        };
        let symbol = string.filter(|s| valid_symbol(s));
        match (name, literal) {
            ("section", _) if symbol.is_some() => attrs.section = symbol.map(str::to_owned),
            ("link_name", _)
                if symbol.is_some_and(|s| {
                    !s.starts_with(".L") && !s.starts_with("llvm.") && !s.starts_with("aura_fn_")
                }) =>
            {
                attrs.link_name = symbol.map(str::to_owned)
            }
            ("align", Some(ExprKind::Int(n))) if n.is_power_of_two() && *n <= 1 << 29 => {
                attrs.align = Some(*n as u32)
            }
            ("noinline" | "always_inline", _) if flag && attrs.inline.is_none() => {
                attrs.inline = Some(name == "always_inline")
            }
            ("used", _) if flag => attrs.used = true,
            ("naked", _) if flag => {
                attrs.naked = Some(naked_template(decl).ok_or_else(|| {
                    invalid(
                        at,
                        "a naked body must contain exactly one operand-free asm statement",
                    )
                })?)
            }
            ("interrupt", _) => {
                attrs.interrupt = Some(match string {
                    Some("x86_64") => Interrupt::X86,
                    Some("x86_64_error") => Interrupt::X86Error,
                    Some("aarch64") => Interrupt::Aarch64,
                    Some("riscv64") => Interrupt::Riscv,
                    _ => return Err(invalid(
                        at,
                        "expected interrupt convention x86_64, x86_64_error, aarch64 or riscv64",
                    )),
                })
            }
            _ => {
                return Err(invalid(
                    at,
                    format!("invalid or conflicting machine attribute `{name}`"),
                ))
            }
        }
    }
    if (attrs.naked.is_some() || attrs.interrupt.is_some())
        && (attrs.inline == Some(true)
            || (attrs.naked.is_some() && attrs.interrupt.is_some())
            || !decl.contracts.is_empty())
    {
        return Err(invalid(span, "a machine entry cannot combine naked and interrupt, carry contracts or request inlining"));
    }
    decl.machine = Some(Box::new(attrs));
    Ok(())
}

fn naked_template(decl: &FunctionDecl) -> Option<String> {
    let [Stmt::Expr(stmt)] = decl.body.as_slice() else {
        return None;
    };
    let ExprKind::Call { callee, args } = &stmt.expr.kind else {
        return None;
    };
    let [arg] = args.as_slice() else { return None };
    match (&callee.kind, &arg.value.kind) {
        (ExprKind::Name(name), ExprKind::String(text))
            if name == "asm" && arg.name.is_none() && !stmt.discarded =>
        {
            Some(text.clone())
        }
        _ => None,
    }
}

pub(crate) fn check_function(function: &FunctionInfo) -> Result<()> {
    let decl = &function.decl;
    let Some(attrs) = &decl.machine else {
        return Ok(());
    };
    let target = crate::freestanding_check_target().ok_or_else(|| {
        Diagnostic::coded_at(
            "AU2094",
            decl.span,
            "machine attributes require a freestanding build",
        )
    })?;
    if !decl.type_params.is_empty() || decl.lemma || decl.spec {
        return Err(invalid(
            decl.span,
            "machine attributes require a concrete executable function",
        ));
    }
    if let Some(irq) = attrs.interrupt {
        if target.arch() != irq.arch() {
            return Err(Diagnostic::coded_at(
                "AU2096",
                decl.span,
                format!("this interrupt convention requires {}", irq.arch()),
            ));
        }
        let parameters_ok = if irq == Interrupt::X86Error {
            matches!(decl.params.as_slice(), [p] if p.mode != ParamMode::BorrowMut && p.default.is_none())
                && function.signature.params == [crate::sema::Type::named("uint64")]
        } else {
            decl.params.is_empty()
        };
        if !parameters_ok || function.signature.return_type != crate::sema::Type::Unit {
            return Err(invalid(decl.span, "an interrupt handler returns None and takes only the hardware error code, a uint64 in the x86_64_error convention"));
        }
    }
    Ok(())
}

pub(crate) fn check_callable(function: &FunctionInfo, span: Span) -> Result<()> {
    if function
        .decl
        .machine
        .as_ref()
        .is_some_and(|attrs| attrs.interrupt.is_some())
    {
        return Err(Diagnostic::coded_at(
            "AU2096",
            span,
            "an interrupt handler can be entered only by hardware",
        ));
    }
    Ok(())
}

pub(crate) fn collect(program: &Program) -> BTreeMap<String, MachineAttributes> {
    fn visit(ns: &ModuleNamespace, entries: &mut BTreeMap<String, MachineAttributes>) {
        for (name, function) in &ns.all_functions {
            if let Some(attrs) = &function.decl.machine {
                entries.insert(
                    format!("{}::{name}", function.module_name),
                    (**attrs).clone(),
                );
            }
        }
        for child in ns.modules.values() {
            visit(child, entries);
        }
    }
    let mut entries = BTreeMap::new();
    for function in program.functions.values() {
        if function.module_name == program.module_name {
            if let Some(attrs) = &function.decl.machine {
                entries.insert(function.decl.name.clone(), (**attrs).clone());
            }
        }
    }
    for namespace in program.module_registry.values() {
        visit(namespace, &mut entries);
    }
    entries
}
