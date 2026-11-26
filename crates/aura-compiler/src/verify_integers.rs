//! Integer operations beyond `+`, `-`, `*`, `//`, and `%`, casts in
//! contracts, and the `math` module.
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
//!
//! The `wrapping_` and `saturating_` methods, a power with a literal
//! exponent or base, and the rounding and counting functions of `math` are
//! exact too. A power of two unknowns and the other `math` functions here
//! are unknown functions with the facts that hold of each use.
//!
//! Every model was checked against the interpreter on edge values of each
//! integer type; see `verify_integers_tests.rs`.

use super::exec::{and, lit, Exec, FloatTy, Goal, IntTy, Val};
use super::ObligationKind;
use crate::ast::{BinaryOp, TypeRef, TypeRefKind};
use crate::diag::Span;
use crate::runtime_ops::ScalarOp;
use crate::sema::Type;

/// The `&` of two integers that are not literals.
const BITAND: &str = "bitand!";

/// `2^count` for a shift count in `0..64`.
const POW2: &str = "aura.pow2";

/// The `**` of two integers that are not literals.
const INTPOW: &str = "intpow!";

/// The largest literal exponent a power is written out for.
const MAX_EXPONENT: i128 = 63;

impl IntTy {
    /// The number of bits of the type.
    pub(super) fn width(self) -> u32 {
        self.0.bit_width()
    }

    /// Whether the bit operations, shifts, powers and fixed-width methods
    /// here model the type. Their arithmetic counts the values of the
    /// type in an `i128`, which a 128-bit type does not fit.
    pub(super) fn modeled(self) -> bool {
        self.width() < 128
    }

    /// The type's smallest value. Only for a type that is `modeled`.
    pub(super) fn lo(self) -> i128 {
        debug_assert!(self.modeled());
        self.bounds().0
    }

    /// The type's largest value. Only for a type that is `modeled`.
    pub(super) fn hi(self) -> i128 {
        debug_assert!(self.modeled());
        self.bounds().1 as i128
    }
}

/// The operator a fixed-width method computes, and whether a result
/// outside the type wraps around; otherwise it stops at the nearest end.
pub(super) fn fixed_width(op: ScalarOp) -> Option<(BinaryOp, bool)> {
    Some(match op {
        ScalarOp::WrappingAdd => (BinaryOp::Add, true),
        ScalarOp::WrappingSub => (BinaryOp::Sub, true),
        ScalarOp::WrappingMul => (BinaryOp::Mul, true),
        ScalarOp::WrappingShl => (BinaryOp::Shl, true),
        ScalarOp::WrappingShr => (BinaryOp::Shr, true),
        ScalarOp::SaturatingAdd => (BinaryOp::Add, false),
        ScalarOp::SaturatingSub => (BinaryOp::Sub, false),
        ScalarOp::SaturatingMul => (BinaryOp::Mul, false),
        ScalarOp::SaturatingShl => (BinaryOp::Shl, false),
        ScalarOp::SaturatingShr => (BinaryOp::Shr, false),
        _ => return None,
    })
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
    let top = if ty.lo() == 0 { ty.hi() } else { -1 };
    match literal(a) {
        Some(value) => lit(top - value),
        None => format!("(- {} {a})", lit(top)),
    }
}

/// An `as` cast between numbers in a contract, with the checks the run
/// time makes of one in a function body: an integer must be a value of
/// the integer type it converts to and must convert to a float exactly,
/// and a float must be finite with its whole part in the integer type.
pub(super) fn spec_cast(value: Val, ty: &TypeRef, wd: &mut Vec<String>) -> Result<Val, String> {
    let TypeRefKind::Named { name, .. } = &ty.kind else {
        return Err("a cast to this type is not modeled in contracts yet".into());
    };
    let target = Type::Named(name.clone(), Vec::new());
    match (value, IntTy::of(&target), FloatTy::of(&target)) {
        (Val::Int(term, _), Some(int), _) => {
            wd.push(int.contains(&term));
            Ok(Val::Int(term, int))
        }
        (_, Some(int), _) if !int.modeled() => {
            Err("a cast to a 128-bit integer is not modeled in contracts yet".into())
        }
        (Val::Float(term, from), Some(int), _) => {
            wd.push(format!(
                "(and {} {})",
                from.above(&term, int.lo() - 1),
                from.below(&term, int.hi() + 1)
            ));
            let truncated = format!("(to_int (fp.to_real (fp.roundToIntegral RTZ {term})))");
            Ok(Val::Int(truncated, int))
        }
        (Val::Int(term, _), _, Some(to)) => {
            let result = to.round_int(&term);
            let exact = to.exact_ints();
            wd.push(format!(
                "(or (and (<= (- {exact}) {term}) (<= {term} {exact})) (= (fp.to_real {result}) (to_real {term})))"
            ));
            Ok(Val::Float(result, to))
        }
        (Val::Float(term, from), _, Some(to)) => Ok(Val::Float(to.convert(&term, from), to)),
        _ => Err("this cast is not modeled in contracts yet".into()),
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
        let (lo, hi) = (lit(ty.lo()), lit(ty.hi()));
        self.unmodeled(what, |exec| {
            exec.define_with("Int", "bitand", term, |r| {
                // `a | b` is `a + b - r` and `a ^ b` is `a + b - 2r`;
                // both are values of the type.
                let ranges = format!(
                    "(<= (- (+ {a} {b}) {hi}) {r}) (<= (- (+ {a} {b}) {hi}) (* 2 {r})) (<= (* 2 {r}) (- (+ {a} {b}) {lo}))"
                );
                if ty.lo() == 0 {
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
        if !ty.modeled() {
            return Err("a bit operation on a 128-bit integer is not modeled yet".into());
        }
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

    /// The term of a `wrapping_` or `saturating_` method, and the
    /// condition a shift count must meet. Nothing else stops the program.
    pub(super) fn width_term(
        &mut self,
        (op, wraps): (BinaryOp, bool),
        a: &str,
        b: &str,
        ty: IntTy,
    ) -> (String, String) {
        let (exact, in_range) = match op {
            BinaryOp::Add => (format!("(+ {a} {b})"), "true".to_string()),
            BinaryOp::Sub => (format!("(- {a} {b})"), "true".to_string()),
            BinaryOp::Mul => (format!("(* {a} {b})"), "true".to_string()),
            // A right shift stays in the type.
            BinaryOp::Shr => return self.shift(op, a, b, ty),
            _ => self.shift(op, a, b, ty),
        };
        let (lo, hi) = (lit(ty.lo()), lit(ty.hi()));
        let size = ty.hi() - ty.lo() + 1;
        let body = match (wraps, op) {
            // A sum or a difference is off by at most one whole range.
            (true, BinaryOp::Add | BinaryOp::Sub) => {
                format!("(ite (> e {hi}) (- e {size}) (ite (< e {lo}) (+ e {size}) e))")
            }
            (true, _) => format!("(+ {lo} (mod (- e {lo}) {size}))"),
            (false, _) => format!("(ite (> e {hi}) {hi} (ite (< e {lo}) {lo} e))"),
        };
        (format!("(let ((e {exact})) {body})"), in_range)
    }

    /// A `wrapping_` or `saturating_` method call in a function body.
    pub(super) fn int_method(
        &mut self,
        pc: &str,
        kind: (BinaryOp, bool),
        ty: IntTy,
        (receiver, rhs): (Option<Val>, Val),
    ) -> Option<Val> {
        let (Some(Val::Int(a, _)), Val::Int(b, _)) = (receiver, rhs) else {
            return None;
        };
        let (term, in_range) = self.width_term(kind, &a, &b, ty);
        if in_range != "true" {
            let span = self.span;
            self.oblige(
                ObligationKind::Overflow,
                span,
                "shift amount in range",
                pc,
                Goal::Holds(in_range),
            );
        }
        Some(Val::Int(self.define("Int", "width", term), ty))
    }

    /// The term of `a ** b`, the condition that the exponent is not
    /// negative, and the condition that the result is a value of the
    /// type when that is known. `None` when the power is not modeled.
    fn pow_term(&mut self, a: &str, b: &str, ty: IntTy) -> Option<(String, String, String)> {
        if let Some(power) = literal(b) {
            if !(0..=MAX_EXPONENT).contains(&power) {
                // A negative exponent stops the program. A large one is
                // left to the general case.
                return (power < 0).then(|| ("1".into(), "false".into(), "true".into()));
            }
            let term = match power {
                0 => "1".to_string(),
                1 => a.to_string(),
                _ => format!("(* {})", vec![a; power as usize].join(" ")),
            };
            return Some((term.clone(), "true".to_string(), ty.contains(&term)));
        }
        let base = literal(a)?;
        let (table, in_type) = match base {
            // These never leave the type: 0, 1, and -1 repeat.
            0 => (format!("(ite (= {b} 0) 1 0)"), "true".to_string()),
            1 => ("1".to_string(), "true".to_string()),
            -1 => (
                format!("(ite (= (mod {b} 2) 0) 1 (- 1))"),
                "true".to_string(),
            ),
            _ => {
                // The powers that are values of the type, by exponent. A
                // larger base leaves the type sooner, and for good.
                let mut powers = vec![1i128];
                while let Some(next) = powers[powers.len() - 1]
                    .checked_mul(base)
                    .filter(|next| (ty.lo()..=ty.hi()).contains(next))
                {
                    powers.push(next);
                }
                let mut table = "0".to_string();
                for (exponent, power) in powers.iter().enumerate().rev() {
                    table = format!("(ite (= {b} {exponent}) {} {table})", lit(*power));
                }
                (table, format!("(< {b} {})", powers.len()))
            }
        };
        Some((table, format!("(<= 0 {b})"), in_type))
    }

    /// An integer `**` in a function body. The run time stops on a
    /// negative exponent and on a result outside the type.
    pub(super) fn int_pow(&mut self, pc: &str, a: &str, b: &str, ty: IntTy, span: Span) -> Val {
        let Some((term, exponent, in_type)) = self.pow_term(a, b, ty) else {
            return self.pow_unknown(pc, a, b, ty, span);
        };
        if exponent != "true" {
            self.oblige(
                ObligationKind::Overflow,
                span,
                "exponent is not negative",
                pc,
                Goal::Holds(exponent.clone()),
            );
        }
        let result = self.define("Int", "pow", term);
        self.oblige(
            ObligationKind::Overflow,
            span,
            "integer result in range",
            &and(pc, &exponent),
            Goal::Holds(in_type),
        );
        Val::Int(result, ty)
    }

    /// `a ** b` where the exponent is not a literal, nor the base: an
    /// unknown function of the two, with what holds of every power.
    fn pow_unknown(&mut self, pc: &str, a: &str, b: &str, ty: IntTy, span: Span) -> Val {
        let exponent = format!("(<= 0 {b})");
        self.oblige(
            ObligationKind::Overflow,
            span,
            "exponent is not negative",
            pc,
            Goal::Holds(exponent.clone()),
        );
        let pc = and(pc, &exponent);
        self.oblige(
            ObligationKind::Overflow,
            span,
            "integer result in range",
            &pc,
            Goal::Unsupported(
                "`**` of two values that are not constants is not modeled yet".into(),
            ),
        );
        let decl = format!("(declare-fun {INTPOW} (Int Int) Int)");
        if !self.decls.contains(&decl) {
            self.decls.push(decl);
        }
        let result = self.unmodeled("`**` of two values that are not constants", |exec| {
            exec.define_with("Int", "pow", format!("({INTPOW} {a} {b})"), |r| {
                format!(
                    "(=> (>= {b} 0) (and (=> (= {b} 0) (= {r} 1)) (=> (= {b} 1) (= {r} {a})) (=> (>= {a} 0) (>= {r} 0)) (=> (>= {a} 1) (>= {r} 1)) (=> (>= {a} 2) (> {r} {b})) (=> (and (>= {a} 1) (>= {b} 1)) (>= {r} {a})) (=> (and (= {a} 0) (>= {b} 1)) (= {r} 0)) (=> (= {a} 1) (= {r} 1))))"
                )
            })
        });
        // Past this point the program did not stop, so the power is a
        // value of the type.
        self.assume(&pc, ty.contains(&result));
        Val::Int(result, ty)
    }

    /// An integer `**` in a contract.
    pub(super) fn spec_pow(
        &mut self,
        lhs: &Val,
        rhs: &Val,
        literal_left: bool,
        wd: &mut Vec<String>,
    ) -> Result<Val, String> {
        let (Val::Int(a, aty), Val::Int(b, bty)) = (lhs, rhs) else {
            return Err("`**` on these operands is not modeled in contracts yet".into());
        };
        let ty = if literal_left { *bty } else { *aty };
        if !ty.modeled() {
            return Err("`**` on a 128-bit integer is not modeled in contracts yet".into());
        }
        let Some((term, exponent, in_type)) = self.pow_term(a, b, ty) else {
            return Err(
                "`**` of two values that are not constants is not modeled in contracts yet".into(),
            );
        };
        wd.extend([exponent, in_type]);
        Ok(Val::Int(term, ty))
    }
}

// ----- The `math` module ---------------------------------------------------

/// The functions of the `math` module the encoding says something about.
const MATH: [&str; 16] = [
    "floor",
    "ceil",
    "trunc",
    "is_nan",
    "is_inf",
    "is_finite",
    "sqrt",
    "isqrt",
    "factorial",
    "gcd",
    "lcm",
    "comb",
    "perm",
    "sin",
    "cos",
    "exp",
];

/// The name inside `math` of the MIR function `name`, when it is one of
/// those.
pub(super) fn math_function(name: &str) -> Option<&str> {
    let short = name.strip_prefix("math::")?;
    MATH.contains(&short).then_some(short)
}

/// What `math.<name>` gives for some arguments, and each condition the
/// run time checks before it gives a value, with what it is called in a
/// report. The overflow of `lcm`, `comb`, `perm`, and `exp` is not among
/// them: nothing checks it.
type MathValue = (Val, Vec<(String, String)>);

/// `|term|` of an integer.
fn magnitude(term: &str) -> String {
    format!("(ite (< {term} 0) (- {term}) {term})")
}

impl Exec<'_> {
    /// `function` applied to `args`. The solver knows the function only
    /// by what `facts` says of each use. With `what`, that is not all of
    /// its value, and a check that fails by the solver's choice of it is
    /// reported as not modeled.
    fn applied(
        &mut self,
        (function, signature, sort): (&str, &str, &str),
        args: &[&str],
        what: Option<String>,
        facts: impl FnOnce(&str) -> String,
    ) -> String {
        let decl = format!("(declare-fun {function} {signature} {sort})");
        if !self.decls.contains(&decl) {
            self.decls.push(decl);
        }
        let term = format!("({function} {})", args.join(" "));
        match what {
            Some(what) => self.unmodeled(what, |exec| exec.define_with(sort, "math", term, facts)),
            None => self.define_with(sort, "math", term, facts),
        }
    }

    /// The value of `math.<name>(args)`, for a `name` of `MATH` other
    /// than `sqrt`, and what the run time checks first.
    pub(super) fn math_value(&mut self, name: &str, args: &[Val]) -> Option<MathValue> {
        let exact = |value, checks| Some((value, checks));
        let float = FloatTy::F64;
        let what = Some(format!("`math.{name}`"));
        match (name, args) {
            ("floor" | "ceil" | "trunc", [Val::Float(x, from)]) => {
                let x = float.convert(x, *from);
                let mode = match name {
                    "floor" => "RTN",
                    "ceil" => "RTP",
                    _ => "RTZ",
                };
                // The rounded value is an `int64` exactly when the
                // argument is above `-2^63 - 1` and below `2^63`.
                let int = IntTy::INT64;
                let fits = format!(
                    "(and {} {})",
                    float.above(&x, int.lo() - 1),
                    float.below(&x, int.hi() + 1)
                );
                exact(
                    Val::Int(
                        format!("(to_int (fp.to_real (fp.roundToIntegral {mode} {x})))"),
                        int,
                    ),
                    vec![(format!("`math.{name}` result is an `int64`"), fits)],
                )
            }
            ("is_nan" | "is_inf" | "is_finite", [Val::Float(x, _)]) => {
                let term = match name {
                    "is_nan" => format!("(fp.isNaN {x})"),
                    "is_inf" => format!("(fp.isInfinite {x})"),
                    _ => format!("(not (or (fp.isNaN {x}) (fp.isInfinite {x})))"),
                };
                exact(Val::Bool(term), Vec::new())
            }
            ("isqrt", [Val::Int(a, _)]) => {
                // The one integer whose square is at most `a` and whose
                // successor's square is above it.
                let root = self.applied(("isqrt!", "(Int)", "Int"), &[a], None, |r| {
                    format!(
                        "(=> (<= 0 {a}) (and (<= 0 {r}) (<= {r} {a}) (<= {r} 3037000499) (<= (* {r} {r}) {a}) (< {a} (* (+ {r} 1) (+ {r} 1)))))"
                    )
                });
                exact(
                    Val::Int(root, IntTy::INT64),
                    vec![(
                        "`math.isqrt` argument is not negative".to_string(),
                        format!("(<= 0 {a})"),
                    )],
                )
            }
            ("factorial", [Val::Int(n, _)]) => {
                // 20! is the largest factorial an `int64` holds.
                let mut table = "0".to_string();
                let mut products = vec![1i128];
                for factor in 1..=20 {
                    products.push(products[factor - 1] * factor as i128);
                }
                for (count, product) in products.iter().enumerate().rev() {
                    table = format!("(ite (= {n} {count}) {product} {table})");
                }
                exact(
                    Val::Int(table, IntTy::INT64),
                    vec![(
                        "`math.factorial` argument is from 0 to 20".to_string(),
                        format!("(and (<= 0 {n}) (<= {n} 20))"),
                    )],
                )
            }
            ("gcd" | "lcm", [Val::Int(a, _), Val::Int(b, _)]) => {
                // Both are symmetric: one order, so both name one term.
                let (a, b) = if a <= b { (a, b) } else { (b, a) };
                let (size_a, size_b) = (magnitude(a), magnitude(b));
                let lo = lit(IntTy::INT64.lo());
                if name == "gcd" {
                    let result = self.applied(("gcd!", "(Int Int)", "Int"), &[a, b], what, |r| {
                        format!(
                            "(and (<= 0 {r}) (=> (= {a} 0) (= {r} {size_b})) (=> (= {b} 0) (= {r} {size_a})) (=> (not (= {a} 0)) (and (<= 1 {r}) (<= {r} {size_a}))) (=> (not (= {b} 0)) (and (<= 1 {r}) (<= {r} {size_b}))) (=> (= {size_a} {size_b}) (= {r} {size_a})) (=> (or (= {size_a} 1) (= {size_b} 1)) (= {r} 1)))"
                        )
                    });
                    // Only `2^63` is a divisor that is not an `int64`.
                    return exact(
                        Val::Int(result, IntTy::INT64),
                        vec![(
                            "`math.gcd` result is an `int64`".to_string(),
                            format!(
                                "(not (or (and (= {a} {lo}) (or (= {b} 0) (= {b} {lo}))) (and (= {b} {lo}) (= {a} 0))))"
                            ),
                        )],
                    );
                }
                let result = self.applied(("lcm!", "(Int Int)", "Int"), &[a, b], what, |r| {
                    format!(
                        "(and (<= 0 {r}) (=> (or (= {a} 0) (= {b} 0)) (= {r} 0)) (=> (not (or (= {a} 0) (= {b} 0))) (and (<= {size_a} {r}) (<= {size_b} {r}))) (=> (= {size_a} {size_b}) (= {r} {size_a})) (=> (= {size_a} 1) (= {r} {size_b})) (=> (= {size_b} 1) (= {r} {size_a})))"
                    )
                });
                Some((Val::Int(result, IntTy::INT64), Vec::new()))
            }
            ("comb" | "perm", [Val::Int(n, _), Val::Int(k, _)]) => {
                let function = if name == "comb" { "comb!" } else { "perm!" };
                let result = self.applied((function, "(Int Int)", "Int"), &[n, k], what, |r| {
                    // Choosing all of them is one way; arranging them is
                    // not.
                    let all = if name == "comb" {
                        format!(
                            "(=> (= {k} {n}) (= {r} 1)) (=> (and (<= 1 {k}) (< {k} {n})) (<= {n} {r}))"
                        )
                    } else {
                        format!("(=> (and (<= 1 {k}) (<= {k} {n})) (<= {n} {r}))")
                    };
                    format!(
                        "(=> (and (<= 0 {n}) (<= 0 {k})) (and (<= 0 {r}) (=> (< {n} {k}) (= {r} 0)) (=> (<= {k} {n}) (<= 1 {r})) (=> (= {k} 0) (= {r} 1)) (=> (= {k} 1) (= {r} {n})) {all}))"
                    )
                });
                exact(
                    Val::Int(result, IntTy::INT64),
                    vec![(
                        format!("`math.{name}` arguments are not negative"),
                        format!("(and (<= 0 {n}) (<= 0 {k}))"),
                    )],
                )
            }
            ("sin" | "cos" | "exp", [Val::Float(x, from)]) => {
                let x = float.convert(x, *from);
                let (one, zero) = (FloatTy::literal(1.0), FloatTy::literal(0.0));
                let function = format!("{name}!");
                let bounds = |r: &str| {
                    if name == "exp" {
                        format!("(fp.leq {zero} {r})")
                    } else {
                        format!("(and (fp.leq (fp.neg {one}) {r}) (fp.leq {r} {one}))")
                    }
                };
                let result = self.applied((&function, "(Float64)", "Float64"), &[&x], what, |r| {
                    // NaN gives NaN, and nothing else does.
                    format!(
                        "(and (= (fp.isNaN {r}) (fp.isNaN {x})) (=> (not (fp.isNaN {x})) {}))",
                        bounds(r)
                    )
                });
                let value = Val::Float(result, float);
                if name == "exp" {
                    return Some((value, Vec::new()));
                }
                exact(
                    value,
                    vec![(
                        format!("`math.{name}` argument is not infinite"),
                        format!("(not (fp.isInfinite {x}))"),
                    )],
                )
            }
            _ => None,
        }
    }

    /// A call of `math.<name>` in a function body.
    pub(super) fn math_call(&mut self, pc: &str, name: &str, args: &[Val]) -> Option<Val> {
        let span = self.span;
        if let ("sqrt", [Val::Float(x, FloatTy::F64)]) = (name, args) {
            // The run time refuses a negative argument; `-0.0` is not one.
            self.oblige(
                ObligationKind::Overflow,
                span,
                "`math.sqrt` argument is not negative",
                pc,
                Goal::Holds(format!(
                    "(not (and (fp.isNegative {x}) (not (fp.isZero {x}))))"
                )),
            );
            let root = self.define_rounded(FloatTy::F64, super::float::RoundedOp::Sqrt(x.clone()));
            return Some(Val::Float(root, FloatTy::F64));
        }
        let (value, checks) = self.math_value(name, args)?;
        let mut reached = pc.to_string();
        for (message, check) in checks {
            self.oblige(
                ObligationKind::Overflow,
                span,
                message,
                pc,
                Goal::Holds(check.clone()),
            );
            reached = and(&reached, &check);
        }
        Some(match value {
            Val::Int(term, ty) => {
                let result = self.define("Int", "math", term);
                // Past the call the program did not stop, so the result
                // is a value of the type.
                self.assume(&reached, ty.contains(&result));
                Val::Int(result, ty)
            }
            value => value,
        })
    }
}
