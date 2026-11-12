//! Freestanding objects (ADR-0094): code for a machine with no operating
//! system and no Aura runtime.
//!
//! A freestanding program holds only machine words, so the emitter's
//! word-only paths compile it. This module holds what differs from a host
//! object:
//!
//! - [`prepare`] keeps the functions the exports reach, drops the text an
//!   assertion would render, and reports each construct that needs the
//!   runtime as an `AU2052` diagnostic.
//! - A failed check calls the trap hook `aura_trap` in place of the
//!   runtime.
//! - Each `export "C" def` gets an entry under its C symbol.
//! - The object carries weak definitions of the four C memory routines
//!   that LLVM may call.
//! - [`check_object`] refuses an object that names any other symbol it
//!   does not define.

use super::*;
use crate::diag::Diagnostic;
use crate::ffi::CCallback;
use crate::native_backend::MemoryAccess;

/// The diagnostic code of a construct a freestanding build cannot compile.
pub(crate) const UNSUPPORTED: &str = "AU2052";

/// The symbol of the trap hook: `void aura_trap(uint32_t code, const char
/// *file, size_t file_len, uint32_t line, uint32_t column)`.
pub(crate) const TRAP_HOOK: &str = "aura_trap";

/// The C memory routines LLVM may call for a copy, a fill, or a compare.
pub(crate) const MEMORY_ROUTINES: [&str; 4] = ["memcpy", "memmove", "memset", "memcmp"];

pub(super) const TRAP_OVERFLOW: i64 = 1;
pub(super) const TRAP_DIVISION: i64 = 2;
pub(super) const TRAP_ASSERT: i64 = 3;
pub(super) const TRAP_CONTRACT: i64 = 4;
pub(super) const TRAP_CAST: i64 = 5;
pub(super) const TRAP_SHIFT: i64 = 6;
pub(super) const TRAP_FOREIGN_VALUE: i64 = 7;
pub(super) const TRAP_UNREACHABLE: i64 = 8;

/// The C signature of an export or a callback of a freestanding object.
///
/// An integer result narrower than a register is returned as the whole
/// register, extended the way the target's convention asks, so no caller
/// reads bits the code left undefined.
pub(super) fn entry_signature(callback: &CCallback, target: FreestandingTarget) -> Signature {
    let _ = target;
    let (mut entry, _) = foreign::callback_signatures(callback);
    entry.returns = callback
        .result
        .map(|result| match result.ffi_type() {
            FfiType::F32 => types::F32,
            FfiType::F64 => types::F64,
            _ => types::I64,
        })
        .into_iter()
        .collect();
    entry
}

/// The C signature an exported function is called with, or `None` when a
/// parameter or the result is not a scalar.
fn export_signature(function: &MirFunction) -> Option<CCallback> {
    let ty = Type::Function {
        params: function
            .params
            .iter()
            .map(|param| FunctionParamContract {
                name: param.name.clone(),
                ty: param.ty.clone(),
                passing: match param.passing {
                    MirReceiverKind::BorrowMut => ReceiverKind::BorrowMut,
                    _ => ReceiverKind::Value,
                },
                has_default: false,
                keyword_only: false,
            })
            .collect(),
        return_type: Box::new(function.return_type.clone()),
    };
    crate::mir_runtime::callback_signature(&ty)
}

/// The marker of an emitter error that reports a construct a freestanding
/// build cannot compile, which the subset check could not see in the MIR.
const REFUSAL: &str = "freestanding build cannot compile: ";

/// The emitter error for `what`, a use the program must change.
pub(super) fn refusal(what: String) -> String {
    format!("{REFUSAL}{what}")
}

/// The diagnostic for an emitter error: `AU2052` for a refusal, and an
/// uncoded one for a fault of the code generator.
pub(super) fn emitter_diagnostic(error: String) -> Diagnostic {
    match error.strip_prefix(REFUSAL) {
        Some(what) => Diagnostic::coded(
            UNSUPPORTED,
            format!("{what}, which a freestanding build cannot compile"),
        ),
        None => Diagnostic::new(error),
    }
}

/// The path the trap hook gets for the source file `path` of the program
/// at `program_path`: relative to the program file's directory when the
/// file lies under it, so the object's bytes do not depend on where the
/// package was checked out.
pub(super) fn trap_path(path: &str, program_path: &str) -> String {
    let directory =
        std::path::Path::new(program_path)
            .parent()
            .and_then(|directory| match directory.as_os_str().is_empty() {
                true => std::env::current_dir().ok(),
                false => std::fs::canonicalize(directory).ok(),
            });
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.into());
    match directory
        .as_ref()
        .and_then(|directory| resolved.strip_prefix(directory).ok())
    {
        Some(relative) => relative.to_string_lossy().replace('\\', "/"),
        None => path.to_string(),
    }
}

impl NativeCodegen<'_> {
    /// Defines every function of a freestanding program, the entries of
    /// its exports and callbacks, and the memory routines.
    pub(super) fn define_freestanding_program(&mut self) -> std::result::Result<(), String> {
        let target = self
            .freestanding
            .expect("only a freestanding program is defined here");
        // An export is declared before any body is built, so a body that
        // names the same symbol, as a check names the trap hook, calls the
        // export.
        let mut exports = Vec::new();
        for export in &self.module.foreign_link.exports {
            let function = self
                .module
                .functions
                .iter()
                .find(|function| function.name == export.function)
                .ok_or_else(|| {
                    format!(
                        "freestanding build cannot find exported function `{}`",
                        export.function
                    )
                })?;
            let callback = export_signature(function).ok_or_else(|| {
                format!(
                    "freestanding build cannot export `{}`: its parameters and result must be scalars",
                    export.function
                )
            })?;
            let signature = entry_signature(&callback, target);
            let entry = try_or_string_error!(
                self.module_backend()?.declare_function(
                    &export.symbol,
                    Linkage::Export,
                    &signature
                ),
                "failed to declare export `{}`: {}",
                export.symbol
            );
            exports.push((export, callback, signature, entry));
        }
        for function in &self.module.functions {
            self.define_function(function)?;
        }
        for ((name, callback), (entry_id, body_id)) in self.callback_thunks.clone() {
            let signature = entry_signature(&callback, target);
            let function = self.functions[&name];
            self.define_body(
                entry_id,
                &signature,
                &format!("callback entry for `{name}`"),
                |_, builder| {
                    emit_entry(builder, &callback, function, target);
                    Ok(())
                },
            )?;
            // The host form of the entry runs this body under the
            // runtime's guard. Nothing calls it here.
            let (_, body) = foreign::callback_signatures(&callback);
            self.define_body(
                body_id,
                &body,
                &format!("callback body for `{name}`"),
                |_, builder| {
                    foreign::emit_callback_body(builder, &callback, function);
                    Ok(())
                },
            )?;
        }
        for (export, callback, signature, entry) in exports {
            let function = self.functions[&export.function];
            self.define_body(
                entry,
                &signature,
                &format!("export `{}`", export.symbol),
                |_, builder| {
                    emit_entry(builder, &callback, function, target);
                    Ok(())
                },
            )?;
        }
        // The module's retain and release are declared for every object.
        // Nothing in a freestanding program calls them, and LLVM drops
        // them.
        self.define_ownership_fast_paths()?;
        self.define_memory_routines()
    }

    /// Defines the C memory routines as weak byte loops, so the object
    /// links without a C library. A routine the program declares itself,
    /// as an export or an `extern`, is left to the program.
    fn define_memory_routines(&mut self) -> std::result::Result<(), String> {
        let declared = declared_symbols(self.module);
        for name in MEMORY_ROUTINES {
            if declared.contains(name) {
                continue;
            }
            let mut signature = Signature::new(CallConv::C);
            signature.params = vec![types::I64; 3];
            signature.returns = vec![types::I64];
            let routine = try_or_string_error!(
                self.module_backend()?
                    .declare_function(name, Linkage::Export, &signature),
                "failed to declare `{}`: {}",
                name
            );
            self.module_backend()?.mark_weak_routine(routine);
            self.define_body(routine, &signature, name, |_, builder| {
                emit_memory_routine(builder, name);
                Ok(())
            })?;
        }
        Ok(())
    }
}

/// Every C symbol the module names itself: its exports and the functions
/// its foreign calls reach.
fn declared_symbols(module: &MirModule) -> HashSet<&str> {
    let mut symbols = module
        .foreign_link
        .exports
        .iter()
        .map(|export| export.symbol.as_str())
        .collect::<HashSet<_>>();
    for instruction in module
        .functions
        .iter()
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.instructions)
    {
        if let Instruction::Assign {
            value:
                Rvalue::Call {
                    callee: CallTarget::Extern(call),
                    ..
                },
            ..
        } = instruction
        {
            symbols.insert(call.symbol.as_str());
        }
    }
    symbols
}

/// The code C calls for an export or a callback: it turns the C arguments
/// into Aura words, calls `function`, stores what the function wrote to
/// its `mut` parameters through their pointers, and returns the result.
fn emit_entry(
    builder: &mut dyn FunctionBackend,
    callback: &CCallback,
    function: FuncHandle,
    target: FreestandingTarget,
) {
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    let arguments = builder.block_params(entry);
    let flags = MemFlags::trusted();
    // A null pointer argument reads and writes this word instead.
    let spare = callback.params.iter().any(|param| param.pointer).then(|| {
        let spare = builder.create_sized_stack_slot(8, 3);
        let spare = builder.stack_addr(types::I64, spare, 0);
        let zero = builder.iconst(types::I64, 0);
        builder.store(flags, zero, spare, 0);
        spare
    });
    let mut words = Vec::new();
    let mut pointees = Vec::new();
    for (param, argument) in callback.params.iter().zip(arguments) {
        let scalar =
            foreign::foreign_scalar(param.scalar.ffi_type()).expect("an entry takes scalars");
        let raw = if param.pointer {
            let spare = spare.expect("a pointer parameter has a spare word");
            let present = builder.icmp_imm(IntCC::NotEqual, argument, 0);
            let address = builder.select(present, argument, spare);
            pointees.push((scalar.ty, address));
            builder.load(scalar.ty, flags, address, 0)
        } else {
            argument
        };
        words.push(foreign::callback_word(builder, &scalar, raw));
    }
    let function = builder.import_function(function);
    let results = builder.call(function, &words).results;
    // The function returns its result, then one word for each `mut`
    // parameter in order.
    let written = &results[results.len() - pointees.len()..];
    for ((ty, address), &word) in pointees.into_iter().zip(written) {
        let narrow = foreign::callback_narrow(builder, ty, word);
        builder.store(flags, narrow, address, 0);
    }
    let Some(result) = callback.result else {
        builder.return_(&[]);
        return;
    };
    let word = results[0];
    let returned = match result.ffi_type() {
        FfiType::F32 => builder.fdemote(types::F32, word),
        // RISC-V keeps a 32-bit integer sign-extended in its register,
        // an unsigned one too. An Aura `uint32` word is zero-extended.
        FfiType::U32 if target == FreestandingTarget::Riscv64 => {
            let shift = builder.iconst(types::I64, 32);
            let high = builder.ishl(word, shift);
            builder.sshr(high, shift)
        }
        _ => word,
    };
    builder.return_(&[returned]);
}

/// Builds the body of the memory routine `name` as a byte loop.
fn emit_memory_routine(builder: &mut dyn FunctionBackend, name: &str) {
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    let [first, second, count] = builder.block_params(entry)[..] else {
        unreachable!("a memory routine takes three words");
    };
    let flags = MemFlags::new();
    /// One iteration of a memory routine's loop, given the index. It may
    /// leave the loop with a result.
    type LoopBody<'a> = &'a mut dyn FnMut(&mut dyn FunctionBackend, Val) -> Option<(Val, Val)>;
    // A loop over `index` from `start` while `index != end`, by `step`.
    // `body` builds one iteration and may leave the loop with a result;
    // the loop's own exit returns `fallthrough`.
    let counted = |builder: &mut dyn FunctionBackend,
                   start: Val,
                   end: Val,
                   step: i64,
                   fallthrough: Val,
                   body: LoopBody<'_>| {
        let head = builder.create_block();
        let index = builder.append_block_param(head, types::I64);
        let work = builder.create_block();
        let done = builder.create_block();
        builder.jump(head, &[start]);
        builder.switch_to_block(head);
        let finished = builder.icmp(IntCC::Equal, index, end);
        builder.brif(finished, done, &[], work, &[]);
        builder.switch_to_block(work);
        builder.seal_block(work);
        let at = if step < 0 {
            builder.iadd_imm(index, -1)
        } else {
            index
        };
        if let Some((leave, result)) = body(builder, at) {
            let out = builder.create_block();
            let stay = builder.create_block();
            builder.brif(leave, out, &[], stay, &[]);
            builder.switch_to_block(out);
            builder.seal_block(out);
            builder.return_(&[result]);
            builder.switch_to_block(stay);
            builder.seal_block(stay);
        }
        let next = builder.iadd_imm(index, step);
        builder.jump(head, &[next]);
        builder.seal_block(head);
        builder.switch_to_block(done);
        builder.seal_block(done);
        builder.return_(&[fallthrough]);
    };
    let zero = builder.iconst(types::I64, 0);
    let mut copy = |builder: &mut dyn FunctionBackend, at: Val| {
        let source = builder.element_address(second, at);
        let byte = builder.uload8(types::I64, flags, source, 0);
        let byte = builder.ireduce(types::I8, byte);
        let target = builder.element_address(first, at);
        builder.store(flags, byte, target, 0);
        None
    };
    match name {
        "memcpy" => counted(builder, zero, count, 1, first, &mut copy),
        "memmove" => {
            // Bytes move front to back unless the target starts inside the
            // source, where that would overwrite bytes not yet read.
            let distance = builder.isub(first, second);
            let overlaps = builder.icmp(IntCC::UnsignedLessThan, distance, count);
            let backward = builder.create_block();
            let forward = builder.create_block();
            builder.brif(overlaps, backward, &[], forward, &[]);
            builder.switch_to_block(backward);
            builder.seal_block(backward);
            counted(builder, count, zero, -1, first, &mut copy);
            builder.switch_to_block(forward);
            builder.seal_block(forward);
            counted(builder, zero, count, 1, first, &mut copy);
        }
        "memset" => {
            let byte = builder.ireduce(types::I8, second);
            counted(builder, zero, count, 1, first, &mut |builder, at| {
                let target = builder.element_address(first, at);
                builder.store(flags, byte, target, 0);
                None
            });
        }
        "memcmp" => counted(builder, zero, count, 1, zero, &mut |builder, at| {
            let left = builder.element_address(first, at);
            let left = builder.uload8(types::I64, flags, left, 0);
            let right = builder.element_address(second, at);
            let right = builder.uload8(types::I64, flags, right, 0);
            let differ = builder.icmp(IntCC::NotEqual, left, right);
            Some((differ, builder.isub(left, right)))
        }),
        other => unreachable!("`{other}` is not a memory routine"),
    }
}

impl FunctionCompiler<'_> {
    /// Reports a failed check: a host object calls the runtime's
    /// `function` with `args`, and a freestanding one calls the trap hook
    /// with the check's code and source position.
    pub(super) fn call_failure(&mut self, function: RuntimeFn, args: &[Val]) {
        if self.freestanding.is_none() {
            self.builder.call(self.runtime[function], args);
            return;
        }
        // Each of these takes the line and the column last.
        let code = match function {
            RuntimeFn::FailDivisionByZero => TRAP_DIVISION,
            RuntimeFn::CastIntegerToInteger => TRAP_CAST,
            RuntimeFn::FailInt32Overflow | RuntimeFn::FailNarrowOverflow if self.checks_cast => {
                TRAP_CAST
            }
            _ => TRAP_OVERFLOW,
        };
        let [.., line, column] = args else {
            unreachable!("a failure helper takes a line and a column");
        };
        self.call_trap_hook(code, *line, *column);
    }

    /// Calls the trap hook with the constant `code`.
    pub(super) fn call_trap_hook(&mut self, code: i64, line: Val, column: Val) {
        let code = self.builder.iconst(types::I64, code);
        self.call_trap_hook_with(code, line, column);
    }

    /// Calls the trap hook: the code, the source file's path and its
    /// length in bytes, and the line and column of the failed check. The
    /// caller ends the block with a trap instruction, which runs if the
    /// hook returns.
    pub(super) fn call_trap_hook_with(&mut self, code: Val, line: Val, column: Val) {
        let path = self.current_function_path.clone();
        let (file, file_len) = self
            .string_constant(path.as_bytes())
            .expect("a path is a valid string constant");
        // Every argument fills a register, so a hook that declares the
        // code, line, and column as 32-bit integers reads the same values.
        let mut signature = Signature::new(CallConv::C);
        signature.params = vec![types::I64; 5];
        let signature = self.builder.import_signature(signature);
        let hook = self
            .builder
            .module()
            .declare_foreign_symbol(TRAP_HOOK)
            .expect("the trap hook's name is free for a function");
        let hook = self.builder.import_function(hook);
        let hook = self.builder.func_addr(types::I64, hook);
        self.builder
            .call_indirect(signature, hook, &[code, file, file_len, line, column]);
    }

    /// The slow path of an integer-to-float cast in a freestanding object:
    /// `value` as a float when a significand of `exact_bits` bits holds it
    /// exactly, and a trap otherwise. The runtime's cast decides the same
    /// way.
    pub(super) fn freestanding_wide_integer_to_float(
        &mut self,
        value: Val,
        unsigned: bool,
        exact_bits: u32,
        line: Val,
        column: Val,
    ) -> Val {
        let magnitude = if unsigned {
            value
        } else {
            let negative = self.builder.icmp_imm(IntCC::SignedLessThan, value, 0);
            let negated = self.builder.ineg(value);
            self.builder.select(negative, negated, value)
        };
        // The value is exact when its set bits span at most `exact_bits`
        // places: below `lowest << exact_bits`, where `lowest` is its
        // lowest set bit.
        let negated = self.builder.ineg(magnitude);
        let lowest = self.builder.band(magnitude, negated);
        let shift = self.builder.iconst(types::I64, i64::from(exact_bits));
        let high = self.builder.ushr(magnitude, shift);
        let inexact = self
            .builder
            .icmp(IntCC::UnsignedGreaterThanOrEqual, high, lowest);
        let fail_block = self.builder.create_block();
        let exact_block = self.builder.create_block();
        self.builder
            .brif(inexact, fail_block, &[], exact_block, &[]);
        self.builder.switch_to_block(fail_block);
        self.builder.seal_block(fail_block);
        self.call_trap_hook(TRAP_CAST, line, column);
        self.builder.trap(TrapCode::IntegerOverflow);
        self.builder.switch_to_block(exact_block);
        self.builder.seal_block(exact_block);
        if unsigned {
            self.builder.fcvt_from_uint(types::F64, value)
        } else {
            self.builder.fcvt_from_sint(types::F64, value)
        }
    }
}

/// `module` as a freestanding build compiles it, or the constructs it
/// cannot compile.
///
/// The result holds the functions the exports reach. An assertion's
/// message and rendered operands are dropped: the trap hook gets a code
/// and a position, and rendering text needs the runtime.
pub(crate) fn prepare(module: &MirModule) -> std::result::Result<MirModule, Vec<Diagnostic>> {
    let mut module = module.clone();
    let mut problems = Vec::new();
    if module.foreign_link.exports.is_empty() {
        problems.push(
            Diagnostic::coded(
                UNSUPPORTED,
                "a freestanding program needs at least one `export \"C\" def` function",
            )
            .with_help(
                "a freestanding object has no `main`: the code that links it calls its exported functions",
            ),
        );
        return Err(problems);
    }
    let mut symbols = HashSet::new();
    for export in &module.foreign_link.exports {
        if !symbols.insert(export.symbol.as_str()) {
            problems.push(Diagnostic::coded(
                UNSUPPORTED,
                format!(
                    "two functions are exported as `{}`; an object defines a C symbol once",
                    export.symbol
                ),
            ));
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    keep_reached_functions(&mut module);
    for function in &mut module.functions {
        strip_assertion_text(function);
    }
    let classes = module
        .classes
        .iter()
        .map(|class| (class.name.clone(), class.clone()))
        .collect::<HashMap<_, _>>();
    let functions = module
        .functions
        .iter()
        .map(|function| function.name.as_str())
        .collect::<HashSet<_>>();
    for function in &module.functions {
        check_function(function, &classes, &functions, &mut problems);
    }
    // Each copy of a generic function reports the same line.
    let mut kept: Vec<Diagnostic> = Vec::new();
    for problem in problems {
        let repeated =
            |other: &Diagnostic| other.span == problem.span && other.message == problem.message;
        if !kept.iter().any(repeated) {
            kept.push(problem);
        }
    }
    let problems = kept;
    if problems.is_empty() {
        Ok(module)
    } else {
        Err(problems)
    }
}

/// Keeps the functions the exports reach and drops every other function,
/// the top-level statements, and the module constants nothing reads. The
/// walk follows every string in a kept function, as
/// `reachability::prune_unreachable_functions` does, so it keeps a
/// function named any way.
fn keep_reached_functions(module: &mut MirModule) {
    let by_name = module
        .functions
        .iter()
        .map(|function| (function.name.as_str(), function))
        .collect::<HashMap<_, _>>();
    let mut kept = HashSet::new();
    let mut pending = Vec::new();
    for export in &module.foreign_link.exports {
        if let Some(function) = by_name.get(export.function.as_str()) {
            if kept.insert(export.function.clone()) {
                pending.push(*function);
            }
        }
    }
    let mut names = Vec::new();
    while let Some(function) = pending.pop() {
        let Ok(value) = serde_json::to_value(&function.blocks) else {
            return;
        };
        collect_strings(&value, &mut names);
        for name in names.drain(..) {
            if let Some(function) = by_name.get(name.as_str()) {
                if kept.insert(name) {
                    pending.push(*function);
                }
            }
        }
    }
    module
        .functions
        .retain(|function| kept.contains(&function.name));
    module.top_level = None;
    let read = module
        .functions
        .iter()
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::Assign {
                value: Rvalue::ModuleConstant { key, .. },
                ..
            } => Some(key.clone()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    module
        .constants
        .retain(|constant| read.contains(&constant.key));
}

fn collect_strings(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => out.push(text.clone()),
        serde_json::Value::Array(items) => items.iter().for_each(|item| collect_strings(item, out)),
        serde_json::Value::Object(fields) => {
            fields.values().for_each(|item| collect_strings(item, out))
        }
        _ => {}
    }
}

/// Drops what each failed assertion of `function` would render: the block
/// that ends in the failure computes only its message and its operands'
/// text. Locals nothing mentions afterwards are dropped with them.
fn strip_assertion_text(function: &mut MirFunction) {
    for block in &mut function.blocks {
        if let Terminator::AssertFail {
            message, captures, ..
        } = &mut block.terminator
        {
            *message = None;
            captures.clear();
            block.instructions.clear();
        }
    }
    let mut mentioned = HashSet::new();
    for block in &function.blocks {
        let mut mentions = Vec::new();
        for instruction in &block.instructions {
            crate::mir::instruction_place_mentions(instruction, &mut mentions);
            if let Instruction::Assign { target, .. } = instruction {
                mentions.push(target);
            }
        }
        mentions.extend(crate::mir::terminator_place_mentions(&block.terminator));
        for place in mentions {
            let root = place.split(['.', '[']).next().unwrap_or(place);
            mentioned.insert(root.to_string());
        }
    }
    function
        .local_types
        .retain(|local| local.name == "self" || mentioned.contains(&local.name));
}

/// Whether a freestanding program can hold a value of type `ty`: the
/// emitter keeps it in machine words with no box.
fn words_only(ty: &Type, classes: &HashMap<String, MirClass>) -> bool {
    fn words(ty: &DirectType) -> bool {
        match ty {
            DirectType::Scalar(_) | DirectType::PlainClass(_) => true,
            DirectType::Union(union) => {
                union.owning.is_empty() && union.members.iter().all(|member| words(&member.ty))
            }
            DirectType::Callable(_) | DirectType::Opaque(_) => false,
        }
    }
    direct_type(ty, classes).is_some_and(|ty| words(&ty))
}

const WORDS_HELP: &str = "a freestanding program holds fixed-width integers, floats, `bool`, Copy classes of those, and their unions with `None`";

/// Reports each construct of `function` that a freestanding build cannot
/// compile.
fn check_function(
    function: &MirFunction,
    classes: &HashMap<String, MirClass>,
    functions: &HashSet<&str>,
    problems: &mut Vec<Diagnostic>,
) {
    let name = public_direct_function_name(&function.name);
    let found = std::cell::RefCell::new(Vec::<Diagnostic>::new());
    let report = |span: crate::diag::Span, what: String, help: &str| {
        let mut diagnostic = Diagnostic::coded_at(
            UNSUPPORTED,
            span,
            format!("`{name}` {what}, which a freestanding build cannot compile"),
        );
        if !help.is_empty() {
            diagnostic = diagnostic.with_help(help);
        }
        if let Some(path) = &function.source_path {
            diagnostic.render_path = Some(path.clone());
        }
        // One report for each construct and position is enough.
        let mut found = found.borrow_mut();
        if !found
            .iter()
            .any(|known| known.message == diagnostic.message && known.span == diagnostic.span)
        {
            found.push(diagnostic);
        }
    };
    let typed = function
        .params
        .iter()
        .map(|param| (param.name.as_str(), &param.ty, "takes"))
        .chain(
            function
                .local_types
                .iter()
                .filter(|local| !function.params.iter().any(|param| param.name == local.name))
                .map(|local| (local.name.as_str(), &local.ty, "holds")),
        );
    // A compiler-made local has no name the source shows. The construct
    // that fills it is reported where it stands, so its type is reported
    // only when nothing else in the function is.
    let mut unnamed = Vec::new();
    for (local, ty, verb) in typed {
        if words_only(ty, classes) {
            continue;
        }
        if local.starts_with(['%', '_']) || local.contains('$') {
            unnamed.push(format!("{verb} a value of type `{ty}`"));
        } else {
            report(
                function.span,
                format!("{verb} `{local}` of type `{ty}`"),
                WORDS_HELP,
            );
        }
    }
    if !words_only(&function.return_type, classes) {
        report(
            function.span,
            format!("returns `{}`", function.return_type),
            WORDS_HELP,
        );
    }
    let is_float = |operand: &Operand| match operand {
        Operand::Float(_) => true,
        Operand::Place(place) | Operand::MovePlace(place) => function
            .local_types
            .iter()
            .map(|local| (&local.name, &local.ty))
            .chain(function.params.iter().map(|param| (&param.name, &param.ty)))
            .any(|(name, ty)| {
                name == place
                    && matches!(ty, Type::Named(name, args)
                        if args.is_empty() && matches!(name.as_str(), "float32" | "float64"))
            }),
        _ => false,
    };
    let mut span = function.span;
    for block in &function.blocks {
        for instruction in &block.instructions {
            let value = match instruction {
                Instruction::SourceLocation { line, column } => {
                    span = crate::diag::Span::new(*line as usize, *column as usize);
                    continue;
                }
                Instruction::Safepoint
                | Instruction::SourceName { .. }
                | Instruction::Retain { .. }
                | Instruction::Release { .. } => continue,
                // A loan of a place that holds words reads and writes the
                // words in place.
                Instruction::Lend { .. }
                | Instruction::EndLend { .. }
                | Instruction::BeginLoan { .. }
                | Instruction::BeginReturnedLoan { .. }
                | Instruction::Reborrow { .. }
                | Instruction::ReadLoan { .. }
                | Instruction::EndLoan { .. }
                | Instruction::ReturnLoan { .. } => continue,
                Instruction::WriteLoan { value, .. } => value,
                Instruction::Assign { value, .. } => value,
                Instruction::Eval { .. } => continue,
                Instruction::PushCleanup { .. } | Instruction::PopCleanup { .. } => {
                    report(span, "uses a `with` block".to_string(), "");
                    continue;
                }
                Instruction::IndexInRange { .. }
                | Instruction::BeginElementLoan { .. }
                | Instruction::ReborrowElement { .. } => {
                    report(span, "reads an element of a collection".to_string(), "");
                    continue;
                }
            };
            let unsupported = match value {
                Rvalue::Use(_)
                | Rvalue::Unary { .. }
                | Rvalue::Cast { .. }
                | Rvalue::UnionTagTest { .. }
                | Rvalue::UnionTakePayload { .. }
                | Rvalue::UnionInject { .. }
                | Rvalue::NoneTest { .. }
                | Rvalue::Construct { .. }
                | Rvalue::Member { .. } => None,
                Rvalue::Binary {
                    op: BinaryOp::Pow, ..
                } => Some(("uses `**`".to_string(), "")),
                Rvalue::Binary {
                    op: op @ (BinaryOp::FloorDiv | BinaryOp::Mod),
                    left,
                    right,
                    ..
                } if is_float(left) || is_float(right) => Some((
                    format!(
                        "uses `{}` on floats",
                        if *op == BinaryOp::Mod { "%" } else { "//" }
                    ),
                    "",
                )),
                Rvalue::Binary { .. } => None,
                Rvalue::Call { callee, args, .. } => {
                    check_call(callee, args, classes, functions)
                }
                Rvalue::ModuleConstant { key, .. } => Some((
                    format!("reads the module constant `{key}`"),
                    "a freestanding program has no startup code to compute a constant: write the value where it is used, or return it from a function",
                )),
                Rvalue::FormatString { .. } => Some(("builds text".to_string(), "")),
                Rvalue::Closure { .. } | Rvalue::CallableAdapt { .. } => {
                    Some(("makes a function value".to_string(), ""))
                }
                Rvalue::TraitObject { .. }
                | Rvalue::TraitObjectHolds { .. }
                | Rvalue::TraitObjectTake { .. } => Some(("uses a trait object".to_string(), "")),
                Rvalue::Try { .. } => Some(("uses `try`".to_string(), "")),
                Rvalue::StartTask { .. } => Some(("starts a task".to_string(), "")),
                Rvalue::VecLiteral { .. } | Rvalue::SetLiteral { .. } | Rvalue::MapLiteral { .. } => {
                    Some(("builds a collection".to_string(), ""))
                }
                Rvalue::TupleLiteral { .. }
                | Rvalue::TupleElement { .. }
                | Rvalue::TupleTakeElement { .. } => Some(("uses a tuple".to_string(), "")),
                Rvalue::EnumVariant { .. } | Rvalue::VariantPayload { .. } => {
                    Some(("uses an enum value".to_string(), ""))
                }
            };
            if let Some((what, help)) = unsupported {
                report(span, what, help);
            }
        }
        match &block.terminator {
            Terminator::Return(_)
            | Terminator::Goto(_)
            | Terminator::Branch { .. }
            | Terminator::AssertFail { .. }
            | Terminator::Unreachable => {}
            Terminator::ForRange { .. } => report(
                span,
                "loops over a value that is not a `range`".to_string(),
                "",
            ),
            Terminator::Match { .. } => report(span, "matches on an enum value".to_string(), ""),
        }
    }
    if found.borrow().is_empty() {
        for what in unnamed {
            report(function.span, what, WORDS_HELP);
        }
    }
    problems.extend(found.into_inner());
}

/// The access a `mem` function makes and the integer it reads or writes.
pub(super) fn memory_operation(name: &str) -> Option<(MemoryAccess, ScalarKind)> {
    let operation = name.strip_prefix("mem::")?;
    if operation == "fence" {
        return Some((MemoryAccess::Fence, ScalarKind::Unit));
    }
    let (operation, width) = operation.rsplit_once("_uint")?;
    let kind = match width {
        "8" => ScalarKind::Uint8,
        "16" => ScalarKind::Uint16,
        "32" => ScalarKind::Uint32,
        "64" => ScalarKind::Uint64,
        _ => return None,
    };
    let access = match operation {
        "read" => MemoryAccess::Read,
        "write" => MemoryAccess::Write,
        "volatile_read" => MemoryAccess::VolatileRead,
        "volatile_write" => MemoryAccess::VolatileWrite,
        "atomic_load" => MemoryAccess::AtomicLoad,
        "atomic_store" => MemoryAccess::AtomicStore,
        "atomic_swap" => MemoryAccess::AtomicSwap,
        "atomic_add" => MemoryAccess::AtomicAdd,
        "atomic_compare_exchange" => MemoryAccess::AtomicCompareExchange,
        _ => return None,
    };
    Some((access, kind))
}

impl FunctionCompiler<'_> {
    /// A call of a `mem` function as inline code: one load, store, or
    /// atomic instruction, in hosted and freestanding builds alike.
    pub(super) fn compile_memory_access(
        &mut self,
        name: &str,
        args: &[MirArg],
    ) -> std::result::Result<Option<ValueRef>, String> {
        let Some((access, kind)) = memory_operation(name) else {
            return Ok(None);
        };
        let mut words = Vec::with_capacity(args.len());
        for argument in args {
            words.push(self.load_operand(&argument.value)?.values[0]);
        }
        let width = match kind {
            ScalarKind::Uint8 => types::I8,
            ScalarKind::Uint16 => types::I16,
            ScalarKind::Uint32 => types::I32,
            _ => types::I64,
        };
        let value = match self.builder.memory_access(access, width, &words) {
            Some(word) => ValueRef {
                values: vec![word],
                ty: DirectType::Scalar(kind),
            },
            None => unit_value(self.builder),
        };
        Ok(Some(value))
    }
}

/// Why a freestanding build cannot compile the call of `callee`, if it
/// cannot.
fn check_call(
    callee: &CallTarget,
    args: &[MirArg],
    classes: &HashMap<String, MirClass>,
    functions: &HashSet<&str>,
) -> Option<(String, &'static str)> {
    match callee {
        CallTarget::Name(name) if functions.contains(name.as_str()) => None,
        CallTarget::Name(name) => {
            Some((format!("calls `{}`", public_direct_function_name(name)), ""))
        }
        CallTarget::Member {
            dispatch: Some(MethodDispatch::Static { function }),
            ..
        }
        | CallTarget::TraitMember {
            dispatch: Some(MethodDispatch::Static { function }),
            ..
        } if functions.contains(function.as_str()) => None,
        CallTarget::Member { field, .. } | CallTarget::TraitMember { field, .. } => {
            Some((format!("calls the method `{field}`"), ""))
        }
        CallTarget::Value(_) => Some(("calls a function value".to_string(), "")),
        CallTarget::Extern(call) => {
            for (param, argument) in call.params.iter().zip(args) {
                let text = matches!(argument.value, Operand::String(_));
                let callback = matches!(argument.value, Operand::Function { .. })
                    && crate::mir_runtime::callback_signature(&param.ty).is_some();
                if !text && !callback && !words_only(&param.ty, classes) {
                    return Some((
                        format!(
                            "passes `{}` of type `{}` to the foreign function `{}`",
                            param.name, param.ty, call.symbol
                        ),
                        "a freestanding foreign call takes scalars, `mut` scalars, `extern \"C\"` classes, literal text, and named functions as callbacks",
                    ));
                }
            }
            if !words_only(&call.return_type, classes) {
                return Some((
                    format!(
                        "takes a `{}` from the foreign function `{}`",
                        call.return_type, call.symbol
                    ),
                    "",
                ));
            }
            None
        }
        CallTarget::Runtime {
            // The emitter refuses the argument kinds it cannot compute in
            // words.
            op:
                RuntimeOp::Function(
                    FunctionOp::Abs | FunctionOp::Min | FunctionOp::Max | FunctionOp::Sqrt,
                ),
            receiver: None,
            ..
        } => None,
        // A `mem` function is one instruction.
        CallTarget::Runtime {
            op: RuntimeOp::Host(builtin),
            ..
        } if memory_operation(builtin.name()).is_some() => None,
        CallTarget::Runtime { op, .. } => Some(match op.name() {
            name if name.starts_with("__") => ("reads or changes a collection".to_string(), ""),
            name => (format!("calls `{name}`"), ""),
        }),
    }
}

/// Refuses a freestanding object that names a symbol it does not define,
/// other than the trap hook, the program's own foreign functions, and the
/// memory routines. The subset check keeps runtime calls out of the
/// program; this keeps one the check missed from reaching a linker.
pub(crate) fn check_object(bytes: &[u8], module: &MirModule) -> std::result::Result<(), String> {
    use object::{Object, ObjectSymbol};
    let file = object::File::parse(bytes)
        .map_err(|error| format!("failed to read the freestanding object: {error}"))?;
    let declared = declared_symbols(module);
    let mut stray = file
        .symbols()
        .filter(|symbol| symbol.is_undefined())
        .filter_map(|symbol| symbol.name().ok())
        .filter(|name| {
            !name.is_empty()
                && *name != TRAP_HOOK
                && !MEMORY_ROUTINES.contains(name)
                && !declared.contains(name)
        })
        .collect::<Vec<_>>();
    stray.sort_unstable();
    stray.dedup();
    if stray.is_empty() {
        return Ok(());
    }
    Err(format!(
        "internal error: the freestanding object needs {}, which no freestanding program provides; please report this with the program",
        stray
            .iter()
            .map(|name| format!("`{name}`"))
            .collect::<Vec<_>>()
            .join(", ")
    ))
}
