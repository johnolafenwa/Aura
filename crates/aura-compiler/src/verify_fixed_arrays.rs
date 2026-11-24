//! Symbolic inline arrays have fixed lengths and reject negative indexes.
use super::*;
use crate::mir::fixed_arrays::{LITERAL, READ, REPEAT, WRITE};

impl Exec<'_> {
    pub(super) fn fixed_call(
        &mut self,
        state: &mut State,
        op: &str,
        args: &[MirArg],
        ty: Option<&Type>,
    ) -> Val {
        let Some((elem, n)) = ty.and_then(Type::as_fixed_array) else {
            return Val::Opaque;
        };
        let len = n.to_string();
        let hint = IntTy::of(elem);
        match op {
            LITERAL => {
                let mut data = self.fresh_store(elem, "fixed");
                for (i, arg) in args.iter().enumerate() {
                    let value = self.operand(state, &arg.value, hint);
                    data = data.and_then(|data| {
                        self.put_or_fresh(&data, elem, &i.to_string(), &value, "fixed")
                    });
                }
                Val::List {
                    len,
                    data,
                    elem: elem.clone(),
                }
            }
            REPEAT if args.len() == 1 => {
                let item = self.operand(state, &args[0].value, hint);
                let data = element_sort(elem)
                    .zip(scalar_term(&item))
                    .map(|(sort, term)| {
                        Store::Leaf(format!("((as const (Array Int {sort})) {term})"), sort)
                    });
                Val::List {
                    len,
                    data,
                    elem: elem.clone(),
                }
            }
            READ | WRITE if args.len() >= 4 => {
                let array = self.operand(state, &args[0].value, None);
                let index = self.operand(state, &args[1].value, Some(IntTy::INT64));
                let (Operand::Int(line), Operand::Int(column)) = (&args[2].value, &args[3].value)
                else {
                    return Val::Opaque;
                };
                let span = Span::new(*line as usize, *column as usize);
                let Val::Int(index, _) = index else {
                    return Val::Opaque;
                };
                self.oblige(
                    ObligationKind::IndexInRange,
                    span,
                    "fixed array index in range",
                    &state.pc,
                    Goal::Holds(format!("(and (<= 0 {index}) (< {index} {len}))")),
                );
                if op == READ {
                    return self.element(&array, &index, "element");
                }
                let Val::List { data, .. } = array else {
                    return Val::Opaque;
                };
                let Some(value) = args.get(4) else {
                    return Val::Opaque;
                };
                let value = self.operand(state, &value.value, hint);
                let data =
                    data.and_then(|store| self.put_or_fresh(&store, elem, &index, &value, "fixed"));
                Val::List {
                    len,
                    data,
                    elem: elem.clone(),
                }
            }
            _ => Val::Opaque,
        }
    }
}
