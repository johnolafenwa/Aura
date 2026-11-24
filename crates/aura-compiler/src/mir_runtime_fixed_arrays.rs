//! Interpret fixed-array operations using the same element codecs as C
//! array fields. The native array has neither a handle nor a length word.
use super::*;
use crate::mir::fixed_arrays::{LITERAL, REPEAT, READ, WRITE};

impl MirRuntime {
    pub(super) fn evaluate_fixed(&mut self, op: &str, args: &[MirArg], ty: Option<&Type>, env: &mut Env) -> Result<Value> {
        let (element, count) = ty.and_then(Type::as_fixed_array)
            .ok_or_else(|| Diagnostic::new("missing fixed array type in MIR"))?;
        match op {
            LITERAL if args.len() == count => {
                let values = args.iter().map(|arg| self.evaluate_owned_operand(&arg.value, env)).collect::<Result<Vec<_>>>()?;
                Ok(Value::Vec(VecValue::new(element.clone(), values.into())))
            }
            REPEAT if args.len() == 1 => {
                let value = self.evaluate_owned_operand(&args[0].value, env)?;
                Ok(Value::Vec(VecValue::new(element.clone(), vec![value; count].into())))
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
                let index = raw.as_i128().and_then(|i| usize::try_from(i).ok()).filter(|i| *i < count);
                let Some(index) = index else {
                    let (Operand::Int(line), Operand::Int(column)) = (&args[2].value, &args[3].value) else {
                        return Err(Diagnostic::new("MIR array access needs a source position"));
                    };
                    return Err(Diagnostic::coded_at("AU4031", crate::diag::Span::new(*line as usize, *column as usize), format!("fixed array index `{raw}` is out of bounds for length `{count}`")));
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
