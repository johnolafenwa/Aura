//! Integer operations beyond `+`, `-`, `*`, `//`, and `%`.
//!
//! An integer is a mathematical SMT `Int` inside its type's range, so a
//! bit operation is written as arithmetic. Two's complement makes this
//! exact: bit `i` of `x` is `(x div 2^i) mod 2` for a negative `x` too.
//!
//! - A shift is a multiplication or a floor division by a power of two.
//!   `>>` keeps the sign, as the run time does. The power of a count that
//!   is not a literal is read from `aura.pow2`, a table of the 64 counts.
//! - `x & c` with a literal `c` is the sum of the bit fields `c` selects,
//!   each a `div` and a `mod` by constants. `|` and `^` follow from it:
//!   `x | y = x + y - (x & y)` and `x ^ y = x + y - 2 * (x & y)`.
//! - `x & y` of two values that are not literals is `bitand!`, a function
//!   the solver knows nothing about, with the bounds the signs of `x` and
//!   `y` give stated for each use. A check that fails by the solver's
//!   choice of such a result is reported as not modeled.
//!
//! Bit-vector terms would be exact for that last case. They are not used
//! because one such fact in scope makes unrelated arithmetic checks time
//! out.

use super::exec::{and, lit, Exec, Goal, IntTy, Val};
use super::ObligationKind;
use crate::ast::BinaryOp;
use crate::diag::Span;

/// The `&` of two integers that are not literals.
const BITAND: &str = "bitand!";

/// `2^count` for a shift count in `0..64`.
const POW2: &str = "aura.pow2";

impl IntTy {
    /// The number of bits of the type.
    pub(super) fn width(self) -> u32 {
        (self.hi - self.lo + 1).trailing_zeros()
    }
}

/// The value of an SMT integer literal.
pub(super) fn literal(term: &str) -> Option<i128> {
    match term.strip_prefix("(- ").and_then(|s| s.strip_suffix(')')) {
        Some(rest) => rest.parse::<i128>().ok().map(|value| -value),
        None => term.parse().ok(),
    }
}

/// `x & mask` for a literal `mask`, as arithmetic.
fn and_literal(x: &str, mask: i128) -> String {
    if mask < 0 {
        // A negative mask keeps every bit its complement does not name.
        let cleared = and_literal(x, !mask);
        return if cleared == "0" {
            x.to_string()
        } else {
            format!("(- {x} {cleared})")
        };
    }
    let mut fields = Vec::new();
    let mut bit = 0u32;
    while mask >> bit != 0 {
        let len = (mask >> bit).trailing_ones();
        if len == 0 {
            bit += 1;
            continue;
        }
        let size = 1i128 << len;
        fields.push(if bit == 0 {
            format!("(mod {x} {size})")
        } else {
            let unit = 1i128 << bit;
            format!("(* {unit} (mod (div {x} {unit}) {size}))")
        });
        bit += len;
    }
    match fields.len() {
        0 => "0".to_string(),
        1 => fields.remove(0),
        _ => format!("(+ {})", fields.join(" ")),
    }
}

/// The term of `~a`: every bit of the type's width flipped.
pub(super) fn bitnot(a: &str, ty: IntTy) -> String {
    // For a signed type that is `-a - 1`; an unsigned one counts down
    // from its largest value.
    let top = if ty.lo == 0 { ty.hi } else { -1 };
    match literal(a) {
        Some(value) => lit(top - value),
        None => format!("(- {} {a})", lit(top)),
    }
}

impl Exec<'_> {
    /// The term of an integer `&`, `|`, or `^` of operands of type `ty`.
    pub(super) fn bitwise(&mut self, op: BinaryOp, a: &str, b: &str, ty: IntTy) -> String {
        let common = match (literal(a), literal(b)) {
            (Some(x), Some(y)) => {
                return lit(match op {
                    BinaryOp::BitAnd => x & y,
                    BinaryOp::BitOr => x | y,
                    _ => x ^ y,
                });
            }
            (Some(mask), None) => and_literal(b, mask),
            (None, Some(mask)) => and_literal(a, mask),
            (None, None) if a == b => a.to_string(),
            (None, None) => self.and_unknown(op, a, b, ty),
        };
        match op {
            BinaryOp::BitAnd => common,
            BinaryOp::BitOr => format!("(- (+ {a} {b}) {common})"),
            _ => format!("(- (+ {a} {b}) (* 2 {common}))"),
        }
    }

    /// `a & b` where neither is a literal: an unknown function of the two,
    /// with what their signs say about this use.
    fn and_unknown(&mut self, op: BinaryOp, a: &str, b: &str, ty: IntTy) -> String {
        let what = format!(
            "`{}` of two values that are not constants",
            crate::sema::binary_operator_symbol(op)
        );
        let decl = format!("(declare-fun {BITAND} (Int Int) Int)");
        if !self.decls.contains(&decl) {
            self.decls.push(decl);
            // A use inside a quantifier has no constant of its own, so
            // the function's name marks a check that reads it.
            self.unmodeled_steps.push((what.clone(), self.span));
            self.unmodeled
                .insert(BITAND.to_string(), self.unmodeled_steps.len() - 1);
        }
        // The operation is commutative: one order, so both name one term.
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        let term = format!("({BITAND} {a} {b})");
        if term.contains("k!q") {
            return term;
        }
        let (lo, hi) = (lit(ty.lo), lit(ty.hi));
        self.unmodeled(what, |exec| {
            exec.define_with("Int", "bitand", term, |r| {
                // `a | b` is `a + b - r` and `a ^ b` is `a + b - 2r`;
                // both are values of the type.
                let ranges = format!(
                    "(<= (- (+ {a} {b}) {hi}) {r}) (<= (- (+ {a} {b}) {hi}) (* 2 {r})) (<= (* 2 {r}) (- (+ {a} {b}) {lo}))"
                );
                if ty.lo == 0 {
                    return format!("(and (<= 0 {r}) (<= {r} {a}) (<= {r} {b}) {ranges})");
                }
                // Clearing bits of a value that is not negative keeps it
                // between 0 and itself; of a negative one, lowers it. The
                // `|` of a negative value is negative.
                format!(
                    "(and (=> (>= {a} 0) (and (<= 0 {r}) (<= {r} {a}))) (=> (>= {b} 0) (and (<= 0 {r}) (<= {r} {b}))) (=> (< {a} 0) (or (<= {r} {a}) (>= {b} 0))) (=> (< {b} 0) (or (<= {r} {b}) (>= {a} 0))) (=> (or (< {a} 0) (< {b} 0)) (> {r} (+ {a} {b}))) {ranges})"
                )
            })
        })
    }

    /// The term of `a << b` or `a >> b`, and the condition that the count
    /// is one the run time accepts: from 0 to below the type's width.
    pub(super) fn shift(&mut self, op: BinaryOp, a: &str, b: &str, ty: IntTy) -> (String, String) {
        let width = ty.width();
        let (factor, in_range) = match literal(b) {
            Some(count) if (0..width as i128).contains(&count) => {
                ((1i128 << count).to_string(), "true".to_string())
            }
            // The program stops here, so the value is never read.
            Some(_) => ("1".to_string(), "false".to_string()),
            None => {
                let mut table = "1".to_string();
                for count in (0..64).rev() {
                    table = format!("(ite (= c {count}) {} {table})", 1i128 << count);
                }
                let decl = format!("(define-fun {POW2} ((c Int)) Int {table})");
                if !self.decls.contains(&decl) {
                    self.decls.push(decl);
                }
                (
                    format!("({POW2} {b})"),
                    format!("(and (<= 0 {b}) (< {b} {width}))"),
                )
            }
        };
        let term = match op {
            _ if factor == "1" => a.to_string(),
            BinaryOp::Shl => format!("(* {a} {factor})"),
            // `div` by a positive number rounds down, as a shift that
            // keeps the sign does.
            _ => format!("(div {a} {factor})"),
        };
        (term, in_range)
    }

    /// An integer `&`, `|`, or `^` in a function body.
    pub(super) fn int_bitwise(&mut self, op: BinaryOp, a: &str, b: &str, ty: IntTy) -> Val {
        let term = self.bitwise(op, a, b, ty);
        Val::Int(self.define("Int", "bits", term), ty)
    }

    /// An integer shift in a function body, with the checks the run time
    /// makes: the count is in range, and `<<` loses no bits.
    pub(super) fn int_shift(
        &mut self,
        pc: &str,
        op: BinaryOp,
        (a, b): (&str, &str),
        ty: IntTy,
        span: Span,
    ) -> Val {
        let (term, in_range) = self.shift(op, a, b, ty);
        if in_range != "true" {
            self.oblige(
                ObligationKind::Overflow,
                span,
                "shift amount in range",
                pc,
                Goal::Holds(in_range.clone()),
            );
        }
        let result = self.define("Int", "shift", term);
        if op == BinaryOp::Shl {
            self.oblige(
                ObligationKind::Overflow,
                span,
                "left shift result in range",
                &and(pc, &in_range),
                Goal::Holds(ty.contains(&result)),
            );
        }
        Val::Int(result, ty)
    }

    /// An integer bit operation or shift in a contract. `ty` is the type
    /// of the operand that is not a literal.
    pub(super) fn spec_bits(
        &mut self,
        op: BinaryOp,
        lhs: &Val,
        rhs: &Val,
        literal_left: bool,
        wd: &mut Vec<String>,
    ) -> Result<Val, String> {
        let (Val::Int(a, aty), Val::Int(b, bty)) = (lhs, rhs) else {
            return Err("a bit operation on these operands is not modeled yet".into());
        };
        let ty = if literal_left { *bty } else { *aty };
        if !matches!(op, BinaryOp::Shl | BinaryOp::Shr) {
            return Ok(Val::Int(self.bitwise(op, a, b, ty), ty));
        }
        let (term, in_range) = self.shift(op, a, b, ty);
        if in_range != "true" {
            wd.push(in_range);
        }
        if op == BinaryOp::Shl {
            wd.push(ty.contains(&term));
        }
        Ok(Val::Int(term, ty))
    }
}
