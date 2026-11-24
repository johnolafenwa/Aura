//! Interpret fixed-array operations using the same element codecs as C
//! array fields. The native array has neither a handle nor a length word.
use super::*;
use crate::mir::fixed_arrays::{LITERAL, READ, REPEAT, WRITE};

impl MirRuntime {
    pub(super) fn evaluate_fixed(
        &mut self,
        op: &str,
        args: &[MirArg],
        ty: Option<&Type>,
        env: &mut Env,
    ) -> Result<Value> {
        let (element, count) = ty
            .and_then(Type::as_fixed_array)
            .ok_or_else(|| Diagnostic::new("missing fixed array type in MIR"))?;
        let op = crate::mir::fixed_arrays::checked_form(op);
        match op {
            LITERAL if args.len() == count => {
                let values = args
                    .iter()
                    .map(|arg| self.evaluate_owned_operand(&arg.value, env))
                    .collect::<Result<Vec<_>>>()?;
                Ok(Value::Vec(VecValue::new(element.clone(), values.into())))
            }
            REPEAT if args.len() == 1 => {
                let value = self.evaluate_owned_operand(&args[0].value, env)?;
                Ok(Value::Vec(VecValue::new(
                    element.clone(),
                    vec![value; count].into(),
                )))
            }
            crate::mir::fixed_arrays::ADDRESS if args.len() == 5 => {
                let (Operand::Int(line), Operand::Int(column), Operand::Int(stride)) =
                    (&args[2].value, &args[3].value, &args[4].value)
                    else { return Err(Diagnostic::new("missing array stride or position")); };
                let (Value::Int(address), Value::Int(index)) = (self.evaluate_operand(&args[0].value,env)?, self.evaluate_operand(&args[1].value,env)?)
                    else { return Err(Diagnostic::new("invalid array element address")); };
                let i = index.as_i128().and_then(|i| usize::try_from(i).ok()).filter(|i| *i < count)
                    .ok_or_else(|| Diagnostic::coded_at("AU4031", crate::diag::Span::new(*line as usize,*column as usize),
                        format!("fixed array index `{index}` is out of bounds for length `{count}`")))?;
                let base = address.as_i128().and_then(|n| u64::try_from(n).ok())
                    .ok_or_else(||Diagnostic::coded("AU4032","invalid base address for a fixed array"))?;
                Ok(Value::Int(IntegerValue::from_u64(base.wrapping_add((i as u64).wrapping_mul(*stride as u64)))))
            }
            READ | WRITE if args.len() == if op == READ { 4 } else { 5 } => {
                let Value::Vec(mut array) = self.evaluate_operand(&args[0].value, env)? else {
                    return Err(Diagnostic::new("MIR array access needs an inline array"));
                };
                if array.elements.len() != count {
                    return Err(Diagnostic::new("MIR fixed array has the wrong length"));
                }
                let Value::Int(raw) = self.evaluate_operand(&args[1].value, env)? else {
                    return Err(Diagnostic::new("MIR array index must be an integer"));
                };
                let index = raw
                    .as_i128()
                    .and_then(|i| usize::try_from(i).ok())
                    .filter(|i| *i < count);
                let Some(index) = index else {
                    let (Operand::Int(line), Operand::Int(column)) =
                        (&args[2].value, &args[3].value)
                    else {
                        return Err(Diagnostic::new("MIR array access needs a source position"));
                    };
                    return Err(Diagnostic::coded_at(
                        "AU4031",
                        crate::diag::Span::new(*line as usize, *column as usize),
                        format!("fixed array index `{raw}` is out of bounds for length `{count}`"),
                    ));
                };
                if op == READ {
                    return Ok(array.elements.get(index).unwrap().into_owned());
                }
                let value = self.evaluate_owned_operand(&args[4].value, env)?;
                array.elements.replace(index, value);
                Ok(Value::Vec(array))
            }
            _ => Err(Diagnostic::new(format!("invalid arguments to {op}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn fixed_arrays_run_with_lengths_and_source_positions() {
        let source = include_str!("../tests/fixtures/run-pass/fixed_arrays.au");
        assert_eq!(crate::run_source(source).unwrap().stdout, "16\n4\n7\n");
        let source = "def main() -> None:\n    a: fixed[int32, 2] = [0, 1]\n    print(a[-1])\n";
        let error = crate::run_source(source).unwrap_err();
        assert_eq!(error.code, "AU4031");
        assert_eq!(error.span.unwrap().line, 3);
    }
}

#[cfg(test)]
#[test]
fn nested_and_wide_fixed_arrays_preserve_each_lane() {
    for (source, expected) in [
        (
            include_str!("../tests/fixtures/run-pass/nested_fixed_arrays.au"),
            include_str!("../tests/fixtures/run-pass/nested_fixed_arrays.stdout"),
        ),
        (
            include_str!("../tests/fixtures/run-pass/wide_fixed_arrays.au"),
            include_str!("../tests/fixtures/run-pass/wide_fixed_arrays.stdout"),
        ),
        (
            include_str!("../../../examples/collections/fixed_scores.au"),
            "3\n0\n85\n0\n",
        ),
    ] {
        assert_eq!(crate::run_source(source).unwrap().stdout, expected);
    }
}
