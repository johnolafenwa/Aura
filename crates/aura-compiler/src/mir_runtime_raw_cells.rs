//! A local whose field has an exposed address keeps one aligned C
//! representation until the containing activation returns.
use super::*;
use crate::c_layout::CStruct;

fn pack(value: &Value, layout: &CStruct, bytes: &mut [u8]) -> Result<()> {
    if layout.name == "#fixed" {
        return aura_runtime::c_values::pack_kind(value, &layout.fields[0].kind, "fixed", bytes)
            .map_err(|error| c_value_diagnostic("fixed", error));
    }
    write_struct_bytes(value, layout, bytes)
}

fn unpack(layout: &CStruct, bytes: &[u8], symbol: &str) -> Result<Value> {
    if layout.name == "#fixed" {
        return aura_runtime::c_values::unpack_kind(&layout.fields[0].kind, bytes)
            .map_err(|error| c_value_diagnostic(symbol, error));
    }
    struct_runtime_value(layout, bytes, symbol)
}

pub(super) type SharedCells = HashMap<String, Arc<Mutex<CellBytes>>>;

pub(super) type ParameterLayouts = HashMap<String, HashMap<String, CStruct>>;

pub(super) fn parameter_layouts(module: &MirModule) -> ParameterLayouts {
    let mut layouts = ParameterLayouts::new();
    for function in &module.functions {
        for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
            if let Instruction::Assign {
                value: Rvalue::CFieldAddress { root, layout, .. },
                ..
            } = instruction
            {
                if function
                    .params
                    .iter()
                    .any(|param| param.name == *root && param.passing != MirReceiverKind::Value)
                {
                    layouts
                        .entry(function.name.clone())
                        .or_default()
                        .insert(root.clone(), layout.clone());
                }
            }
        }
    }
    layouts
}

pub(super) struct CellBytes {
    layout: CStruct,
    allocation: Vec<u8>,
    start: usize,
}

impl CellBytes {
    fn new(layout: &CStruct, value: &Value) -> Result<Self> {
        let mut allocation = vec![0; layout.size() + layout.align() - 1];
        let start = allocation.as_ptr().align_offset(layout.align());
        pack(value, layout, &mut allocation[start..start + layout.size()])?;
        Ok(Self {
            layout: layout.clone(),
            allocation,
            start,
        })
    }

    fn read(&self) -> Result<Value> {
        unpack(
            &self.layout,
            &self.allocation[self.start..self.start + self.layout.size()],
            "mem.address_of",
        )
    }
}

impl Env {
    pub(super) fn raw_field_address(
        &mut self,
        root: &str,
        layout: &CStruct,
        offset: usize,
    ) -> Result<u64> {
        if !self.raw_cells.contains_key(root) {
            let value = self.read_place(root)?;
            self.raw_cells.insert(
                root.to_string(),
                Arc::new(Mutex::new(CellBytes::new(layout, &value)?)),
            );
        }
        let mut cell = self.raw_cells[root].lock().expect("C local storage");
        Ok(cell
            .allocation
            .as_mut_ptr()
            .wrapping_add(cell.start + offset) as usize as u64)
    }

    pub(super) fn borrowed_cells(
        &mut self,
        function: &MirFunction,
        args: &[MirArg],
        params: Option<&HashMap<String, CStruct>>,
    ) -> Result<SharedCells> {
        let mut cells = SharedCells::new();
        for (i, arg) in args.iter().enumerate() {
            let Some(param) = arg
                .name
                .as_ref()
                .and_then(|name| function.params.iter().find(|p| &p.name == name))
                .or_else(|| function.params.get(i))
            else {
                continue;
            };
            if param.passing == MirReceiverKind::Value {
                continue;
            }
            let place = match (&arg.writeback_place, &arg.value) {
                (Some(place), _) | (_, Operand::Place(place)) => self.resolve_loan_place(place)?,
                _ => continue,
            };
            if !self.raw_cells.contains_key(&place) {
                if let Some(layout) = params.and_then(|p| p.get(&param.name)) {
                    self.raw_field_address(&place, layout, 0)?;
                }
            }
            if let Some(cell) = self.raw_cells.get(&place) {
                cells.insert(param.name.clone(), Arc::clone(cell));
            }
        }
        Ok(cells)
    }

    pub(super) fn refresh_raw_cells(&mut self) -> Result<()> {
        for (name, cell) in &self.raw_cells {
            let fields = split_place_segments(name)?;
            if let Some(root) = self.values.get_mut(&fields[0]) {
                let held = cell.lock().expect("C local storage").read()?;
                if fields.len() == 1 {
                    *root = held;
                } else {
                    let path = fields[1..].iter().map(String::as_str).collect::<Vec<_>>();
                    write_nested_place(root, &path, held, name)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn update_addressed_members(&self, root: &str) -> Result<()> {
        for (place, cell) in &self.raw_cells {
            if place.split_once('.').is_some_and(|(base, _)| base == root) {
                let value = self.place_ref(place)?;
                let mut guard = cell.lock().expect("C local storage");
                let cell = &mut *guard;
                pack(
                    &value,
                    &cell.layout,
                    &mut cell.allocation[cell.start..cell.start + cell.layout.size()],
                )?;
            }
        }
        Ok(())
    }

    pub(super) fn write_raw_cell(
        &mut self,
        root: &str,
        rest: &[String],
        place: &str,
        value: &Value,
    ) -> Result<()> {
        let mut guard = self.raw_cells[root].lock().expect("C local storage");
        let cell = &mut *guard;
        let whole = if rest.is_empty() {
            try_clone_mir_value(value)?
        } else {
            let mut whole = cell.read()?;
            let path = rest.iter().map(String::as_str).collect::<Vec<_>>();
            write_nested_place(&mut whole, &path, try_clone_mir_value(value)?, place)?;
            whole
        };
        pack(
            &whole,
            &cell.layout,
            &mut cell.allocation[cell.start..cell.start + cell.layout.size()],
        )
    }
}

pub(super) fn access(
    word: &Value,
    store: Option<&Value>,
    layout: &CStruct,
    volatile: bool,
) -> Result<Value> {
    let address = match word {
        Value::Int(number) => number.as_i128().and_then(|n| usize::try_from(n).ok()),
        _ => None,
    }
    .ok_or_else(|| Diagnostic::coded("AU4032", "raw storage needs a pointer-sized address"))?;
    if matches!(layout.name.as_str(), "#fixed.int128" | "#fixed.uint128") {
        // SAFETY: the caller of a typed pointer operation supplies the required
        // range, access rights, and (for volatility) alignment.
        return unsafe {
            match store {
                Some(value)
                    if aura_runtime::raw_wide_memory::write(
                        address as *mut u128,
                        value,
                        volatile,
                    ) =>
                {
                    Ok(Value::Unit)
                }
                Some(_) => Err(Diagnostic::coded(
                    "AU4032",
                    "wide memory write needs an integer",
                )),
                None => Ok(aura_runtime::raw_wide_memory::read(
                    address as *const u128,
                    layout.name == "#fixed.int128",
                    volatile,
                )),
            }
        };
    }
    let ptr = address as *mut u8;
    let mut bytes = vec![0; layout.size()];
    // SAFETY: the FFI grant places the address range and lifetime under the
    // caller's control. A byte access does not require aggregate alignment.
    if let Some(value) = store {
        pack(value, layout, &mut bytes)?;
        unsafe {
            if volatile {
                for (index, byte) in bytes.into_iter().enumerate() {
                    ptr.add(index).write_volatile(byte);
                }
            } else {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
            }
        }
        Ok(Value::Unit)
    } else {
        unsafe {
            if volatile {
                for (index, byte) in bytes.iter_mut().enumerate() {
                    *byte = ptr.add(index).read_volatile();
                }
            } else {
                std::ptr::copy_nonoverlapping(ptr, bytes.as_mut_ptr(), bytes.len());
            }
        }
        unpack(layout, &bytes, "pointer.read")
    }
}

pub(super) fn permission_error() -> Diagnostic {
    Diagnostic::coded(
        "AU4032",
        "raw memory operations require a manifest-checked path with allow_ffi = true",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lower(source: &str) -> MirModule {
        let parsed = crate::parse_source(source).expect("pointer fixture should parse");
        let program = crate::check_module_with_builtin_imports(parsed)
            .expect("pointer fixture should type check");
        crate::mir::lower(&program)
    }

    #[test]
    fn typed_pointer_memory_needs_manifest_checked_mir() {
        let mut cell = 1_i32;
        let addr = &mut cell as *mut i32 as usize;
        for operation in ["p.read()", "p.write(2)"] {
            let source =
                format!("def main() -> None:\n    p = {addr} as ptr[int32]\n    {operation}\n");
            assert_eq!(
                crate::mir_runtime::run(&lower(&source)).unwrap_err().code,
                "AU4032"
            );
        }
        let source = format!("extern \"C\" class Item:\n    value: int32\n\ndef main() -> None:\n    p = {addr} as ptr[Item]\n    p.read()\n");
        assert_eq!(
            crate::mir_runtime::run(&lower(&source)).unwrap_err().code,
            "AU4032"
        );
        assert_eq!(cell, 1);
    }

    #[test]
    fn typed_pointer_interpreter_runs_scalar_and_aggregate_operations() {
        for source in [
            include_str!("../tests/fixtures/typed-pointers/scalars.au"),
            include_str!("../tests/fixtures/typed-pointers/aggregate.au"),
            include_str!("../tests/fixtures/typed-pointers/foreign.au"),
            include_str!("../tests/fixtures/typed-pointers/foreign_array.au"),
            include_str!("../tests/fixtures/typed-pointers/links.au"),
            include_str!("../tests/fixtures/typed-pointers/widths.au"),
            include_str!("../tests/fixtures/typed-pointers/nested.au"),
            include_str!("../tests/fixtures/typed-pointers/wide.au"),
            include_str!("../tests/fixtures/typed-pointers/fixed.au"),
        ] {
            let output = crate::mir_runtime::run_trusted(&lower(source))
                .expect("a valid allocation should preserve stored values");
            assert!(matches!(output.value, Value::Unit));
        }
    }
}
