use super::*;
use aura_core::integer_bits::BitMethod;

pub(super) fn evaluate(
    method: BitMethod,
    value: IntegerValue,
    static_type: Option<&Type>,
    args: &[MirArg],
    env: &mut Env,
) -> Result<Value> {
    let kind = static_type
        .and_then(|ty| match ty {
            Type::Named(name, _) => IntegerKind::from_runtime_type_name(name),
            _ => None,
        })
        .or(value.runtime_kind())
        .unwrap_or(IntegerKind::Int64);
    let count = match evaluate_named_args(args, env)?.as_slice() {
        [] => 0,
        [count] => expect_i64_value(&count.value, "rotation count")?,
        _ => return Err(Diagnostic::coded("AU4034", "invalid bit method arguments")),
    };
    method
        .apply(value, kind, count)
        .map(Value::Int)
        .map_err(|message| Diagnostic::coded("AU4034", message))
}
