//! A cheaper first query for checks that read a float `/` or `sqrt`.
//!
//! The solver handles an IEEE-754 division or square root by building the
//! circuit that computes it, which can take it longer than the time limit
//! even when the check follows from simple facts, such as `x / y <= x` for
//! `y >= 1.0`. So a check whose function divides floats or takes a square
//! root is first asked of a weaker problem: each such result is a value of
//! its own, with only facts that the rounded operation is known to have,
//! such as its sign, when it is NaN, how its size compares with its
//! operands and with 1, and its bounds where its operands are bounded by
//! literals of the function. Every fact holds of the exact operation, so a
//! check proved this way is proved. A check the weaker problem does not
//! prove is asked again of the exact one, which also finds the
//! counterexample of a check that fails.

use super::exec::FloatTy;
use std::collections::BTreeSet;

/// A float `/` or `sqrt` result, defined by the fact at `fact`.
#[derive(Clone, Debug)]
pub(super) struct Rounded {
    pub(super) fact: usize,
    pub(super) result: String,
    pub(super) op: RoundedOp,
    pub(super) float: FloatTy,
}

#[derive(Clone, Debug)]
pub(super) enum RoundedOp {
    Div(String, String),
    Sqrt(String),
}

/// The most literals each result is related to, so the weaker problem
/// stays small.
const MAX_LITERALS: usize = 16;

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
}
