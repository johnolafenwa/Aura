//! The code generator interface of the native emitter.
//!
//! The emitter translates MIR and lays out values through these two traits
//! only: `ModuleBackend` declares and defines functions and data and writes
//! the object, and `FunctionBackend` builds one function's body. Handles are
//! plain indices that each code generator maps to its own entities, so the
//! emitter names no code generator types. Cranelift is the default
//! implementation, and an `aura` built with the `llvm` feature also has an
//! LLVM one, which `AURA_NATIVE_CODEGEN=llvm` selects.

use std::ffi::OsStr;

#[path = "native_backend_cranelift.rs"]
mod cranelift;
#[cfg(feature = "llvm")]
#[path = "native_backend_llvm.rs"]
mod llvm;

/// The environment variable that selects the code generator.
pub(crate) const NATIVE_CODEGEN_ENV: &str = "AURA_NATIVE_CODEGEN";

/// A code generator the native emitter can write objects with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodeGenerator {
    Cranelift,
    #[cfg(feature = "llvm")]
    Llvm,
}

impl CodeGenerator {
    /// The name `AURA_NATIVE_CODEGEN` selects it by, which the native cache
    /// key also carries.
    pub fn name(self) -> &'static str {
        match self {
            CodeGenerator::Cranelift => "cranelift",
            #[cfg(feature = "llvm")]
            CodeGenerator::Llvm => "llvm",
        }
    }
}

/// The code generator `AURA_NATIVE_CODEGEN` selects. Unset, empty, or
/// `cranelift` selects Cranelift.
pub fn selected_code_generator() -> Result<CodeGenerator, String> {
    code_generator_for(std::env::var_os(NATIVE_CODEGEN_ENV).as_deref())
}

/// The code generator one `AURA_NATIVE_CODEGEN` setting selects.
fn code_generator_for(setting: Option<&OsStr>) -> Result<CodeGenerator, String> {
    let setting = setting.unwrap_or_default();
    if setting.is_empty() || setting == "cranelift" {
        return Ok(CodeGenerator::Cranelift);
    }
    if setting == "llvm" {
        #[cfg(feature = "llvm")]
        return Ok(CodeGenerator::Llvm);
        #[cfg(not(feature = "llvm"))]
        return Err(format!(
            "{NATIVE_CODEGEN_ENV}=llvm needs LLVM, but this aura was built without the `llvm` feature"
        ));
    }
    Err(format!(
        "{NATIVE_CODEGEN_ENV} must be `cranelift` or `llvm`, not `{}`",
        setting.to_string_lossy()
    ))
}

/// The module backend for the host, which the emitter writes one object
/// through.
pub(crate) fn host_module_backend() -> Result<Box<dyn ModuleBackend>, String> {
    match selected_code_generator()? {
        CodeGenerator::Cranelift => Ok(Box::new(cranelift::CraneliftModule::for_host()?)),
        #[cfg(feature = "llvm")]
        CodeGenerator::Llvm => Ok(Box::new(llvm::LlvmModule::for_host()?)),
    }
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
    I32,
    I64,
    F32,
    F64,
}

/// The scalar types by their short names.
pub(crate) mod types {
    use super::ScalarType;

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
}

impl MemFlags {
    pub(crate) const fn new() -> Self {
        Self { trusted: false }
    }

    pub(crate) const fn trusted() -> Self {
        Self { trusted: true }
    }
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

    /// Defines `function`, whose body `body` builds. A body that fails
    /// leaves the function undefined.
    fn define_function(
        &mut self,
        function: FuncHandle,
        signature: &Signature,
        body: FunctionBody<'_>,
    ) -> Result<(), DefineError>;

    /// The object file's bytes.
    fn finish(self: Box<Self>) -> Result<Vec<u8>, String>;
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
}

#[cfg(test)]
#[path = "native_backend_tests.rs"]
mod tests;
