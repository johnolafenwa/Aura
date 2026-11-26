//! An Aura caller of a naked C export must enter the single assembly
//! definition through the same C ABI as an external caller.
use super::*;
use crate::ast::FunctionDecl;

pub(super) fn lower_export(
    lowerer: &mut Lowerer<'_>,
    function: &FunctionDecl,
    params: &[MirParam],
    return_type: &Type,
) -> bool {
    if function.export.is_none() || VERIFICATION.with(std::cell::Cell::get) {
        return false;
    }
    let Some(attrs) = function.machine.as_ref().filter(|a| a.naked.is_some()) else {
        return false;
    };
    let args = params
        .iter()
        .map(|p| MirArg {
            name: Some(p.name.clone()),
            value: Operand::Place(p.name.clone()),
            writeback_place: (p.passing == MirReceiverKind::BorrowMut).then(|| p.name.clone()),
        })
        .collect();
    let call = MirExternCall {
        module_name: lowerer.module_name.to_string(),
        name: function.name.clone(),
        symbol: attrs.link_name.as_ref().unwrap_or(&function.name).clone(),
        abi: "C".into(),
        params: params
            .iter()
            .map(|p| MirExternParam {
                name: p.name.clone(),
                passing: p.passing,
                ty: p.ty.clone(),
                c_string: false,
                pointer: false,
                c_struct: None,
            })
            .collect(),
        return_type: return_type.clone(),
        c_string_result: false,
        c_struct_result: None,
        handle_drops: BTreeMap::new(),
    };
    let result = lowerer.new_typed_temp(return_type.clone());
    lowerer.emit(Instruction::Assign {
        target: result.clone(),
        value: Rvalue::Call {
            callee: CallTarget::Extern(call),
            args,
            type_args: Default::default(),
        },
    });
    lowerer.terminate(Terminator::Return(Operand::Place(result)));
    true
}
