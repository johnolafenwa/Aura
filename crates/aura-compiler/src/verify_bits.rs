//! Integer methods operate on a bit vector of the receiver's declared width.
//! Ordinary arithmetic in the verifier continues to use mathematical integers.
use std::collections::BTreeMap;

use super::exec::{IntTy, Val};
use aura_core::integer_bits::BitMethod;

/// Keep the bit-vector representation across nested integer method calls.
/// Z3 can simplify the extraction and concatenation directly in this sort.
#[derive(Default)]
pub(super) struct BitTerms(BTreeMap<(u32, String), String>);

impl BitTerms {
    pub(super) fn evaluate(
        &mut self,
        method: BitMethod,
        value: &str,
        ty: IntTy,
        count: Option<&str>,
    ) -> Val {
        let width = ty.0.bit_width();
        let extract = |high: u32, low: u32| format!("((_ extract {high} {low}) word)");
        let set = |bit| format!("(= {} #b1)", extract(bit, bit));
        let term = match method {
            BitMethod::RotateLeft | BitMethod::RotateRight => {
                let op = if method == BitMethod::RotateLeft {
                    "ext_rotate_left"
                } else {
                    "ext_rotate_right"
                };
                format!("({op} word distance)")
            }
            BitMethod::CountOnes => {
                let terms: Vec<_> = (0..width)
                    .map(|bit| format!("(ite {} 1 0)", set(bit)))
                    .collect();
                format!("(+ {})", terms.join(" "))
            }
            BitMethod::LeadingZeros | BitMethod::TrailingZeros => {
                (0..width).fold(width.to_string(), |rest, position| {
                    let bit = if method == BitMethod::LeadingZeros {
                        position
                    } else {
                        width - 1 - position
                    };
                    format!("(ite {} {} {rest})", set(bit), width - 1 - position)
                })
            }
            BitMethod::SwapBytes if width == 8 => "word".to_string(),
            BitMethod::SwapBytes => {
                let bytes: Vec<_> = (0..width)
                    .step_by(8)
                    .map(|low| extract(low + 7, low))
                    .collect();
                format!("(concat {})", bytes.join(" "))
            }
        };
        let source = self
            .0
            .get(&(width, value.to_string()))
            .cloned()
            .unwrap_or_else(|| format!("((_ int_to_bv {width}) {value})"));
        let distance = count
            .map(|count| format!(" (distance ((_ int_to_bv {width}) {count}))"))
            .unwrap_or_default();
        let bits = format!("(let ((word {source}){distance}) {term})");
        if method.counts() {
            return Val::Int(bits, IntTy::INT64);
        }
        let conv = if ty.0.is_signed() {
            "sbv_to_int"
        } else {
            "ubv_to_int"
        };
        let integer = format!("({conv} {bits})");
        self.0.insert((width, integer.clone()), bits);
        Val::Int(integer, ty)
    }
}
