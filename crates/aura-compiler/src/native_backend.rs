//! The code generator interface of the native emitter.
//!
//! The emitter translates MIR and lays out values through these two traits
//! only: `ModuleBackend` declares and defines functions and data and writes
//! the object, and `FunctionBackend` builds one function's body. Handles are
//! plain indices that the code generator maps to its own entities, so the
//! emitter names no code generator types. LLVM 22, through inkwell,
//! implements both.

#[path = "native_backend_llvm.rs"]
mod llvm;

#[cfg(test)]
thread_local! {
    static UNOPTIMIZED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs `emit` with LLVM's optimizer off on this thread, so a test reads
/// the object the emitter asked for: every function it defined and every
/// call it built.
#[cfg(test)]
pub(crate) fn without_optimization<R>(emit: impl FnOnce() -> R) -> R {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            UNOPTIMIZED.with(|unoptimized| unoptimized.set(self.0));
        }
    }
    let _restore = Restore(UNOPTIMIZED.with(|unoptimized| unoptimized.replace(true)));
    emit()
}

/// Whether LLVM runs its pass pipeline on this thread's objects.
fn optimizes() -> bool {
    #[cfg(test)]
    return !UNOPTIMIZED.with(std::cell::Cell::get);
    #[cfg(not(test))]
    true
}

/// The module backend for the host, which the emitter writes one object
/// through.
pub(crate) fn host_module_backend() -> Result<Box<dyn ModuleBackend>, String> {
    Ok(Box::new(llvm::LlvmModule::for_host()?))
}

/// An SSA value.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct Val(pub(crate) u32);

/// A basic block, which takes parameters in place of phis.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct Blk(pub(crate) u32);

/// A mutable variable, which the backend turns into SSA values.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct Var(pub(crate) u32);

/// A stack slot of the function being built.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct Slot(pub(crate) u32);

/// An instruction the emitter may rewrite after it builds it.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct Inst(pub(crate) u32);

/// A function the module declares.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct FuncHandle(pub(crate) u32);

/// A data object the module declares.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct DataHandle(pub(crate) u32);

/// A declared function as one function body calls it.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct FuncRef(pub(crate) u32);

/// A declared data object as one function body addresses it.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct DataRef(pub(crate) u32);

/// A signature one function body calls through a pointer.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct SigRef(pub(crate) u32);

/// The type of one value. A comparison yields an `I8` flag.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum ScalarType {
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
}

/// The scalar types by their short names.
pub(crate) mod types {
    use super::ScalarType;

    pub(crate) const I8: ScalarType = ScalarType::I8;
    pub(crate) const I16: ScalarType = ScalarType::I16;
    pub(crate) const I32: ScalarType = ScalarType::I32;
    pub(crate) const I64: ScalarType = ScalarType::I64;
    pub(crate) const F32: ScalarType = ScalarType::F32;
    pub(crate) const F64: ScalarType = ScalarType::F64;
}

/// An integer comparison.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IntCC {
    Equal,
    NotEqual,
    SignedLessThan,
    SignedGreaterThanOrEqual,
    SignedGreaterThan,
    SignedLessThanOrEqual,
    UnsignedLessThan,
    UnsignedGreaterThanOrEqual,
    UnsignedGreaterThan,
    UnsignedLessThanOrEqual,
}

/// An ordered float comparison, except `NotEqual`, which is unordered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FloatCC {
    Equal,
    NotEqual,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
}

/// What a memory access may assume. A trusted access is aligned and cannot
/// trap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MemFlags {
    pub(crate) trusted: bool,
    pub(crate) alias: AliasClass,
}

/// The memory an access touches, when the emitter knows it. Accesses of two
/// different known classes never touch the same bytes, which lets the code
/// generator keep a value in a register across the other class's accesses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AliasClass {
    /// Any memory.
    Any,
    /// The header of a task's frame stack.
    FrameHeader,
    /// An entry of a task's frame stack.
    FrameEntry,
}

impl MemFlags {
    pub(crate) const fn new() -> Self {
        Self {
            trusted: false,
            alias: AliasClass::Any,
        }
    }

    pub(crate) const fn trusted() -> Self {
        Self {
            trusted: true,
            alias: AliasClass::Any,
        }
    }

    /// A trusted access of `alias` memory.
    pub(crate) const fn trusted_in(alias: AliasClass) -> Self {
        Self {
            trusted: true,
            alias,
        }
    }
}

/// A defined function as a debugger names it: its source file, the name
/// a call stack shows, and the line of its `def`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FunctionSource {
    pub(crate) path: String,
    pub(crate) name: String,
    pub(crate) line: u32,
}

/// How a debugger reads the one word of a variable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DebugEncoding {
    Signed,
    Unsigned,
    Float,
    Bool,
    /// The address of a runtime value.
    Value,
}

/// A source variable a debugger shows, held in one variable of the body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DebugVariable {
    pub(crate) name: String,
    /// The Aura type, as the debugger names the variable's type.
    pub(crate) type_name: String,
    pub(crate) encoding: DebugEncoding,
}

/// A source variable of a plain class type a debugger shows, whose fields
/// are each held in one variable of the body. Each field's `name` is the
/// field's.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DebugRecord {
    pub(crate) name: String,
    pub(crate) type_name: String,
    pub(crate) fields: Vec<(Var, DebugVariable)>,
}

/// Why a trap ends the task. Nothing reads the code; it only marks the
/// path in the machine code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TrapCode {
    IntegerOverflow,
    IntegerDivisionByZero,
    User(u8),
}

/// A calling convention. Aura functions call each other through a private
/// convention that may return many values; the runtime and the host call
/// through the C ABI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CallConv {
    Aura,
    C,
}

/// A function signature.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Signature {
    pub(crate) call_conv: CallConv,
    pub(crate) params: Vec<ScalarType>,
    pub(crate) returns: Vec<ScalarType>,
}

impl Signature {
    pub(crate) fn new(call_conv: CallConv) -> Self {
        Self {
            call_conv,
            params: Vec::new(),
            returns: Vec::new(),
        }
    }
}

/// Where a declared symbol is visible.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Linkage {
    /// Defined by another object.
    Import,
    /// Defined here and private to the object.
    Local,
    /// Defined here and visible to the linker.
    Export,
}

/// The address a data object holds at one offset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DataAddress {
    Function(FuncHandle),
    Data(DataHandle),
}

/// The contents of a data object: its bytes, its alignment, and the
/// addresses written into it, in order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DataDescription {
    pub(crate) bytes: Vec<u8>,
    pub(crate) align: Option<u64>,
    pub(crate) addresses: Vec<(u32, DataAddress)>,
}

impl DataDescription {
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            ..Self::default()
        }
    }

    pub(crate) fn write_function_addr(&mut self, offset: u32, function: FuncHandle) {
        self.addresses
            .push((offset, DataAddress::Function(function)));
    }

    pub(crate) fn write_data_addr(&mut self, offset: u32, data: DataHandle) {
        self.addresses.push((offset, DataAddress::Data(data)));
    }
}

/// A call instruction and its results.
#[derive(Clone, Debug)]
pub(crate) struct Call {
    pub(crate) inst: Inst,
    pub(crate) results: Vec<Val>,
}

/// Why a function definition failed: its body's own error, or the code
/// generator's.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DefineError {
    Body(String),
    Backend(String),
}

/// Builds the body of one function.
pub(crate) type FunctionBody<'b> =
    Box<dyn FnOnce(&mut dyn FunctionBackend) -> Result<(), String> + 'b>;

/// Declares and defines the functions and data of one object.
pub(crate) trait ModuleBackend {
    fn declare_function(
        &mut self,
        name: &str,
        linkage: Linkage,
        signature: &Signature,
    ) -> Result<FuncHandle, String>;

    fn declare_data(
        &mut self,
        name: &str,
        linkage: Linkage,
        writable: bool,
    ) -> Result<DataHandle, String>;

    fn define_data(&mut self, data: DataHandle, contents: DataDescription) -> Result<(), String>;
    /// Makes the object a debug build: unoptimized, with DWARF for the
    /// functions `describe_function` names.
    fn enable_debug_info(&mut self);
    /// Gives `function` a debugger's name and source position. Only a
    /// debug build reads it.
    fn describe_function(&mut self, function: FuncHandle, source: FunctionSource);

    /// Declares that `function`, an import, touches no memory its callers
    /// see and returns the same value on every call from one task, so the
    /// code generator may merge its calls.
    fn mark_task_invariant(&mut self, function: FuncHandle);

    /// Asks the code generator to inline `function`, a small local
    /// function, into every caller.
    fn mark_inline(&mut self, function: FuncHandle);

    /// Defines `function`, whose body `body` builds. A body that fails
    /// leaves the function undefined.
    fn define_function(
        &mut self,
        function: FuncHandle,
        signature: &Signature,
        body: FunctionBody<'_>,
    ) -> Result<(), DefineError>;

    /// The object file's bytes, as one object.
    fn finish(self: Box<Self>) -> Result<Vec<u8>, String>;

    /// The object files' bytes. A large module splits into several
    /// objects, which the code generator writes in parallel; a small one
    /// writes the one object `finish` would.
    fn finish_units(self: Box<Self>) -> Result<Vec<Vec<u8>>, String>;
}

/// Builds one function body: blocks, variables, stack slots, and
/// instructions.
pub(crate) trait FunctionBackend {
    /// The module the function belongs to, for declarations made while the
    /// body is built.
    fn module(&mut self) -> &mut dyn ModuleBackend;

    fn create_block(&mut self) -> Blk;
    fn switch_to_block(&mut self, block: Blk);
    /// Declares that every branch to `block` is built.
    fn seal_block(&mut self, block: Blk);
    fn seal_all_blocks(&mut self);
    /// Marks `block` as rarely run, so the code generator lays it out and
    /// predicts branches to it as cold.
    fn set_cold_block(&mut self, block: Blk);
    /// The source position of the instructions built from here on, for a
    /// debug build's line table.
    fn set_source_location(&mut self, line: u32, column: u32);
    /// Shows `var` in a debugger as `variable`, in a debug build.
    fn describe_var(&mut self, var: Var, variable: DebugVariable);
    /// Shows the variables of `record`'s fields in a debugger as one
    /// record, in a debug build.
    fn describe_record(&mut self, record: DebugRecord);
    fn current_block(&self) -> Option<Blk>;
    fn block_params(&self, block: Blk) -> Vec<Val>;
    fn append_block_param(&mut self, block: Blk, ty: ScalarType) -> Val;
    fn append_block_params_for_function_params(&mut self, block: Blk);

    fn declare_var(&mut self, var: Var, ty: ScalarType);
    fn def_var(&mut self, var: Var, value: Val);
    fn use_var(&mut self, var: Var) -> Val;

    /// A stack slot of `size` bytes aligned to `1 << align_shift`.
    fn create_sized_stack_slot(&mut self, size: u32, align_shift: u8) -> Slot;
    fn stack_addr(&mut self, ty: ScalarType, slot: Slot, offset: i32) -> Val;

    fn import_function(&mut self, function: FuncHandle) -> FuncRef;
    fn import_data(&mut self, data: DataHandle) -> DataRef;
    fn import_signature(&mut self, signature: Signature) -> SigRef;
    fn func_addr(&mut self, ty: ScalarType, function: FuncRef) -> Val;
    fn symbol_value(&mut self, ty: ScalarType, data: DataRef) -> Val;

    fn call(&mut self, callee: FuncRef, args: &[Val]) -> Call;
    fn call_indirect(&mut self, signature: SigRef, callee: Val, args: &[Val]) -> Call;
    fn jump(&mut self, block: Blk, args: &[Val]);
    fn brif(
        &mut self,
        condition: Val,
        then_block: Blk,
        then_args: &[Val],
        else_block: Blk,
        else_args: &[Val],
    );
    fn return_(&mut self, values: &[Val]);
    fn trap(&mut self, code: TrapCode);

    fn value_type(&self, value: Val) -> ScalarType;
    /// The constant `value` holds when an `iconst` defines it.
    fn integer_constant(&self, value: Val) -> Option<i64>;
    /// Whether `inst` is in the current block and each call after it there
    /// calls one of `allowed`.
    fn only_calls_since(&self, inst: Inst, allowed: &[FuncRef]) -> bool;
    /// Turns the call `inst` into a call of `callee`, keeping its results.
    fn replace_with_call(&mut self, inst: Inst, callee: FuncRef, args: &[Val]);
    /// Removes `inst`, which defines no value anything uses.
    fn replace_with_nop(&mut self, inst: Inst);
    /// Turns `inst` into a copy of `value` that keeps its one result.
    fn replace_with_copy(&mut self, inst: Inst, value: Val);
    /// Checks the function, with its text in the error.
    fn verify(&self) -> Result<(), String>;
    /// The number of instructions built so far.
    #[cfg(test)]
    fn instruction_count(&self) -> usize;

    fn iconst(&mut self, ty: ScalarType, value: i64) -> Val;
    fn f64const(&mut self, value: f64) -> Val;
    fn iadd(&mut self, left: Val, right: Val) -> Val;
    fn iadd_imm(&mut self, value: Val, imm: i64) -> Val;
    fn isub(&mut self, left: Val, right: Val) -> Val;
    fn imul(&mut self, left: Val, right: Val) -> Val;
    fn imul_imm(&mut self, value: Val, imm: i64) -> Val;
    fn ineg(&mut self, value: Val) -> Val;
    fn sdiv(&mut self, left: Val, right: Val) -> Val;
    fn udiv(&mut self, left: Val, right: Val) -> Val;
    fn srem(&mut self, left: Val, right: Val) -> Val;
    fn urem(&mut self, left: Val, right: Val) -> Val;
    fn band(&mut self, left: Val, right: Val) -> Val;
    fn band_imm(&mut self, value: Val, imm: i64) -> Val;
    fn bor(&mut self, left: Val, right: Val) -> Val;
    fn bxor(&mut self, left: Val, right: Val) -> Val;
    fn bxor_imm(&mut self, value: Val, imm: i64) -> Val;
    /// `value` shifted left, logically right, or arithmetically right by
    /// `count`, which must be below the width of `value`'s type.
    fn ishl(&mut self, value: Val, count: Val) -> Val;
    fn ushr(&mut self, value: Val, count: Val) -> Val;
    fn sshr(&mut self, value: Val, count: Val) -> Val;
    fn icmp(&mut self, cond: IntCC, left: Val, right: Val) -> Val;
    fn icmp_imm(&mut self, cond: IntCC, value: Val, imm: i64) -> Val;
    fn select(&mut self, condition: Val, if_true: Val, if_false: Val) -> Val;
    fn uextend(&mut self, ty: ScalarType, value: Val) -> Val;
    fn ireduce(&mut self, ty: ScalarType, value: Val) -> Val;
    fn bitcast(&mut self, ty: ScalarType, value: Val) -> Val;
    /// The sum, difference, or product of two integers, with a flag set
    /// when the operation overflows.
    fn sadd_overflow(&mut self, left: Val, right: Val) -> (Val, Val);
    fn uadd_overflow(&mut self, left: Val, right: Val) -> (Val, Val);
    fn ssub_overflow(&mut self, left: Val, right: Val) -> (Val, Val);
    fn usub_overflow(&mut self, left: Val, right: Val) -> (Val, Val);
    fn smul_overflow(&mut self, left: Val, right: Val) -> (Val, Val);
    fn umul_overflow(&mut self, left: Val, right: Val) -> (Val, Val);

    fn load(&mut self, ty: ScalarType, flags: MemFlags, address: Val, offset: i32) -> Val;
    fn uload8(&mut self, ty: ScalarType, flags: MemFlags, address: Val, offset: i32) -> Val;
    /// A narrow integer load, sign- or zero-extended to 64 bits.
    fn sload8(&mut self, flags: MemFlags, address: Val, offset: i32) -> Val;
    fn uload16(&mut self, flags: MemFlags, address: Val, offset: i32) -> Val;
    fn sload16(&mut self, flags: MemFlags, address: Val, offset: i32) -> Val;
    fn uload32(&mut self, flags: MemFlags, address: Val, offset: i32) -> Val;
    fn sload32(&mut self, flags: MemFlags, address: Val, offset: i32) -> Val;
    fn store(&mut self, flags: MemFlags, value: Val, address: Val, offset: i32);

    fn fadd(&mut self, left: Val, right: Val) -> Val;
    fn fsub(&mut self, left: Val, right: Val) -> Val;
    fn fmul(&mut self, left: Val, right: Val) -> Val;
    fn fdiv(&mut self, left: Val, right: Val) -> Val;
    fn fneg(&mut self, value: Val) -> Val;
    fn fcmp(&mut self, cond: FloatCC, left: Val, right: Val) -> Val;
    fn fpromote(&mut self, ty: ScalarType, value: Val) -> Val;
    fn fdemote(&mut self, ty: ScalarType, value: Val) -> Val;
    fn fcvt_from_uint(&mut self, ty: ScalarType, value: Val) -> Val;
    fn fcvt_from_sint(&mut self, ty: ScalarType, value: Val) -> Val;
    /// A float truncated toward zero to a signed or unsigned integer of
    /// type `ty`. The truncated value must fit the type.
    fn fcvt_to_sint(&mut self, ty: ScalarType, value: Val) -> Val;
    fn fcvt_to_uint(&mut self, ty: ScalarType, value: Val) -> Val;
}
