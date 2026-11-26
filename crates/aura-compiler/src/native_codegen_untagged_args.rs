//! Enum arguments passed without their runtime type tag.
//!
//! A call coerces each boxed argument to its parameter's type, which tags
//! a box that does not carry its type's tag yet: a load of the tag cell,
//! a compare with the box's shape word, and a runtime call when they
//! differ. A callee that needs no record of its caller (see
//! `frame_free_callees`) and only matches on an enum parameter and reads
//! its scalar or `str` payloads never looks at the tag: the variant tests
//! and payload reads read the variant's layout, or its value in the
//! runtime, and the parameter goes nowhere else. Its callers pass such an
//! argument as it is.

use std::collections::HashSet;

use super::*;
use crate::mir::{instruction_place_mentions, terminator_place_mentions};

/// The enum parameters of `free`'s functions, by function and position,
/// that the function only matches on and reads payloads of.
pub(super) fn match_only_params(
    module: &MirModule,
    free: &HashSet<String>,
) -> HashSet<(String, usize)> {
    let mut params = HashSet::new();
    for function in module
        .functions
        .iter()
        .filter(|function| free.contains(&function.name))
    {
        for (index, param) in function.params.iter().enumerate() {
            let is_enum = matches!(&param.ty, Type::Named(name, args) if args.is_empty()
                && module.enums.iter().any(|candidate| &candidate.name == name));
            if is_enum
                && param.passing == MirReceiverKind::Borrow
                && only_matched(function, &param.name)
            {
                params.insert((function.name.clone(), index));
            }
        }
    }
    params
}

/// Whether every mention of `param` in `function` is the scrutinee of a
/// match or of a payload read.
fn only_matched(function: &MirFunction, param: &str) -> bool {
    let is_param = |operand: &Operand| matches!(operand, Operand::Place(place) if place == param);
    function.blocks.iter().all(|block| {
        let terminator_reads = match &block.terminator {
            Terminator::Match { scrutinee, .. } if is_param(scrutinee) => true,
            terminator => !terminator_place_mentions(terminator)
                .into_iter()
                .any(|place| place == param),
        };
        terminator_reads
            && block
                .instructions
                .iter()
                .all(|instruction| match instruction {
                    Instruction::Assign {
                        value: Rvalue::VariantPayload { scrutinee, .. },
                        ..
                    } if is_param(scrutinee) => true,
                    instruction => {
                        let mut mentions = Vec::new();
                        instruction_place_mentions(instruction, &mut mentions);
                        let target = match instruction {
                            Instruction::Assign { target, .. } => Some(target.as_str()),
                            _ => None,
                        };
                        target != Some(param) && !mentions.into_iter().any(|place| place == param)
                    }
                })
    })
}
