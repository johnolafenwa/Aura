//! Integer methods operate on a bit vector of the receiver's declared width.
//! Ordinary arithmetic in the verifier continues to use mathematical integers.
use super::exec::{IntTy, Val};
use aura_core::integer_bits::BitMethod;

pub(super) fn evaluate(method: BitMethod, value: &str, ty: IntTy, count: Option<&str>) -> Val {
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
            let count = count.expect("a checked rotation supplies its count");
            format!("({op} word ((_ int_to_bv {width}) {count}))")
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
    let (term, result_ty) = if method.counts() {
        (term, IntTy::INT64)
    } else {
        let conv = if ty.0.is_signed() {
            "sbv_to_int"
        } else {
            "ubv_to_int"
        };
        (format!("({conv} {term})"), ty)
    };
    Val::Int(
        format!("(let ((word ((_ int_to_bv {width}) {value}))) {term})"),
        result_ty,
    )
}
