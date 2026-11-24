//! The heap of a freestanding program (ADR-0128).
//!
//! A freestanding program asks for memory through three hooks, C symbols
//! its environment may define:
//!
//! ```c
//! void *aura_alloc(size_t size, size_t align);
//! void aura_free(void *address, size_t size, size_t align);
//! void *aura_realloc(void *address, size_t old_size, size_t align, size_t new_size);
//! ```
//!
//! `mem.alloc`, `mem.free`, and `mem.realloc` call them. A program that
//! declares none of them gets Aura's own allocator: [`include_allocator`]
//! adds the functions of `std/freestanding_heap.au` to the program, and
//! the object defines the hooks, `aura_heap_add_region`, and
//! `aura_heap_free_bytes` as weak symbols that call those functions with
//! the address of the heap's state, which the object holds.

use super::*;

/// The allocator, in Aura.
const ALLOCATOR_SOURCE: &str = include_str!("../std/freestanding_heap.au");

/// The path a failed check in the allocator reports to the trap hook.
const ALLOCATOR_PATH: &str = "aura/freestanding_heap.au";

/// What each allocator function is named in a program, so no function of
/// the program has the same name. A source name holds no `:`.
const ALLOCATOR_PREFIX: &str = "aura::heap::";

/// The bytes of the heap's state, which the allocator's source describes.
const STATE_BYTES: usize = 784;

/// One C symbol of the heap: a hook, or a function of Aura's allocator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HeapSymbol {
    /// The `mem` function that calls the symbol.
    pub(crate) builtin: &'static str,
    pub(crate) symbol: &'static str,
    /// How many words the symbol takes.
    pub(crate) params: usize,
    pub(crate) returns: bool,
    /// The allocator function that defines the symbol by default, and
    /// which of the symbol's words it takes after the state's address.
    function: &'static str,
    passes: &'static [usize],
}

/// Every C symbol of the heap. The first three are the hooks.
pub(crate) const HEAP_SYMBOLS: [HeapSymbol; 5] = [
    HeapSymbol {
        builtin: "mem::alloc",
        symbol: "aura_alloc",
        params: 2,
        returns: true,
        function: "heap_alloc",
        passes: &[0, 1],
    },
    HeapSymbol {
        builtin: "mem::free",
        symbol: "aura_free",
        params: 3,
        returns: false,
        function: "heap_free",
        passes: &[0],
    },
    HeapSymbol {
        builtin: "mem::realloc",
        symbol: "aura_realloc",
        params: 4,
        returns: true,
        function: "heap_realloc",
        passes: &[0, 2, 3],
    },
    HeapSymbol {
        builtin: "mem::heap_add_region",
        symbol: "aura_heap_add_region",
        params: 2,
        returns: false,
        function: "heap_add_region",
        passes: &[0, 1],
    },
    HeapSymbol {
        builtin: "mem::heap_free_bytes",
        symbol: "aura_heap_free_bytes",
        params: 0,
        returns: true,
        function: "heap_free_bytes",
        passes: &[],
    },
];

/// The hooks: the symbols a program defines to bring its own allocator.
const HOOKS: usize = 3;

/// The heap symbol the `mem` function `name` calls.
pub(crate) fn heap_operation(name: &str) -> Option<&'static HeapSymbol> {
    HEAP_SYMBOLS.iter().find(|symbol| symbol.builtin == name)
}

/// Whether `symbol` is a C symbol of the heap.
pub(crate) fn is_heap_symbol(symbol: &str) -> bool {
    HEAP_SYMBOLS.iter().any(|known| known.symbol == symbol)
}

fn allocator_name(function: &str) -> String {
    format!("{ALLOCATOR_PREFIX}{function}")
}

/// Whether `module` holds Aura's allocator.
pub(super) fn has_allocator(module: &MirModule) -> bool {
    let name = allocator_name(HEAP_SYMBOLS[0].function);
    module
        .functions
        .iter()
        .any(|function| function.name == name)
}

/// Whether a function of `module` calls a `mem` function of the heap.
fn uses_heap(module: &MirModule) -> bool {
    module
        .functions
        .iter()
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.instructions)
        .any(|instruction| {
            matches!(
                instruction,
                Instruction::Assign {
                    value: Rvalue::Call {
                        callee: CallTarget::Runtime {
                            op: RuntimeOp::Host(builtin),
                            ..
                        },
                        ..
                    },
                    ..
                } if heap_operation(builtin.name()).is_some()
            )
        })
}

/// The allocator's functions as a program holds them: lowered from its
/// source and renamed.
fn allocator_functions() -> Vec<MirFunction> {
    let module = crate::parse_source(ALLOCATOR_SOURCE).expect("the allocator parses");
    let program =
        crate::check_module_with_builtin_imports(module).expect("the allocator type-checks");
    let lowered = crate::mir::lower(&program);
    let names = lowered
        .functions
        .iter()
        .map(|function| function.name.clone())
        .filter(|name| name.starts_with("heap_"))
        .collect::<HashSet<_>>();
    // The lowered module also holds a function for each host builtin.
    // The program has its own, and the allocator's are left behind.
    let own = lowered
        .functions
        .iter()
        .filter(|function| names.contains(&function.name))
        .collect::<Vec<_>>();
    // A function's name appears in its own record and in each call of
    // it, and nowhere else: no local of the allocator shares a name with
    // a function.
    fn rename(value: &mut serde_json::Value, names: &HashSet<String>) {
        match value {
            serde_json::Value::String(text) if names.contains(text.as_str()) => {
                *text = allocator_name(text);
            }
            serde_json::Value::Array(items) => {
                items.iter_mut().for_each(|item| rename(item, names));
            }
            serde_json::Value::Object(fields) => {
                fields.values_mut().for_each(|item| rename(item, names));
            }
            _ => {}
        }
    }
    let mut value = serde_json::to_value(&own).expect("MIR functions are JSON values");
    rename(&mut value, &names);
    let mut functions: Vec<MirFunction> =
        serde_json::from_value(value).expect("renamed MIR functions read back");
    for function in &mut functions {
        function.source_path = Some(ALLOCATOR_PATH.to_string());
    }
    functions
}

/// Adds Aura's allocator to `module`, a freestanding program, when the
/// program calls a heap function of `mem` and declares no hook itself.
/// `declared` holds the C symbols the program exports or imports.
pub(super) fn include_allocator(module: &mut MirModule, declared: &HashSet<String>) {
    if uses_heap(module)
        && !HEAP_SYMBOLS[..HOOKS]
            .iter()
            .any(|hook| declared.contains(hook.symbol))
    {
        module.functions.extend(allocator_functions());
    }
}

impl NativeCodegen<'_> {
    /// Declares the heap symbols Aura's allocator defines, when the
    /// program holds it. A symbol the program declares itself is left to
    /// the program. Call this before any body is built, so a body that
    /// names a symbol calls the definition here.
    pub(super) fn declare_heap_symbols(
        &mut self,
        declared: &HashSet<&str>,
    ) -> std::result::Result<Vec<(HeapSymbol, Signature, FuncHandle)>, String> {
        let mut entries = Vec::new();
        if !has_allocator(self.module) {
            return Ok(entries);
        }
        for symbol in HEAP_SYMBOLS {
            if declared.contains(symbol.symbol) {
                continue;
            }
            let signature = heap_signature(&symbol);
            let entry = try_or_string_error!(
                self.module_backend()?.declare_function(
                    symbol.symbol,
                    Linkage::Export,
                    &signature
                ),
                "failed to declare `{}`: {}",
                symbol.symbol
            );
            self.module_backend()?.mark_weak_routine(entry);
            entries.push((symbol, signature, entry));
        }
        Ok(entries)
    }

    /// Defines the heap's state and the symbols `declare_heap_symbols`
    /// declared: each calls its allocator function with the state's
    /// address.
    pub(super) fn define_heap_symbols(
        &mut self,
        entries: Vec<(HeapSymbol, Signature, FuncHandle)>,
    ) -> std::result::Result<(), String> {
        if entries.is_empty() {
            return Ok(());
        }
        let state = try_or_string_error!(
            self.module_backend()?
                .declare_data("aura_heap_state", Linkage::Local, true),
            "failed to declare the heap's state: {}"
        );
        let mut contents = DataDescription::new(vec![0; STATE_BYTES]);
        contents.align = Some(16);
        try_or_string_error!(
            self.module_backend()?.define_data(state, contents),
            "failed to define the heap's state: {}"
        );
        for (symbol, signature, entry) in entries {
            let function = self.functions[&allocator_name(symbol.function)];
            self.define_body(entry, &signature, symbol.symbol, |_, builder| {
                let block = builder.create_block();
                builder.append_block_params_for_function_params(block);
                builder.switch_to_block(block);
                builder.seal_block(block);
                let arguments = builder.block_params(block);
                let state = builder.import_data(state);
                let mut words = vec![builder.symbol_value(types::I64, state)];
                words.extend(symbol.passes.iter().map(|&index| arguments[index]));
                let function = builder.import_function(function);
                let results = builder.call(function, &words).results;
                if symbol.returns {
                    builder.return_(&[results[0]]);
                } else {
                    builder.return_(&[]);
                }
                Ok(())
            })?;
        }
        Ok(())
    }
}

/// The C signature of a heap symbol: words in, and at most one word out.
fn heap_signature(symbol: &HeapSymbol) -> Signature {
    let mut signature = Signature::new(CallConv::C);
    signature.params = vec![types::I64; symbol.params];
    if symbol.returns {
        signature.returns = vec![types::I64];
    }
    signature
}

impl FunctionCompiler<'_> {
    /// A call of a heap function of `mem` in a freestanding object: a
    /// call of its C symbol. A hosted build calls the runtime's function
    /// of the same name, which uses the process allocator.
    pub(super) fn compile_heap_call(
        &mut self,
        name: &str,
        args: &[MirArg],
    ) -> std::result::Result<Option<ValueRef>, String> {
        let Some(symbol) = heap_operation(name) else {
            return Ok(None);
        };
        if self.freestanding.is_none() {
            return Ok(None);
        }
        let mut words = Vec::with_capacity(args.len());
        for argument in args {
            words.push(self.load_operand(&argument.value)?.values[0]);
        }
        let signature = self.builder.import_signature(heap_signature(symbol));
        let callee = try_or_string_error!(
            self.builder.module().declare_foreign_symbol(symbol.symbol),
            "failed to declare `{}`: {}",
            symbol.symbol
        );
        let callee = self.builder.import_function(callee);
        let callee = self.builder.func_addr(types::I64, callee);
        let results = self
            .builder
            .call_indirect(signature, callee, &words)
            .results;
        Ok(Some(if symbol.returns {
            ValueRef {
                values: vec![results[0]],
                ty: DirectType::Scalar(ScalarKind::Uint64),
            }
        } else {
            unit_value(self.builder)
        }))
    }
}

#[cfg(test)]
#[path = "native_codegen_freestanding_heap_tests.rs"]
mod tests;
