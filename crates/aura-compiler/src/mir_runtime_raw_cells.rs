//! A local whose field has an exposed address keeps one aligned C
//! representation until the containing activation returns.
use super::*;
use crate::c_layout::CStruct;

pub(super) struct CellBytes {
    layout: CStruct,
    allocation: Vec<u8>,
    start: usize,
}

impl CellBytes {
    fn new(layout: &CStruct, value: &Value) -> Result<Self> {
        let mut allocation = vec![0; layout.size() + layout.align() - 1];
        let start = allocation.as_ptr().align_offset(layout.align());
        write_struct_bytes(value, layout, &mut allocation[start..start + layout.size()])?;
        Ok(Self {
            layout: layout.clone(),
            allocation,
            start,
        })
    }

    fn read(&self) -> Result<Value> {
        struct_runtime_value(
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
            self.raw_cells
                .insert(root.to_string(), CellBytes::new(layout, &value)?);
        }
        let cell = self.raw_cells.get_mut(root).expect("initialized C local");
        Ok(cell
            .allocation
            .as_mut_ptr()
            .wrapping_add(cell.start + offset) as usize as u64)
    }

    pub(super) fn refresh_raw_cells(&mut self) -> Result<()> {
        for (name, cell) in &self.raw_cells {
            if let Some(value) = self.values.get_mut(name) {
                *value = cell.read()?;
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
        let cell = self.raw_cells.get_mut(root).expect("addressed C local");
        let whole = if rest.is_empty() {
            try_clone_mir_value(value)?
        } else {
            let mut whole = cell.read()?;
            let path = rest.iter().map(String::as_str).collect::<Vec<_>>();
            write_nested_place(&mut whole, &path, try_clone_mir_value(value)?, place)?;
            whole
        };
        write_struct_bytes(
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
    let ptr = address as *mut u8;
    let mut bytes = vec![0; layout.size()];
    // SAFETY: the FFI grant places the address range and lifetime under the
    // caller's control. A byte access does not require aggregate alignment.
    if let Some(value) = store {
        write_struct_bytes(value, layout, &mut bytes)?;
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
        struct_runtime_value(layout, &bytes, "pointer.read")
    }
}

pub(super) fn requires_package_grant(value: &Rvalue) -> bool {
    match value {
        Rvalue::CPointerAccess { .. } | Rvalue::CFieldAddress { .. } => true,
        Rvalue::Call { callee: CallTarget::Name(name), .. } => name.starts_with("mem::"),
        Rvalue::Call { callee: CallTarget::Runtime { op: RuntimeOp::Host(host), .. }, .. } =>
            host.name().starts_with("mem::"),
        _ => false,
    }
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
        for method in ["return p.read()", "p.write(1)\n    return 0"] {
            let source = format!("def operation(p: ptr[int32]) -> int32:\n    {method}\n");
            let module = lower(&source);
            assert_eq!(reject_untrusted_extern_calls(&module).unwrap_err().code, "AU2084");
        }
        let source = "extern \"C\" class Item:\n    value: int32\n\ndef operation(p: ptr[Item]) -> int32:\n    return p.read().value\n";
        assert_eq!(reject_untrusted_extern_calls(&lower(source)).unwrap_err().code, "AU2084");
    }

    #[test]
    fn typed_pointer_interpreter_runs_scalar_and_aggregate_operations() {
        for source in [
            include_str!("../tests/fixtures/typed-pointers/scalars.au"),
            include_str!("../tests/fixtures/typed-pointers/aggregate.au"),
            include_str!("../tests/fixtures/typed-pointers/foreign.au"),
            include_str!("../tests/fixtures/typed-pointers/links.au"),
            include_str!("../tests/fixtures/typed-pointers/widths.au"),
        ] {
            let output = crate::mir_runtime::run_trusted(&lower(source))
                .expect("a valid allocation should preserve stored values");
            assert!(matches!(output.value, Value::Unit));
        }
    }
}
