//! A cheaper first query for checks that read float arithmetic.
//!
//! The solver handles an IEEE-754 `+`, `-`, `*`, `/`, or square root by
//! building the circuit that computes it, which can take it longer than the
//! time limit even when the check follows from simple facts, such as
//! `x / y <= x` for `y >= 1.0`. So a check whose function computes with
//! floats is first asked of a weaker problem: each such result is a value of
//! its own, with only facts that the rounded operation is known to have,
//! such as its sign, when it is NaN, how its size compares with its
//! operands and with 1, and its bounds where its operands are bounded by
//! literals of the function. Every fact holds of the exact operation, so a
//! check proved this way is proved. A check the weaker problem does not
//! prove is asked again of the exact one, which also finds the
//! counterexample of a check that fails.

use super::exec::FloatTy;
use std::collections::BTreeSet;

/// A float `+`, `-`, `*`, `/`, or `sqrt` result, defined by the fact at
/// `fact`.
#[derive(Clone, Debug)]
pub(super) struct Rounded {
    pub(super) fact: usize,
    pub(super) result: String,
    pub(super) op: RoundedOp,
    pub(super) float: FloatTy,
}

#[derive(Clone, Debug)]
pub(super) enum RoundedOp {
    Add(String, String),
    Sub(String, String),
    Mul(String, String),
    Div(String, String),
    Sqrt(String),
}

/// The most literals each result is related to, so the weaker problem
/// stays small.
const MAX_LITERALS: usize = 8;

/// Each finite, non-negative `float64` literal in `texts`, smallest first.
pub(super) fn literals<'t>(texts: impl IntoIterator<Item = &'t str>) -> Vec<f64> {
    let mut found = BTreeSet::new();
    for text in texts {
        let mut rest = text;
        while let Some(at) = rest.find("(fp #b") {
            rest = &rest[at..];
            let end = rest.find(')').unwrap_or(rest.len());
            if let Some(value) = parse_literal(&rest[..end]) {
                if value.is_finite() && value >= 0.0 {
                    found.insert(value.to_bits());
                }
            }
            rest = &rest[end.min(rest.len())..];
        }
    }
    let mut values: Vec<f64> = found.into_iter().map(f64::from_bits).collect();
    values.sort_by(f64::total_cmp);
    values.truncate(MAX_LITERALS);
    values
}

/// The value of a `float64` literal `(fp #bS #bE.. #bM..` without its
/// closing parenthesis.
fn parse_literal(text: &str) -> Option<f64> {
    let fields: Vec<&str> = text.strip_prefix("(fp ")?.split_whitespace().collect();
    let [sign, exponent, significand] = fields.as_slice() else {
        return None;
    };
    let bits = |field: &str, width: usize| {
        let digits = field.strip_prefix("#b")?;
        (digits.len() == width)
            .then(|| u64::from_str_radix(digits, 2).ok())
            .flatten()
    };
    let (sign, exponent, significand) =
        (bits(sign, 1)?, bits(exponent, 11)?, bits(significand, 52)?);
    Some(f64::from_bits(sign << 63 | exponent << 52 | significand))
}

/// The SMT literal of `value` rounded to `float`.
fn literal(float: FloatTy, value: f64) -> String {
    match float {
        FloatTy::F64 => FloatTy::literal(value),
        FloatTy::F32 => {
            let bits = (value as f32).to_bits();
            format!(
                "(fp #b{} #b{:08b} #b{:023b})",
                bits >> 31,
                (bits >> 23) & 0xff,
                bits & ((1 << 23) - 1)
            )
        }
    }
}

/// `a / c` or `sqrt(c)` in `float`, rounded as the program rounds it.
fn rounded(
    float: FloatTy,
    value: impl Fn(f64) -> f64,
    value32: impl Fn(f32) -> f32,
) -> impl Fn(f64) -> f64 {
    move |c| match float {
        FloatTy::F64 => value(c),
        FloatTy::F32 => value32(c as f32) as f64,
    }
}

/// The facts the weaker problem has about `rounded`, in place of its
/// definition.
pub(super) fn axioms(rounded_op: &Rounded, literals: &[f64]) -> Vec<String> {
    let float = rounded_op.float;
    let d = &rounded_op.result;
    let zero = literal(float, 0.0);
    let one = literal(float, 1.0);
    let lit = |value: f64| literal(float, value);
    let literals = &values_in(float, literals);
    let mut facts = Vec::new();
    match &rounded_op.op {
        RoundedOp::Div(x, y) => {
            let number = format!("(not (fp.isNaN {d}))");
            facts.push(format!(
                "(= (fp.isNaN {d}) (or (fp.isNaN {x}) (fp.isNaN {y}) (and (fp.isZero {x}) (fp.isZero {y})) (and (fp.isInfinite {x}) (fp.isInfinite {y}))))"
            ));
            facts.push(format!(
                "(=> {number} (= (fp.isNegative {d}) (xor (fp.isNegative {x}) (fp.isNegative {y}))))"
            ));
            facts.push(format!(
                "(=> (and {number} (fp.geq (fp.abs {y}) {one})) (fp.leq (fp.abs {d}) (fp.abs {x})))"
            ));
            facts.push(format!(
                "(=> (and {number} (fp.leq (fp.abs {y}) {one})) (fp.geq (fp.abs {d}) (fp.abs {x})))"
            ));
            facts.push(format!(
                "(=> (and {number} (fp.leq (fp.abs {x}) (fp.abs {y}))) (fp.leq (fp.abs {d}) {one}))"
            ));
            facts.push(format!(
                "(=> (and {number} (fp.geq (fp.abs {x}) (fp.abs {y}))) (fp.geq (fp.abs {d}) {one}))"
            ));
            facts.push(format!("(=> (fp.eq {y} {one}) (= {d} {x}))"));
            facts.push(format!(
                "(=> (and (fp.isZero {x}) (not (fp.isZero {y})) (not (fp.isNaN {y}))) (fp.isZero {d}))"
            ));
            facts.push(format!(
                "(=> (and (fp.isInfinite {y}) (not (fp.isInfinite {x})) (not (fp.isNaN {x}))) (fp.isZero {d}))"
            ));
            facts.push(format!(
                "(=> (and (fp.isZero {y}) (not (fp.isZero {x})) (not (fp.isNaN {x}))) (fp.isInfinite {d}))"
            ));
            facts.push(format!(
                "(=> (and (fp.isInfinite {x}) (not (fp.isInfinite {y})) (not (fp.isNaN {y}))) (fp.isInfinite {d}))"
            ));
            // Division is monotone in each operand, and rounding keeps
            // the order, so bounds of the operands bound the result.
            for &a in literals {
                for &c in literals.iter().filter(|c| **c > 0.0) {
                    let q = match float {
                        FloatTy::F64 => lit(a / c),
                        FloatTy::F32 => lit(((a as f32) / (c as f32)) as f64),
                    };
                    let (a, c) = (lit(a), lit(c));
                    let neg_a = format!("(fp.neg {a})");
                    let neg_q = format!("(fp.neg {q})");
                    facts.push(format!(
                        "(=> (and (fp.leq {zero} {x}) (fp.leq {x} {a}) (fp.geq {y} {c})) (fp.leq {d} {q}))"
                    ));
                    facts.push(format!(
                        "(=> (and (fp.geq {x} {a}) (fp.lt {zero} {y}) (fp.leq {y} {c})) (fp.geq {d} {q}))"
                    ));
                    facts.push(format!(
                        "(=> (and (fp.leq {neg_a} {x}) (fp.leq {x} {zero}) (fp.geq {y} {c})) (fp.geq {d} {neg_q}))"
                    ));
                    facts.push(format!(
                        "(=> (and (fp.leq {x} {neg_a}) (fp.lt {zero} {y}) (fp.leq {y} {c})) (fp.leq {d} {neg_q}))"
                    ));
                }
            }
        }
        RoundedOp::Add(x, y) => {
            facts.push(format!(
                "(= (fp.isNaN {d}) (or (fp.isNaN {x}) (fp.isNaN {y}) (and (fp.isInfinite {x}) (fp.isInfinite {y}) (distinct (fp.isNegative {x}) (fp.isNegative {y})))))"
            ));
            facts.push(format!(
                "(=> (and (fp.isZero {x}) (not (fp.isNaN {y}))) (fp.eq {d} {y}))"
            ));
            facts.push(format!(
                "(=> (and (fp.isZero {y}) (not (fp.isNaN {x}))) (fp.eq {d} {x}))"
            ));
            // Addition is monotone in each operand.
            for (a, c) in bound_pairs(literals) {
                let (la, lc) = (lit(a), lit(c));
                let sum = lit(round_in(float, a, c, |a, c| a + c, |a, c| a + c));
                facts.push(format!(
                    "(=> (and (fp.leq {x} {la}) (fp.leq {y} {lc})) (fp.leq {d} {sum}))"
                ));
                facts.push(format!(
                    "(=> (and (fp.geq {x} {la}) (fp.geq {y} {lc})) (fp.geq {d} {sum}))"
                ));
            }
        }
        RoundedOp::Sub(x, y) => {
            facts.push(format!(
                "(= (fp.isNaN {d}) (or (fp.isNaN {x}) (fp.isNaN {y}) (and (fp.isInfinite {x}) (fp.isInfinite {y}) (= (fp.isNegative {x}) (fp.isNegative {y})))))"
            ));
            // `x - x` of a finite `x` is `+0`.
            facts.push(format!(
                "(=> (and (= {x} {y}) (not (fp.isInfinite {x})) (not (fp.isNaN {x}))) (and (fp.isZero {d}) (fp.isPositive {d})))"
            ));
            facts.push(format!(
                "(=> (and (fp.isZero {y}) (not (fp.isNaN {x}))) (fp.eq {d} {x}))"
            ));
            // Subtraction rises with `x` and falls with `y`.
            for (a, c) in bound_pairs(literals) {
                let (la, lc) = (lit(a), lit(c));
                let difference = lit(round_in(float, a, c, |a, c| a - c, |a, c| a - c));
                facts.push(format!(
                    "(=> (and (fp.leq {x} {la}) (fp.geq {y} {lc})) (fp.leq {d} {difference}))"
                ));
                facts.push(format!(
                    "(=> (and (fp.geq {x} {la}) (fp.leq {y} {lc})) (fp.geq {d} {difference}))"
                ));
            }
        }
        RoundedOp::Mul(x, y) => {
            facts.push(format!(
                "(= (fp.isNaN {d}) (or (fp.isNaN {x}) (fp.isNaN {y}) (and (fp.isZero {x}) (fp.isInfinite {y})) (and (fp.isInfinite {x}) (fp.isZero {y}))))"
            ));
            facts.push(format!(
                "(=> (not (fp.isNaN {d})) (= (fp.isNegative {d}) (xor (fp.isNegative {x}) (fp.isNegative {y}))))"
            ));
            facts.push(format!("(=> (fp.eq {y} {one}) (= {d} {x}))"));
            facts.push(format!("(=> (fp.eq {x} {one}) (= {d} {y}))"));
            // Multiplication of non-negative values is monotone in each.
            for (a, c) in bound_pairs(literals) {
                let (la, lc) = (lit(a), lit(c));
                let product = lit(round_in(float, a, c, |a, c| a * c, |a, c| a * c));
                facts.push(format!(
                    "(=> (and (fp.leq {zero} {x}) (fp.leq {x} {la}) (fp.leq {zero} {y}) (fp.leq {y} {lc})) (fp.leq {d} {product}))"
                ));
                if a > 0.0 && c > 0.0 {
                    facts.push(format!(
                        "(=> (and (fp.geq {x} {la}) (fp.geq {y} {lc})) (fp.geq {d} {product}))"
                    ));
                }
            }
        }
        RoundedOp::Sqrt(x) => {
            facts.push(format!(
                "(= (fp.isNaN {d}) (or (fp.isNaN {x}) (fp.lt {x} {zero})))"
            ));
            facts.push(format!(
                "(=> (not (fp.isNaN {d})) (= (fp.isNegative {d}) (fp.isNegative {x})))"
            ));
            facts.push(format!("(=> (fp.isZero {x}) (= {d} {x}))"));
            facts.push(format!(
                "(=> (and (fp.isInfinite {x}) (fp.isPositive {x})) (= {d} {x}))"
            ));
            facts.push(format!(
                "(=> (fp.geq {x} {one}) (and (fp.leq {one} {d}) (fp.leq {d} {x})))"
            ));
            facts.push(format!(
                "(=> (and (fp.leq {zero} {x}) (fp.leq {x} {one})) (and (fp.leq {x} {d}) (fp.leq {d} {one})))"
            ));
            // The square root is monotone, and rounding keeps the order.
            let root = rounded(float, f64::sqrt, f32::sqrt);
            for &c in literals {
                let r = lit(root(c));
                let c = lit(c);
                facts.push(format!(
                    "(=> (and (fp.leq {zero} {x}) (fp.leq {x} {c})) (fp.leq {d} {r}))"
                ));
                facts.push(format!("(=> (fp.geq {x} {c}) (fp.geq {d} {r}))"));
            }
        }
    }
    facts
}

/// The finite values `literals` have in `float`, each once.
///
/// The bounds are computed and stated in `float`, and a float64 literal can
/// round to zero or to infinity as a float32. Taken as it was written, it
/// would pass for a positive, finite bound and give a bound of `0 / 0`,
/// `inf - inf`, or `0 * inf`: a NaN, which no value compares with, so the
/// fact would rule out values the program can have.
fn values_in(float: FloatTy, literals: &[f64]) -> Vec<f64> {
    let mut values: Vec<f64> = Vec::new();
    for &value in literals {
        let value = match float {
            FloatTy::F64 => value,
            FloatTy::F32 => f64::from(value as f32),
        };
        if value.is_finite() && !values.contains(&value) {
            values.push(value);
        }
    }
    values
}

/// Each ordered pair of `literals`.
fn bound_pairs(literals: &[f64]) -> impl Iterator<Item = (f64, f64)> + '_ {
    literals
        .iter()
        .flat_map(move |&a| literals.iter().map(move |&c| (a, c)))
}

/// `op(a, c)` computed in `float`, so it rounds as the program's does.
fn round_in(
    float: FloatTy,
    a: f64,
    c: f64,
    op64: impl Fn(f64, f64) -> f64,
    op32: impl Fn(f32, f32) -> f32,
) -> f64 {
    match float {
        FloatTy::F64 => op64(a, c),
        FloatTy::F32 => op32(a as f32, c as f32) as f64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_are_read_back_from_their_smt_text() {
        let text = format!(
            "(and (fp.leq {} x) (fp.leq x {}) (fp.leq {} x))",
            FloatTy::literal(0.0),
            FloatTy::literal(100.0),
            FloatTy::literal(-2.5)
        );
        assert_eq!(literals([text.as_str(), "(fp #b0 #b01)"]), vec![0.0, 100.0]);
    }

    #[test]
    fn float32_bounds_round_as_float32() {
        let rounded_op = Rounded {
            fact: 0,
            result: "d".into(),
            op: RoundedOp::Sqrt("x".into()),
            float: FloatTy::F32,
        };
        let facts = axioms(&rounded_op, &[2.0]);
        let root = literal(FloatTy::F32, (2.0f32).sqrt() as f64);
        assert!(facts.iter().any(|fact| fact.contains(&root)), "{facts:?}");
    }

    #[test]
    fn float32_sums_differences_and_products_round_as_float32() {
        let x = "x".to_string();
        for (op, value) in [
            (RoundedOp::Add(x.clone(), x.clone()), 0.1f32 + 0.1f32),
            (RoundedOp::Sub(x.clone(), x.clone()), 0.1f32 - 0.1f32),
            (RoundedOp::Mul(x.clone(), x.clone()), 0.1f32 * 0.1f32),
        ] {
            let rounded_op = Rounded {
                fact: 0,
                result: "d".into(),
                op,
                float: FloatTy::F32,
            };
            let facts = axioms(&rounded_op, &[0.1]);
            let bound = literal(FloatTy::F32, value as f64);
            assert!(facts.iter().any(|fact| fact.contains(&bound)), "{facts:?}");
        }
    }

    #[test]
    fn float32_bounds_leave_out_literals_that_round_to_zero_or_infinity() {
        // As a float32, 1e-300 is zero and 1e300 is infinite. A bound
        // computed from those (0 / 0, inf - inf, 0 * inf) is NaN, and a fact
        // that compares with NaN is false of values the program can have.
        let (x, y) = ("x".to_string(), "y".to_string());
        for op in [
            RoundedOp::Div(x.clone(), y.clone()),
            RoundedOp::Add(x.clone(), y.clone()),
            RoundedOp::Sub(x.clone(), y.clone()),
            RoundedOp::Mul(x.clone(), y.clone()),
            RoundedOp::Sqrt(x.clone()),
        ] {
            let mut rounded_op = Rounded {
                fact: 0,
                result: "d".into(),
                op,
                float: FloatTy::F32,
            };
            let facts = axioms(&rounded_op, &[0.0, 1e-300, 5.0, 1e300]);
            assert_eq!(facts, axioms(&rounded_op, &[0.0, 5.0]));
            for value in [f64::NAN, f64::INFINITY] {
                let bound = literal(FloatTy::F32, value);
                assert!(!facts.iter().any(|fact| fact.contains(&bound)), "{facts:?}");
            }
            // Each of them is a float64 value of its own.
            rounded_op.float = FloatTy::F64;
            let facts = axioms(&rounded_op, &[0.0, 1e-300, 5.0, 1e300]);
            for value in [1e-300, 1e300] {
                let bound = literal(FloatTy::F64, value);
                assert!(facts.iter().any(|fact| fact.contains(&bound)), "{facts:?}");
            }
            let nan = literal(FloatTy::F64, f64::NAN);
            assert!(!facts.iter().any(|fact| fact.contains(&nan)), "{facts:?}");
        }
    }
}
