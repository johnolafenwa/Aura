//! The LLVM implementation of the native backend interface.
//!
//! A function body is recorded as the emitter builds it, in a small
//! instruction list of this file's own, so the emitter's inspections and
//! rewrites see the same instructions they built. `finish` then lowers the
//! whole object to LLVM IR in one context, runs LLVM's `O3` pipeline, and
//! writes the object. `finish_units` splits a large object into codegen
//! units first, and threads lower, optimize, and write one unit each, as
//! `native_backend_llvm_units.rs` describes.
//!
//! Lowering keeps every value a 64-bit integer, as the emitter does:
//! loads and stores turn the address into a pointer, and addresses turn
//! back into integers. Blocks become basic blocks, block parameters become
//! phis, and variables and stack slots become allocas in an entry block
//! that `mem2reg` and SROA clean up. A function in the private Aura
//! convention returns many values as one struct, which every Aura caller and
//! callee in the object shares; the C convention uses the C ABI.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

use inkwell::attributes::{Attribute, AttributeLoc};
use inkwell::basic_block::BasicBlock;
use inkwell::builder::{Builder, BuilderError};
use inkwell::context::Context;
use inkwell::debug_info::{
    AsDIScope, DICompileUnit, DIFile, DIFlags, DIFlagsConstants, DILocalVariable, DILocation,
    DIScope, DISubprogram, DIType, DWARFEmissionKind, DWARFSourceLanguage, DebugInfoBuilder,
};
use inkwell::intrinsics::Intrinsic;
use inkwell::module::{FlagBehavior, Linkage as LlvmLinkage, Module};
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine, TargetTriple,
};
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType, IntType};
use inkwell::values::{
    AsValueRef, BasicMetadataValueEnum, BasicValue, BasicValueEnum, FunctionValue, GlobalValue,
    InstructionValue, IntValue, PhiValue, PointerValue,
};
use inkwell::{AddressSpace, FloatPredicate, GlobalVisibility, IntPredicate, OptimizationLevel};

use units::{Partition, Placement};

use super::{
    AliasClass, Blk, Call, CallConv, DataAddress, DataDescription, DataHandle, DataRef,
    DebugEncoding, DebugRecord, DebugVariable, DefineError, FloatCC, FuncHandle, FuncRef,
    FunctionBackend, FunctionBody, FunctionSource, Inst, IntCC, Linkage, MemFlags, ModuleBackend,
    ScalarType, SigRef, Signature, Slot, TrapCode, Val, Var,
};

/// The pass pipeline every object runs. Step 9c measured `O1`, `O2`, and
/// `O3`: `O3` alone sped up a benchmark, by unswitching a list loop on its
/// layout check, and it costs under 1% more compile work than `O2`.
const PASS_PIPELINE: &str = "default<O3>";

/// The LLVM `uwtable` kind for asynchronous unwind tables. Task failures
/// unwind through generated frames, so every function needs one.
const UWTABLE_ASYNC: u64 = 2;

/// How a function whose frame is larger than a page touches the stack
/// before it uses it: one page at a time, inline, as rustc asks for. A task
/// stack has a single guard page below it, and a frame that moved the stack
/// pointer past that page in one step could write into whatever lies below
/// the stack instead of faulting.
const PROBE_STACK: (&str, &str) = ("probe-stack", "inline-asm");

/// The LLVM `memory` attribute value that allows no memory access.
const MEMORY_NONE: u64 = 0;

/// The `branch_weights` of a branch to a cold block, against the other
/// edge, as Clang weighs `__builtin_expect`.
const COLD_BRANCH_WEIGHTS: (u64, u64) = (1, 2000);

/// The stack of a thread that writes one unit, as large as a main
/// thread's.
const UNIT_THREAD_STACK: usize = 8 << 20;

/// One object built with LLVM for the host.
pub(super) struct LlvmModule {
    functions: Vec<FunctionDecl>,
    data: Vec<DataDecl>,
    names: HashMap<String, Symbol>,
    /// Whether the object is a debug build, written without the pass
    /// pipeline and with DWARF for the functions that have a source.
    debug_info: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Symbol {
    Function(u32),
    Data(u32),
}

struct FunctionDecl {
    name: String,
    linkage: Linkage,
    signature: Signature,
    body: Option<FunctionIr>,
    task_invariant: bool,
    inline: bool,
    source: Option<FunctionSource>,
}

struct DataDecl {
    name: String,
    linkage: Linkage,
    writable: bool,
    contents: Option<DataDescription>,
}

impl LlvmModule {
    pub(super) fn for_host() -> Result<Self, String> {
        // Fail at selection, not at the end of a build, when the host has no
        // LLVM target.
        host_target_machine()?;
        Ok(Self {
            functions: Vec::new(),
            data: Vec::new(),
            names: HashMap::new(),
            debug_info: false,
        })
    }
}

impl LlvmModule {
    /// The object as one verified LLVM module, before any pass runs.
    #[cfg(test)]
    fn lower<'ctx>(
        &self,
        context: &'ctx Context,
        machine: &TargetMachine,
    ) -> Result<Module<'ctx>, String> {
        self.lower_unit(context, machine, &Partition::whole(self), 0)
    }

    /// One unit of `partition` as a verified LLVM module, before any pass
    /// runs.
    fn lower_unit<'ctx>(
        &self,
        context: &'ctx Context,
        machine: &TargetMachine,
        partition: &Partition,
        unit: usize,
    ) -> Result<Module<'ctx>, String> {
        let name = match partition.units() {
            1 => "aura_direct".to_string(),
            _ => format!("aura_direct.{unit}"),
        };
        let module = context.create_module(&name);
        module.set_source_file_name(&name);
        module.set_triple(&machine.get_triple());
        module.set_data_layout(&machine.get_target_data().get_data_layout());
        let debug = self
            .debug_info
            .then(|| DebugInfo::new(context, &module, self));
        let lowerer = ObjectLowerer::new(context, &module, self, partition, unit, debug)?;
        lowerer.lower_bodies(self)?;
        if let Some(debug) = &lowerer.debug {
            debug.builder.finalize();
        }
        if let Err(error) = module.verify() {
            return Err(format!("LLVM rejected the generated module: {error}"));
        }
        Ok(module)
    }
}

/// The more visible of two linkages, which a repeated declaration keeps.
fn merge_linkage(previous: Linkage, declared: Linkage) -> Linkage {
    fn rank(linkage: Linkage) -> u8 {
        match linkage {
            Linkage::Import => 0,
            Linkage::Local => 1,
            Linkage::Export => 2,
        }
    }
    if rank(declared) > rank(previous) {
        declared
    } else {
        previous
    }
}

impl ModuleBackend for LlvmModule {
    fn declare_function(
        &mut self,
        name: &str,
        linkage: Linkage,
        signature: &Signature,
    ) -> Result<FuncHandle, String> {
        match self.names.get(name) {
            Some(Symbol::Function(id)) => {
                let decl = &mut self.functions[*id as usize];
                if decl.signature != *signature {
                    return Err(format!(
                        "Function {name} signature {signature:?} is incompatible with previous declaration {:?}",
                        decl.signature
                    ));
                }
                decl.linkage = merge_linkage(decl.linkage, linkage);
                Ok(FuncHandle(*id))
            }
            Some(Symbol::Data(_)) => Err(format!("Incompatible declaration of identifier: {name}")),
            None => {
                let id = self.functions.len() as u32;
                self.functions.push(FunctionDecl {
                    name: name.to_string(),
                    linkage,
                    signature: signature.clone(),
                    body: None,
                    task_invariant: false,
                    inline: false,
                    source: None,
                });
                self.names.insert(name.to_string(), Symbol::Function(id));
                Ok(FuncHandle(id))
            }
        }
    }

    fn declare_data(
        &mut self,
        name: &str,
        linkage: Linkage,
        writable: bool,
    ) -> Result<DataHandle, String> {
        match self.names.get(name) {
            Some(Symbol::Data(id)) => {
                let decl = &mut self.data[*id as usize];
                if decl.writable != writable {
                    return Err(format!("Incompatible declaration of identifier: {name}"));
                }
                decl.linkage = merge_linkage(decl.linkage, linkage);
                Ok(DataHandle(*id))
            }
            Some(Symbol::Function(_)) => {
                Err(format!("Incompatible declaration of identifier: {name}"))
            }
            None => {
                let id = self.data.len() as u32;
                self.data.push(DataDecl {
                    name: name.to_string(),
                    linkage,
                    writable,
                    contents: None,
                });
                self.names.insert(name.to_string(), Symbol::Data(id));
                Ok(DataHandle(id))
            }
        }
    }

    fn define_data(&mut self, data: DataHandle, contents: DataDescription) -> Result<(), String> {
        let decl = &mut self.data[data.0 as usize];
        if decl.linkage == Linkage::Import {
            return Err(format!(
                "Invalid to define identifier declared as an import: {}",
                decl.name
            ));
        }
        if decl.contents.is_some() {
            return Err(format!("Duplicate definition of identifier: {}", decl.name));
        }
        decl.contents = Some(contents);
        Ok(())
    }

    fn mark_task_invariant(&mut self, function: FuncHandle) {
        self.functions[function.0 as usize].task_invariant = true;
    }

    fn mark_inline(&mut self, function: FuncHandle) {
        self.functions[function.0 as usize].inline = true;
    }

    fn enable_debug_info(&mut self) {
        self.debug_info = true;
    }

    fn describe_function(&mut self, function: FuncHandle, source: FunctionSource) {
        self.functions[function.0 as usize].source = Some(source);
    }

    fn define_function(
        &mut self,
        function: FuncHandle,
        signature: &Signature,
        body: FunctionBody<'_>,
    ) -> Result<(), DefineError> {
        let mut builder = LlvmFunction {
            module: self,
            ir: FunctionIr::new(signature.clone()),
        };
        // The body's own error comes first. The definition is checked only
        // once the body is built.
        if let Err(error) = body(&mut builder) {
            return Err(DefineError::Body(error));
        }
        let ir = builder.ir;
        let decl = &self.functions[function.0 as usize];
        if decl.linkage == Linkage::Import {
            return Err(DefineError::Backend(format!(
                "Invalid to define identifier declared as an import: {}",
                decl.name
            )));
        }
        if decl.body.is_some() {
            return Err(DefineError::Backend(format!(
                "Duplicate definition of identifier: {}",
                decl.name
            )));
        }
        if decl.signature != *signature {
            return Err(DefineError::Backend(format!(
                "Function {} signature {signature:?} is incompatible with previous declaration {:?}",
                decl.name, decl.signature
            )));
        }
        self.functions[function.0 as usize].body = Some(ir);
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<Vec<u8>, String> {
        self.unit_object(
            &self.target_machine()?,
            &Partition::whole(&self),
            0,
            super::optimizes() && !self.debug_info,
        )
    }

    fn finish_units(self: Box<Self>) -> Result<Vec<Vec<u8>>, String> {
        // A debug build is one unit: it runs no passes, so it gains little
        // from threads, and one compile unit keeps its DWARF whole.
        if self.debug_info {
            return self.finish().map(|object| vec![object]);
        }
        let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
        self.unit_objects(&Partition::split(&self), threads)
    }
}

impl LlvmModule {
    /// The host machine the object is written for. A debug build's machine
    /// optimizes nothing, so every variable keeps its stack slot.
    fn target_machine(&self) -> Result<TargetMachine, String> {
        if self.debug_info {
            host_target_machine_at(OptimizationLevel::None)
        } else {
            host_target_machine()
        }
    }

    /// The object for `machine`: lowered, optimized, and written.
    #[cfg(test)]
    fn object_for(&self, machine: &TargetMachine) -> Result<Vec<u8>, String> {
        self.unit_object(machine, &Partition::whole(self), 0, super::optimizes())
    }

    /// The objects of every unit of `partition`, in unit order, written
    /// by up to `threads` threads. Threads take the units largest first,
    /// and each writes its unit in its own LLVM context, so the bytes do
    /// not depend on which thread wrote them or how many ran.
    fn unit_objects(&self, partition: &Partition, threads: usize) -> Result<Vec<Vec<u8>>, String> {
        // The optimizer setting is this thread's, as a test sets it.
        let optimize = super::optimizes();
        let order = partition.units_by_weight();
        let next = AtomicUsize::new(0);
        let objects = (0..partition.units())
            .map(|_| OnceLock::new())
            .collect::<Vec<OnceLock<Result<Vec<u8>, String>>>>();
        let work = || {
            while let Some(&unit) = order.get(next.fetch_add(1, Ordering::Relaxed)) {
                let object = host_target_machine()
                    .and_then(|machine| self.unit_object(&machine, partition, unit, optimize));
                let _ = objects[unit].set(object);
            }
        };
        std::thread::scope(|scope| {
            // This thread works too, so the units are written even when no
            // other thread starts.
            for _ in 1..threads.min(partition.units()) {
                let _ = std::thread::Builder::new()
                    .name("aura-llvm-unit".to_string())
                    .stack_size(UNIT_THREAD_STACK)
                    .spawn_scoped(scope, work);
            }
            work();
        });
        objects
            .into_iter()
            .map(|object| object.into_inner().expect("every unit is written"))
            .collect()
    }

    /// One unit's object for `machine`: lowered, optimized unless
    /// `optimize` is off, and written.
    fn unit_object(
        &self,
        machine: &TargetMachine,
        partition: &Partition,
        unit: usize,
        optimize: bool,
    ) -> Result<Vec<u8>, String> {
        let context = Context::create();
        let module = self.lower_unit(&context, machine, partition, unit)?;
        if optimize {
            if let Err(error) =
                module.run_passes(PASS_PIPELINE, machine, PassBuilderOptions::create())
            {
                return Err(format!("LLVM failed to optimize the module: {error}"));
            }
        }
        place_functions_in_own_sections(&module, &machine.get_triple());
        match machine.write_to_memory_buffer(&module, FileType::Object) {
            Ok(buffer) => Ok(buffer.as_slice().to_vec()),
            Err(error) => Err(format!("LLVM failed to write the object: {error}")),
        }
    }
}

/// Gives every defined function its own ELF section, so `--gc-sections`
/// drops the ones nothing calls. It runs after the passes, so it also covers any function they
/// create. Mach-O needs nothing: LLVM marks its objects
/// `MH_SUBSECTIONS_VIA_SYMBOLS`, and `-dead_strip` works per symbol.
fn place_functions_in_own_sections(module: &Module<'_>, triple: &TargetTriple) {
    if triple.as_str().to_string_lossy().contains("apple") {
        return;
    }
    let mut function = module.get_first_function();
    while let Some(current) = function {
        if current.count_basic_blocks() > 0 {
            let section = format!(".text.{}", current.get_name().to_string_lossy());
            set_section(current, &section);
        }
        function = current.get_next_function();
    }
}

/// Names `function`'s section exactly. inkwell's `set_section` prefixes a
/// comma on a macOS host, for Mach-O's `segment,section` form, which turns
/// an ELF name into `,.text.<symbol>` when a macOS host writes ELF.
fn set_section(function: FunctionValue<'_>, section: &str) {
    let name = std::ffi::CString::new(section).expect("symbol names hold no NUL byte");
    // SAFETY: `function` is a live value of a module this thread owns, and
    // LLVM copies the name before the call returns.
    unsafe { inkwell::llvm_sys::core::LLVMSetSection(function.as_value_ref(), name.as_ptr()) }
}

/// The LLVM triple of the host. On macOS it names the deployment target
/// rustc builds the runtime for, so the linker sees one minimum version.
fn host_triple() -> TargetTriple {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("x86_64", "linux") => TargetTriple::create("x86_64-unknown-linux-gnu"),
        ("aarch64", "linux") => TargetTriple::create("aarch64-unknown-linux-gnu"),
        ("aarch64", "macos") => TargetTriple::create("arm64-apple-macosx11.0.0"),
        ("x86_64", "macos") => TargetTriple::create("x86_64-apple-macosx10.12.0"),
        _ => TargetMachine::get_default_triple(),
    }
}

fn host_target_machine() -> Result<TargetMachine, String> {
    host_target_machine_at(OptimizationLevel::Default)
}

fn host_target_machine_at(level: OptimizationLevel) -> Result<TargetMachine, String> {
    static INITIALIZED: OnceLock<Result<(), String>> = OnceLock::new();
    INITIALIZED
        .get_or_init(|| Target::initialize_native(&InitializationConfig::default()))
        .clone()
        .map_err(|error| format!("failed to initialize the LLVM host target: {error}"))?;
    target_machine_at(&host_triple(), level)
}

/// A machine for `triple`, whose target must already be initialized.
#[cfg(test)]
fn target_machine(triple: &TargetTriple) -> Result<TargetMachine, String> {
    target_machine_at(triple, OptimizationLevel::Default)
}

/// A machine for `triple` that optimizes at `level`, whose target must
/// already be initialized.
fn target_machine_at(
    triple: &TargetTriple,
    level: OptimizationLevel,
) -> Result<TargetMachine, String> {
    let target = Target::from_triple(triple)
        .map_err(|error| format!("LLVM has no target for {triple}: {error}"))?;
    // A generic CPU keeps objects the same on every host of one target.
    target
        .create_target_machine(
            triple,
            "generic",
            "",
            level,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| format!("LLVM could not create a target machine for {triple}"))
}

/// What a recorded instruction does. Result types live with its results.
#[derive(Clone, Debug)]
enum Op {
    Nop,
    Copy(Val),
    Iconst(i64),
    Fconst(f64),
    Unary(UnaryOp, Val),
    Binary(BinaryOp, Val, Val),
    BinaryImm(BinaryOp, Val, i64),
    Icmp(IntCC, Val, Val),
    IcmpImm(IntCC, Val, i64),
    Fcmp(FloatCC, Val, Val),
    Select(Val, Val, Val),
    Overflow(OverflowOp, Val, Val),
    Load(LoadKind, MemFlags, Val, i32),
    Store(MemFlags, Val, Val, i32),
    StackAddr(Slot, i32),
    FuncAddr(FuncRef),
    SymbolValue(DataRef),
    Call(FuncRef, Vec<Val>),
    CallIndirect(SigRef, Val, Vec<Val>),
    UseVar(Var),
    DefVar(Var, Val),
    Jump(Blk, Vec<Val>),
    Brif(Val, Blk, Vec<Val>, Blk, Vec<Val>),
    Return(Vec<Val>),
    Trap,
}

impl Op {
    fn is_terminator(&self) -> bool {
        matches!(
            self,
            Op::Jump(..) | Op::Brif(..) | Op::Return(..) | Op::Trap
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UnaryOp {
    Ineg,
    Fneg,
    Uextend,
    Ireduce,
    Bitcast,
    Fpromote,
    Fdemote,
    FcvtFromUint,
    FcvtFromSint,
    FcvtToUint,
    FcvtToSint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BinaryOp {
    Iadd,
    Isub,
    Imul,
    Sdiv,
    Udiv,
    Srem,
    Urem,
    Band,
    Bor,
    Bxor,
    Ishl,
    Ushr,
    Sshr,
    Fadd,
    Fsub,
    Fmul,
    Fdiv,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OverflowOp {
    Sadd,
    Uadd,
    Ssub,
    Usub,
    Smul,
    Umul,
}

impl OverflowOp {
    fn intrinsic(self) -> &'static str {
        match self {
            OverflowOp::Sadd => "llvm.sadd.with.overflow",
            OverflowOp::Uadd => "llvm.uadd.with.overflow",
            OverflowOp::Ssub => "llvm.ssub.with.overflow",
            OverflowOp::Usub => "llvm.usub.with.overflow",
            OverflowOp::Smul => "llvm.smul.with.overflow",
            OverflowOp::Umul => "llvm.umul.with.overflow",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LoadKind {
    Plain,
    Uload8,
    Sload8,
    Uload16,
    Sload16,
    Uload32,
    Sload32,
}

#[derive(Clone, Debug)]
struct InstIr {
    op: Op,
    results: Vec<Val>,
    block: u32,
    /// The source line and column the instruction was built for.
    location: Option<(u32, u32)>,
}

#[derive(Clone, Debug, Default)]
struct BlockIr {
    params: Vec<Val>,
    insts: Vec<u32>,
    sealed: bool,
    cold: bool,
    in_layout: bool,
    predecessors: Vec<u32>,
    /// The value each variable last got in this block.
    definitions: HashMap<u32, Val>,
}

/// One function body as the emitter built it.
#[derive(Clone, Debug)]
struct FunctionIr {
    signature: Signature,
    values: Vec<ScalarType>,
    /// The instruction that defines each value, unless a block parameter.
    value_defs: Vec<Option<u32>>,
    blocks: Vec<BlockIr>,
    layout: Vec<u32>,
    insts: Vec<InstIr>,
    vars: BTreeMap<u32, ScalarType>,
    slots: Vec<(u32, u8)>,
    func_refs: Vec<FuncHandle>,
    data_refs: Vec<DataHandle>,
    sig_refs: Vec<Signature>,
    current: Option<u32>,
    params_block: Option<u32>,
    /// The source position of the instructions built from here on.
    location: Option<(u32, u32)>,
    /// The variables a debugger shows, by variable.
    debug_vars: BTreeMap<u32, DebugVariable>,
    /// The plain class variables a debugger shows.
    debug_records: Vec<DebugRecord>,
}

/// Where a variable's value comes from at a use.
enum VarSource {
    Value(Val),
    Zero,
    Memory,
}

impl FunctionIr {
    fn new(signature: Signature) -> Self {
        Self {
            signature,
            values: Vec::new(),
            value_defs: Vec::new(),
            blocks: Vec::new(),
            layout: Vec::new(),
            insts: Vec::new(),
            vars: BTreeMap::new(),
            slots: Vec::new(),
            func_refs: Vec::new(),
            data_refs: Vec::new(),
            sig_refs: Vec::new(),
            current: None,
            params_block: None,
            location: None,
            debug_vars: BTreeMap::new(),
            debug_records: Vec::new(),
        }
    }

    fn new_value(&mut self, ty: ScalarType) -> Val {
        self.values.push(ty);
        self.value_defs.push(None);
        Val(self.values.len() as u32 - 1)
    }

    fn ty(&self, value: Val) -> ScalarType {
        self.values[value.0 as usize]
    }

    fn push(&mut self, op: Op, result_types: &[ScalarType]) -> (Inst, Vec<Val>) {
        let block = self
            .current
            .expect("the LLVM backend needs a current block to build an instruction");
        let results = result_types
            .iter()
            .map(|ty| self.new_value(*ty))
            .collect::<Vec<_>>();
        if let Op::Jump(target, _) = &op {
            self.blocks[target.0 as usize].predecessors.push(block);
        }
        if let Op::Brif(_, then_block, _, else_block, _) = &op {
            self.blocks[then_block.0 as usize].predecessors.push(block);
            self.blocks[else_block.0 as usize].predecessors.push(block);
        }
        let inst = self.insts.len() as u32;
        for result in &results {
            self.value_defs[result.0 as usize] = Some(inst);
        }
        self.insts.push(InstIr {
            op,
            results: results.clone(),
            block,
            location: self.location,
        });
        let entry = &mut self.blocks[block as usize];
        if !entry.in_layout {
            entry.in_layout = true;
            self.layout.push(block);
        }
        entry.insts.push(inst);
        (Inst(inst), results)
    }

    fn push_value(&mut self, op: Op, ty: ScalarType) -> Val {
        self.push(op, &[ty]).1[0]
    }

    fn push_effect(&mut self, op: Op) {
        self.push(op, &[]);
    }

    fn call_results(&mut self, op: Op, returns: &[ScalarType]) -> Call {
        let (inst, results) = self.push(op, returns);
        Call { inst, results }
    }

    /// Follows sealed blocks with one predecessor back to the variable's
    /// last definition, as an SSA builder answers `use_var`.
    fn var_source(&self, var: Var) -> VarSource {
        let Some(mut block) = self.current else {
            return VarSource::Memory;
        };
        let mut visited = Vec::new();
        loop {
            let entry = &self.blocks[block as usize];
            if let Some(value) = entry.definitions.get(&var.0) {
                return VarSource::Value(*value);
            }
            if !entry.sealed || visited.contains(&block) {
                return VarSource::Memory;
            }
            visited.push(block);
            match entry.predecessors.as_slice() {
                [] => return VarSource::Zero,
                [single] => block = *single,
                _ => return VarSource::Memory,
            }
        }
    }

    /// A readable listing for verifier errors.
    fn display(&self) -> String {
        let mut text = format!("function {:?}\n", self.signature);
        for block in &self.layout {
            let entry = &self.blocks[*block as usize];
            text.push_str(&format!("block{block}({:?}):\n", entry.params));
            for inst in &entry.insts {
                let inst = &self.insts[*inst as usize];
                text.push_str(&format!("    {:?} = {:?}\n", inst.results, inst.op));
            }
        }
        text
    }

    fn verify(&self) -> Result<(), String> {
        for block in &self.layout {
            let entry = &self.blocks[*block as usize];
            let last = entry
                .insts
                .last()
                .map(|inst| &self.insts[*inst as usize].op);
            if !last.is_some_and(Op::is_terminator) {
                return Err(format!("block{block} does not end with a terminator"));
            }
            for inst in &entry.insts[..entry.insts.len() - 1] {
                if self.insts[*inst as usize].op.is_terminator() {
                    return Err(format!("block{block} has a terminator before its end"));
                }
            }
            let targets: Vec<(&Blk, &Vec<Val>)> = match last {
                Some(Op::Jump(target, args)) => vec![(target, args)],
                Some(Op::Brif(_, then_block, then_args, else_block, else_args)) => {
                    vec![(then_block, then_args), (else_block, else_args)]
                }
                _ => Vec::new(),
            };
            for (target, args) in targets {
                let target_entry = &self.blocks[target.0 as usize];
                if !target_entry.in_layout {
                    return Err(format!(
                        "block{block} branches to block{}, which has no instructions",
                        target.0
                    ));
                }
                if target_entry.params.len() != args.len() {
                    return Err(format!(
                        "block{block} passes {} arguments to block{}, which takes {}",
                        args.len(),
                        target.0,
                        target_entry.params.len()
                    ));
                }
            }
        }
        Ok(())
    }
}

/// One function body under construction, with the module it belongs to.
struct LlvmFunction<'a> {
    module: &'a mut LlvmModule,
    ir: FunctionIr,
}

impl LlvmFunction<'_> {
    fn unary(&mut self, op: UnaryOp, ty: ScalarType, operand: Val) -> Val {
        self.ir.push_value(Op::Unary(op, operand), ty)
    }

    fn binary(&mut self, op: BinaryOp, left: Val, right: Val) -> Val {
        let ty = self.ir.ty(left);
        self.ir.push_value(Op::Binary(op, left, right), ty)
    }

    fn binary_imm(&mut self, op: BinaryOp, operand: Val, imm: i64) -> Val {
        let ty = self.ir.ty(operand);
        self.ir.push_value(Op::BinaryImm(op, operand, imm), ty)
    }

    fn overflow(&mut self, op: OverflowOp, left: Val, right: Val) -> (Val, Val) {
        let ty = self.ir.ty(left);
        let (_, results) = self
            .ir
            .push(Op::Overflow(op, left, right), &[ty, ScalarType::I8]);
        (results[0], results[1])
    }

    fn load_kind(
        &mut self,
        kind: LoadKind,
        ty: ScalarType,
        flags: MemFlags,
        address: Val,
        offset: i32,
    ) -> Val {
        self.ir
            .push_value(Op::Load(kind, flags, address, offset), ty)
    }
}

impl FunctionBackend for LlvmFunction<'_> {
    fn module(&mut self) -> &mut dyn ModuleBackend {
        self.module
    }

    fn create_block(&mut self) -> Blk {
        self.ir.blocks.push(BlockIr::default());
        Blk(self.ir.blocks.len() as u32 - 1)
    }

    fn switch_to_block(&mut self, block: Blk) {
        self.ir.current = Some(block.0);
    }

    fn seal_block(&mut self, block: Blk) {
        self.ir.blocks[block.0 as usize].sealed = true;
    }

    fn seal_all_blocks(&mut self) {
        for block in &mut self.ir.blocks {
            block.sealed = true;
        }
    }

    fn set_cold_block(&mut self, block: Blk) {
        self.ir.blocks[block.0 as usize].cold = true;
    }

    fn set_source_location(&mut self, line: u32, column: u32) {
        self.ir.location = Some((line, column));
    }

    fn describe_var(&mut self, var: Var, variable: DebugVariable) {
        self.ir.debug_vars.insert(var.0, variable);
    }

    fn describe_record(&mut self, record: DebugRecord) {
        self.ir.debug_records.push(record);
    }

    fn current_block(&self) -> Option<Blk> {
        self.ir.current.map(Blk)
    }

    fn block_params(&self, block: Blk) -> Vec<Val> {
        self.ir.blocks[block.0 as usize].params.clone()
    }

    fn append_block_param(&mut self, block: Blk, ty: ScalarType) -> Val {
        let value = self.ir.new_value(ty);
        self.ir.blocks[block.0 as usize].params.push(value);
        value
    }

    fn append_block_params_for_function_params(&mut self, block: Blk) {
        for ty in self.ir.signature.params.clone() {
            self.append_block_param(block, ty);
        }
        self.ir.params_block = Some(block.0);
    }

    fn declare_var(&mut self, var: Var, ty: ScalarType) {
        self.ir.vars.insert(var.0, ty);
    }

    fn def_var(&mut self, var: Var, value: Val) {
        self.ir.push_effect(Op::DefVar(var, value));
        let block = self
            .ir
            .current
            .expect("the LLVM backend needs a current block to define a variable");
        self.ir.blocks[block as usize]
            .definitions
            .insert(var.0, value);
    }

    fn use_var(&mut self, var: Var) -> Val {
        let ty = self.ir.vars.get(&var.0).copied().unwrap_or(ScalarType::I64);
        match self.ir.var_source(var) {
            VarSource::Value(value) => value,
            VarSource::Zero => match ty {
                ScalarType::F32 | ScalarType::F64 => self.ir.push_value(Op::Fconst(0.0), ty),
                _ => self.ir.push_value(Op::Iconst(0), ty),
            },
            VarSource::Memory => self.ir.push_value(Op::UseVar(var), ty),
        }
    }

    fn create_sized_stack_slot(&mut self, size: u32, align_shift: u8) -> Slot {
        self.ir.slots.push((size, align_shift));
        Slot(self.ir.slots.len() as u32 - 1)
    }

    fn stack_addr(&mut self, ty: ScalarType, slot: Slot, offset: i32) -> Val {
        self.ir.push_value(Op::StackAddr(slot, offset), ty)
    }

    fn import_function(&mut self, function: FuncHandle) -> FuncRef {
        self.ir.func_refs.push(function);
        FuncRef(self.ir.func_refs.len() as u32 - 1)
    }

    fn import_data(&mut self, data: DataHandle) -> DataRef {
        self.ir.data_refs.push(data);
        DataRef(self.ir.data_refs.len() as u32 - 1)
    }

    fn import_signature(&mut self, signature: Signature) -> SigRef {
        self.ir.sig_refs.push(signature);
        SigRef(self.ir.sig_refs.len() as u32 - 1)
    }

    fn func_addr(&mut self, ty: ScalarType, function: FuncRef) -> Val {
        self.ir.push_value(Op::FuncAddr(function), ty)
    }

    fn symbol_value(&mut self, ty: ScalarType, data: DataRef) -> Val {
        self.ir.push_value(Op::SymbolValue(data), ty)
    }

    fn call(&mut self, callee: FuncRef, args: &[Val]) -> Call {
        let function = self.ir.func_refs[callee.0 as usize];
        let returns = self.module.functions[function.0 as usize]
            .signature
            .returns
            .clone();
        self.ir
            .call_results(Op::Call(callee, args.to_vec()), &returns)
    }

    fn call_indirect(&mut self, signature: SigRef, callee: Val, args: &[Val]) -> Call {
        let returns = self.ir.sig_refs[signature.0 as usize].returns.clone();
        self.ir
            .call_results(Op::CallIndirect(signature, callee, args.to_vec()), &returns)
    }

    fn jump(&mut self, block: Blk, args: &[Val]) {
        self.ir.push_effect(Op::Jump(block, args.to_vec()));
    }

    fn brif(
        &mut self,
        condition: Val,
        then_block: Blk,
        then_args: &[Val],
        else_block: Blk,
        else_args: &[Val],
    ) {
        self.ir.push_effect(Op::Brif(
            condition,
            then_block,
            then_args.to_vec(),
            else_block,
            else_args.to_vec(),
        ));
    }

    fn return_(&mut self, values: &[Val]) {
        self.ir.push_effect(Op::Return(values.to_vec()));
    }

    fn trap(&mut self, _code: TrapCode) {
        self.ir.push_effect(Op::Trap);
    }

    fn value_type(&self, value: Val) -> ScalarType {
        self.ir.ty(value)
    }

    fn integer_constant(&self, value: Val) -> Option<i64> {
        let inst = self
            .ir
            .value_defs
            .get(value.0 as usize)
            .copied()
            .flatten()?;
        match self.ir.insts[inst as usize].op {
            Op::Iconst(constant) => Some(constant),
            _ => None,
        }
    }

    fn only_calls_since(&self, inst: Inst, allowed: &[FuncRef]) -> bool {
        let Some(current) = self.ir.current else {
            return false;
        };
        if self.ir.insts[inst.0 as usize].block != current {
            return false;
        }
        for later in self.ir.blocks[current as usize].insts.iter().rev() {
            if *later == inst.0 {
                return true;
            }
            match &self.ir.insts[*later as usize].op {
                Op::Call(callee, _) if allowed.contains(callee) => {}
                Op::Call(..) | Op::CallIndirect(..) => return false,
                _ => {}
            }
        }
        false
    }

    fn replace_with_call(&mut self, inst: Inst, callee: FuncRef, args: &[Val]) {
        self.ir.insts[inst.0 as usize].op = Op::Call(callee, args.to_vec());
    }

    fn replace_with_nop(&mut self, inst: Inst) {
        let replaced = &mut self.ir.insts[inst.0 as usize];
        replaced.op = Op::Nop;
        replaced.results.clear();
    }

    fn replace_with_copy(&mut self, inst: Inst, value: Val) {
        let replaced = &mut self.ir.insts[inst.0 as usize];
        replaced.op = Op::Copy(value);
        replaced.results.truncate(1);
    }

    fn verify(&self) -> Result<(), String> {
        match self.ir.verify() {
            Ok(()) => Ok(()),
            Err(error) => Err(format!("{error}\n{}", self.ir.display())),
        }
    }

    #[cfg(test)]
    fn instruction_count(&self) -> usize {
        self.ir.insts.len()
    }

    fn iconst(&mut self, ty: ScalarType, value: i64) -> Val {
        self.ir.push_value(Op::Iconst(value), ty)
    }

    fn f64const(&mut self, value: f64) -> Val {
        self.ir.push_value(Op::Fconst(value), ScalarType::F64)
    }

    fn iadd(&mut self, left: Val, right: Val) -> Val {
        self.binary(BinaryOp::Iadd, left, right)
    }

    fn iadd_imm(&mut self, value: Val, imm: i64) -> Val {
        self.binary_imm(BinaryOp::Iadd, value, imm)
    }

    fn isub(&mut self, left: Val, right: Val) -> Val {
        self.binary(BinaryOp::Isub, left, right)
    }

    fn imul(&mut self, left: Val, right: Val) -> Val {
        self.binary(BinaryOp::Imul, left, right)
    }

    fn imul_imm(&mut self, value: Val, imm: i64) -> Val {
        self.binary_imm(BinaryOp::Imul, value, imm)
    }

    fn ineg(&mut self, value: Val) -> Val {
        let ty = self.ir.ty(value);
        self.unary(UnaryOp::Ineg, ty, value)
    }

    fn sdiv(&mut self, left: Val, right: Val) -> Val {
        self.binary(BinaryOp::Sdiv, left, right)
    }

    fn udiv(&mut self, left: Val, right: Val) -> Val {
        self.binary(BinaryOp::Udiv, left, right)
    }

    fn srem(&mut self, left: Val, right: Val) -> Val {
        self.binary(BinaryOp::Srem, left, right)
    }

    fn urem(&mut self, left: Val, right: Val) -> Val {
        self.binary(BinaryOp::Urem, left, right)
    }

    fn band(&mut self, left: Val, right: Val) -> Val {
        self.binary(BinaryOp::Band, left, right)
    }

    fn band_imm(&mut self, value: Val, imm: i64) -> Val {
        self.binary_imm(BinaryOp::Band, value, imm)
    }

    fn bor(&mut self, left: Val, right: Val) -> Val {
        self.binary(BinaryOp::Bor, left, right)
    }

    fn bxor(&mut self, left: Val, right: Val) -> Val {
        self.binary(BinaryOp::Bxor, left, right)
    }

    fn bxor_imm(&mut self, value: Val, imm: i64) -> Val {
        self.binary_imm(BinaryOp::Bxor, value, imm)
    }

    fn ishl(&mut self, value: Val, count: Val) -> Val {
        self.binary(BinaryOp::Ishl, value, count)
    }

    fn ushr(&mut self, value: Val, count: Val) -> Val {
        self.binary(BinaryOp::Ushr, value, count)
    }

    fn sshr(&mut self, value: Val, count: Val) -> Val {
        self.binary(BinaryOp::Sshr, value, count)
    }

    fn icmp(&mut self, cond: IntCC, left: Val, right: Val) -> Val {
        self.ir
            .push_value(Op::Icmp(cond, left, right), ScalarType::I8)
    }

    fn icmp_imm(&mut self, cond: IntCC, value: Val, imm: i64) -> Val {
        self.ir
            .push_value(Op::IcmpImm(cond, value, imm), ScalarType::I8)
    }

    fn select(&mut self, condition: Val, if_true: Val, if_false: Val) -> Val {
        let ty = self.ir.ty(if_true);
        self.ir
            .push_value(Op::Select(condition, if_true, if_false), ty)
    }

    fn uextend(&mut self, ty: ScalarType, value: Val) -> Val {
        self.unary(UnaryOp::Uextend, ty, value)
    }

    fn ireduce(&mut self, ty: ScalarType, value: Val) -> Val {
        self.unary(UnaryOp::Ireduce, ty, value)
    }

    fn bitcast(&mut self, ty: ScalarType, value: Val) -> Val {
        self.unary(UnaryOp::Bitcast, ty, value)
    }

    fn sadd_overflow(&mut self, left: Val, right: Val) -> (Val, Val) {
        self.overflow(OverflowOp::Sadd, left, right)
    }

    fn uadd_overflow(&mut self, left: Val, right: Val) -> (Val, Val) {
        self.overflow(OverflowOp::Uadd, left, right)
    }

    fn ssub_overflow(&mut self, left: Val, right: Val) -> (Val, Val) {
        self.overflow(OverflowOp::Ssub, left, right)
    }

    fn usub_overflow(&mut self, left: Val, right: Val) -> (Val, Val) {
        self.overflow(OverflowOp::Usub, left, right)
    }

    fn smul_overflow(&mut self, left: Val, right: Val) -> (Val, Val) {
        self.overflow(OverflowOp::Smul, left, right)
    }

    fn umul_overflow(&mut self, left: Val, right: Val) -> (Val, Val) {
        self.overflow(OverflowOp::Umul, left, right)
    }

    fn load(&mut self, ty: ScalarType, flags: MemFlags, address: Val, offset: i32) -> Val {
        self.load_kind(LoadKind::Plain, ty, flags, address, offset)
    }

    fn uload8(&mut self, ty: ScalarType, flags: MemFlags, address: Val, offset: i32) -> Val {
        self.load_kind(LoadKind::Uload8, ty, flags, address, offset)
    }

    fn sload8(&mut self, flags: MemFlags, address: Val, offset: i32) -> Val {
        self.load_kind(LoadKind::Sload8, ScalarType::I64, flags, address, offset)
    }

    fn uload16(&mut self, flags: MemFlags, address: Val, offset: i32) -> Val {
        self.load_kind(LoadKind::Uload16, ScalarType::I64, flags, address, offset)
    }

    fn sload16(&mut self, flags: MemFlags, address: Val, offset: i32) -> Val {
        self.load_kind(LoadKind::Sload16, ScalarType::I64, flags, address, offset)
    }

    fn uload32(&mut self, flags: MemFlags, address: Val, offset: i32) -> Val {
        self.load_kind(LoadKind::Uload32, ScalarType::I64, flags, address, offset)
    }

    fn sload32(&mut self, flags: MemFlags, address: Val, offset: i32) -> Val {
        self.load_kind(LoadKind::Sload32, ScalarType::I64, flags, address, offset)
    }

    fn store(&mut self, flags: MemFlags, value: Val, address: Val, offset: i32) {
        self.ir
            .push_effect(Op::Store(flags, value, address, offset));
    }

    fn fadd(&mut self, left: Val, right: Val) -> Val {
        self.binary(BinaryOp::Fadd, left, right)
    }

    fn fsub(&mut self, left: Val, right: Val) -> Val {
        self.binary(BinaryOp::Fsub, left, right)
    }

    fn fmul(&mut self, left: Val, right: Val) -> Val {
        self.binary(BinaryOp::Fmul, left, right)
    }

    fn fdiv(&mut self, left: Val, right: Val) -> Val {
        self.binary(BinaryOp::Fdiv, left, right)
    }

    fn fneg(&mut self, value: Val) -> Val {
        let ty = self.ir.ty(value);
        self.unary(UnaryOp::Fneg, ty, value)
    }

    fn fcmp(&mut self, cond: FloatCC, left: Val, right: Val) -> Val {
        self.ir
            .push_value(Op::Fcmp(cond, left, right), ScalarType::I8)
    }

    fn fpromote(&mut self, ty: ScalarType, value: Val) -> Val {
        self.unary(UnaryOp::Fpromote, ty, value)
    }

    fn fdemote(&mut self, ty: ScalarType, value: Val) -> Val {
        self.unary(UnaryOp::Fdemote, ty, value)
    }

    fn fcvt_from_uint(&mut self, ty: ScalarType, value: Val) -> Val {
        self.unary(UnaryOp::FcvtFromUint, ty, value)
    }

    fn fcvt_from_sint(&mut self, ty: ScalarType, value: Val) -> Val {
        self.unary(UnaryOp::FcvtFromSint, ty, value)
    }

    fn fcvt_to_sint(&mut self, ty: ScalarType, value: Val) -> Val {
        self.unary(UnaryOp::FcvtToSint, ty, value)
    }

    fn fcvt_to_uint(&mut self, ty: ScalarType, value: Val) -> Val {
        self.unary(UnaryOp::FcvtToUint, ty, value)
    }
}

fn built<T>(result: Result<T, BuilderError>) -> Result<T, String> {
    result.map_err(|error| format!("LLVM builder error: {error}"))
}

fn basic_type<'ctx>(context: &'ctx Context, ty: ScalarType) -> BasicTypeEnum<'ctx> {
    match ty {
        ScalarType::I8 => context.i8_type().into(),
        ScalarType::I16 => context.i16_type().into(),
        ScalarType::I32 => context.i32_type().into(),
        ScalarType::I64 => context.i64_type().into(),
        ScalarType::F32 => context.f32_type().into(),
        ScalarType::F64 => context.f64_type().into(),
    }
}

fn function_type<'ctx>(context: &'ctx Context, signature: &Signature) -> FunctionType<'ctx> {
    let params = signature
        .params
        .iter()
        .map(|ty| BasicMetadataTypeEnum::from(basic_type(context, *ty)))
        .collect::<Vec<_>>();
    // Both conventions lower to the C convention. A private Aura function
    // with many results returns them as one struct, which LLVM passes back
    // through memory when it does not fit the return registers.
    match (signature.call_conv, signature.returns.as_slice()) {
        (_, []) => context.void_type().fn_type(&params, false),
        (_, [single]) => basic_type(context, *single).fn_type(&params, false),
        (CallConv::Aura | CallConv::C, many) => {
            let fields = many
                .iter()
                .map(|ty| basic_type(context, *ty))
                .collect::<Vec<_>>();
            context.struct_type(&fields, false).fn_type(&params, false)
        }
    }
}

fn int_predicate(cond: IntCC) -> IntPredicate {
    match cond {
        IntCC::Equal => IntPredicate::EQ,
        IntCC::NotEqual => IntPredicate::NE,
        IntCC::SignedLessThan => IntPredicate::SLT,
        IntCC::SignedGreaterThanOrEqual => IntPredicate::SGE,
        IntCC::SignedGreaterThan => IntPredicate::SGT,
        IntCC::SignedLessThanOrEqual => IntPredicate::SLE,
        IntCC::UnsignedLessThan => IntPredicate::ULT,
        IntCC::UnsignedGreaterThanOrEqual => IntPredicate::UGE,
        IntCC::UnsignedGreaterThan => IntPredicate::UGT,
        IntCC::UnsignedLessThanOrEqual => IntPredicate::ULE,
    }
}

fn float_predicate(cond: FloatCC) -> FloatPredicate {
    match cond {
        FloatCC::Equal => FloatPredicate::OEQ,
        FloatCC::NotEqual => FloatPredicate::UNE,
        FloatCC::LessThan => FloatPredicate::OLT,
        FloatCC::LessThanOrEqual => FloatPredicate::OLE,
        FloatCC::GreaterThan => FloatPredicate::OGT,
        FloatCC::GreaterThanOrEqual => FloatPredicate::OGE,
    }
}

/// The alignment an access may assume: natural when it is trusted,
/// otherwise none, since the emitter's other accesses may be unaligned.
fn access_alignment(ty: ScalarType, flags: MemFlags) -> u32 {
    if !flags.trusted {
        return 1;
    }
    match ty {
        ScalarType::I8 => 1,
        ScalarType::I16 => 2,
        ScalarType::I32 | ScalarType::F32 => 4,
        ScalarType::I64 | ScalarType::F64 => 8,
    }
}

/// One run of a data object's contents: plain bytes or an address.
enum DataField {
    Bytes(usize, usize),
    Address(DataAddress),
}

/// The runs of `contents`, in order. An address takes eight bytes.
fn data_fields(name: &str, contents: &DataDescription) -> Result<Vec<DataField>, String> {
    let mut addresses = contents.addresses.clone();
    addresses.sort_by_key(|(offset, _)| *offset);
    let mut fields = Vec::new();
    let mut cursor = 0usize;
    for (offset, address) in addresses {
        let offset = offset as usize;
        if offset < cursor || offset + 8 > contents.bytes.len() {
            return Err(format!(
                "data `{name}` writes an address at offset {offset} outside its bytes"
            ));
        }
        if offset > cursor {
            fields.push(DataField::Bytes(cursor, offset));
        }
        fields.push(DataField::Address(address));
        cursor = offset + 8;
    }
    if cursor < contents.bytes.len() || fields.is_empty() {
        fields.push(DataField::Bytes(cursor, contents.bytes.len()));
    }
    Ok(fields)
}

/// The LLVM type of a data object: a byte array, or a packed struct of
/// byte arrays and pointers.
fn data_type<'ctx>(context: &'ctx Context, fields: &[DataField]) -> BasicTypeEnum<'ctx> {
    let types = fields
        .iter()
        .map(|field| match field {
            DataField::Bytes(start, end) => {
                context.i8_type().array_type((end - start) as u32).into()
            }
            DataField::Address(_) => context.ptr_type(AddressSpace::default()).into(),
        })
        .collect::<Vec<BasicTypeEnum<'ctx>>>();
    match (fields, types.as_slice()) {
        ([DataField::Bytes(..)], [bytes]) => *bytes,
        _ => context.struct_type(&types, true).into(),
    }
}

/// The module's functions and globals while its bodies are lowered.
struct ObjectLowerer<'ctx, 'm> {
    context: &'ctx Context,
    module: &'m Module<'ctx>,
    functions: Vec<FunctionValue<'ctx>>,
    globals: Vec<GlobalValue<'ctx>>,
    /// The functions whose bodies this unit lowers, in declaration order.
    bodies: Vec<usize>,
    debug: Option<DebugInfo<'ctx>>,
}

/// The DWARF of a debug build's module, as its functions are lowered.
struct DebugInfo<'ctx> {
    builder: DebugInfoBuilder<'ctx>,
    unit: DICompileUnit<'ctx>,
    files: RefCell<HashMap<String, DIFile<'ctx>>>,
    types: RefCell<HashMap<(String, ScalarType, bool), DIType<'ctx>>>,
    runtime_value: std::cell::Cell<Option<DIType<'ctx>>>,
}

/// The DWARF version a debug build writes, which every lldb and gdb in use
/// reads.
const DWARF_VERSION: u64 = 4;

/// The DWARF name of the record type every runtime value's word is.
const RUNTIME_VALUE_TYPE: &str = "AuraValue";

/// The DWARF expression operation that places a slot's bits at an offset
/// of a variable, as LLVM numbers it: `DW_OP_LLVM_fragment`.
const DW_OP_LLVM_FRAGMENT: i64 = 0x1000;

/// The `DW_ATE` encodings of a variable's word.
const DW_ATE_BOOLEAN: u32 = 0x02;
const DW_ATE_FLOAT: u32 = 0x04;
const DW_ATE_SIGNED: u32 = 0x05;
const DW_ATE_UNSIGNED: u32 = 0x07;

impl<'ctx> DebugInfo<'ctx> {
    /// The compile unit of `module`, named for the first source file the
    /// object describes.
    fn new(context: &'ctx Context, module: &Module<'ctx>, object: &LlvmModule) -> Self {
        let path = object
            .functions
            .iter()
            .find_map(|decl| decl.source.as_ref())
            .map_or("aura", |source| source.path.as_str());
        let (directory, file) = split_source_path(path);
        let (builder, unit) = module.create_debug_info_builder(
            true,
            DWARFSourceLanguage::C,
            &file,
            &directory,
            "aura",
            false,
            "",
            0,
            "",
            DWARFEmissionKind::Full,
            0,
            false,
            false,
            "",
            "",
        );
        let i32_type = context.i32_type();
        module.add_basic_value_flag(
            "Debug Info Version",
            FlagBehavior::Warning,
            i32_type.const_int(u64::from(module.get_debug_metadata_version()), false),
        );
        module.add_basic_value_flag(
            "Dwarf Version",
            FlagBehavior::Warning,
            i32_type.const_int(DWARF_VERSION, false),
        );
        Self {
            builder,
            unit,
            files: RefCell::new(HashMap::new()),
            types: RefCell::new(HashMap::new()),
            runtime_value: std::cell::Cell::new(None),
        }
    }

    fn file(&self, path: &str) -> DIFile<'ctx> {
        *self
            .files
            .borrow_mut()
            .entry(path.to_string())
            .or_insert_with(|| {
                let (directory, file) = split_source_path(path);
                self.builder.create_file(&file, &directory)
            })
    }

    /// The subprogram of a function defined at `source`.
    fn subprogram(&self, source: &FunctionSource) -> DISubprogram<'ctx> {
        let file = self.file(&source.path);
        let ty = self
            .builder
            .create_subroutine_type(file, None, &[], DIFlags::ZERO);
        self.builder.create_function(
            self.unit.as_debug_info_scope(),
            &source.name,
            None,
            file,
            source.line,
            ty,
            false,
            true,
            source.line,
            DIFlags::ZERO,
            false,
        )
    }

    /// The width a debugger reads of `variable`'s word of `ty`. A flag lives
    /// in the low byte of its word, which every host Aura targets stores
    /// first.
    fn variable_bits(variable: &DebugVariable, ty: ScalarType) -> u64 {
        match variable.encoding {
            DebugEncoding::Bool => 8,
            _ => scalar_bits(ty),
        }
    }

    /// The record every runtime value's word is, as `RUNTIME_VALUE_TYPE`
    /// describes it.
    fn runtime_value_type(&self, bits: u64) -> DIType<'ctx> {
        if let Some(found) = self.runtime_value.get() {
            return found;
        }
        let file = self.unit.get_file();
        let scope = self.unit.as_debug_info_scope();
        let address = self
            .builder
            .create_basic_type("__aura_address", bits, DW_ATE_UNSIGNED, DIFlags::ZERO)
            .expect("the basic type has a name");
        let member = self.builder.create_member_type(
            scope,
            "address",
            file,
            0,
            bits,
            0,
            0,
            DIFlags::ZERO,
            address.as_type(),
        );
        let record = self
            .builder
            .create_struct_type(
                scope,
                RUNTIME_VALUE_TYPE,
                file,
                0,
                bits,
                0,
                DIFlags::ZERO,
                None,
                &[member.as_type()],
                0,
                None,
                RUNTIME_VALUE_TYPE,
            )
            .as_type();
        self.runtime_value.set(Some(record));
        record
    }

    /// The type a debugger reads `variable` as, held in a word of `ty`. A
    /// runtime value reads as a pointer to an opaque type of its Aura name.
    fn variable_type(&self, variable: &DebugVariable, ty: ScalarType) -> DIType<'ctx> {
        let value = variable.encoding == DebugEncoding::Value;
        let key = (variable.type_name.clone(), ty, value);
        if let Some(found) = self.types.borrow().get(&key) {
            return *found;
        }
        let bits = Self::variable_bits(variable, ty);
        let created = if value {
            // Every runtime value is one record type holding the value's
            // address, which the Aura formatter for lldb renders, under a
            // typedef of its Aura name. A record has no value of its own, so
            // a debugger shows only the rendering.
            let file = self.unit.get_file();
            let scope = self.unit.as_debug_info_scope();
            self.builder
                .create_typedef(
                    self.runtime_value_type(bits),
                    &variable.type_name,
                    file,
                    0,
                    scope,
                    0,
                )
                .as_type()
        } else {
            let (encoding, kind) = match variable.encoding {
                DebugEncoding::Signed => (DW_ATE_SIGNED, "signed"),
                DebugEncoding::Unsigned => (DW_ATE_UNSIGNED, "unsigned"),
                DebugEncoding::Float => (DW_ATE_FLOAT, "float"),
                DebugEncoding::Bool | DebugEncoding::Value => (DW_ATE_BOOLEAN, "bool"),
            };
            // lldb reads a C basic type by its name, so each size and
            // encoding has its own, and the Aura name is a typedef of it.
            let basic = self
                .builder
                .create_basic_type(
                    &format!("__aura_{kind}{bits}"),
                    bits,
                    encoding,
                    DIFlags::ZERO,
                )
                .expect("the basic type has a name");
            self.builder
                .create_typedef(
                    basic.as_type(),
                    &variable.type_name,
                    self.unit.get_file(),
                    0,
                    self.unit.as_debug_info_scope(),
                    0,
                )
                .as_type()
        };
        self.types.borrow_mut().insert(key, created);
        created
    }
}

/// The directory and file name of a source path, as DWARF splits them.
fn split_source_path(path: &str) -> (String, String) {
    let path = std::path::Path::new(path);
    let file = path
        .file_name()
        .map_or_else(|| path.to_string_lossy(), |name| name.to_string_lossy())
        .into_owned();
    let directory = path
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_default();
    (directory, file)
}

/// The width of a word of `ty`.
fn scalar_bits(ty: ScalarType) -> u64 {
    match ty {
        ScalarType::I8 => 8,
        ScalarType::I16 => 16,
        ScalarType::I32 | ScalarType::F32 => 32,
        ScalarType::I64 | ScalarType::F64 => 64,
    }
}

impl<'ctx, 'm> ObjectLowerer<'ctx, 'm> {
    /// Declares every function and data object of `object` in `module`, as
    /// `unit` of `partition` holds it.
    fn new(
        context: &'ctx Context,
        module: &'m Module<'ctx>,
        object: &LlvmModule,
        partition: &Partition,
        unit: usize,
        debug: Option<DebugInfo<'ctx>>,
    ) -> Result<Self, String> {
        let uwtable = context
            .create_enum_attribute(Attribute::get_named_enum_kind_id("uwtable"), UWTABLE_ASYNC);
        let probe_stack = context.create_string_attribute(PROBE_STACK.0, PROBE_STACK.1);
        // A task-invariant helper reads and writes no memory LLVM can see,
        // cannot unwind, and returns, so LLVM may merge its calls.
        let task_invariant =
            [("memory", MEMORY_NONE), ("nounwind", 0), ("willreturn", 0)].map(|(name, value)| {
                context.create_enum_attribute(Attribute::get_named_enum_kind_id(name), value)
            });
        let always_inline =
            context.create_enum_attribute(Attribute::get_named_enum_kind_id("alwaysinline"), 0);
        let mut bodies = Vec::new();
        let functions = object
            .functions
            .iter()
            .enumerate()
            .map(|(index, decl)| {
                let local = decl.linkage == Linkage::Local;
                let placement = partition.function(unit, index);
                let (linkage, hidden) = match placement {
                    Placement::Private if local => (LlvmLinkage::Internal, false),
                    Placement::Private => (LlvmLinkage::External, false),
                    Placement::Shared => (LlvmLinkage::External, local),
                    Placement::Copy => (LlvmLinkage::AvailableExternally, local),
                    // A body here means another unit defines it.
                    Placement::Elsewhere => (LlvmLinkage::External, local && decl.body.is_some()),
                };
                let function = module.add_function(
                    &decl.name,
                    function_type(context, &decl.signature),
                    Some(linkage),
                );
                if hidden {
                    function
                        .as_global_value()
                        .set_visibility(GlobalVisibility::Hidden);
                }
                if placement != Placement::Elsewhere {
                    function.add_attribute(AttributeLoc::Function, uwtable);
                    function.add_attribute(AttributeLoc::Function, probe_stack);
                    bodies.push(index);
                }
                if decl.task_invariant {
                    for attribute in &task_invariant {
                        function.add_attribute(AttributeLoc::Function, *attribute);
                    }
                }
                if decl.inline {
                    function.add_attribute(AttributeLoc::Function, always_inline);
                }
                function
            })
            .collect::<Vec<_>>();
        // Every global exists before any initializer, since data may hold
        // the address of data declared after it.
        let mut globals = Vec::new();
        let mut initialized = Vec::new();
        for (index, decl) in object.data.iter().enumerate() {
            let local = decl.linkage == Linkage::Local;
            let placement = partition.data(unit, index);
            let global = match &decl.contents {
                Some(contents) => {
                    let fields = data_fields(&decl.name, contents)?;
                    let global = module.add_global(data_type(context, &fields), None, &decl.name);
                    global.set_constant(!decl.writable);
                    let (linkage, hidden) = match placement {
                        Placement::Private if local => (LlvmLinkage::Internal, false),
                        Placement::Private => (LlvmLinkage::External, false),
                        Placement::Shared => (LlvmLinkage::External, local),
                        Placement::Copy => (LlvmLinkage::AvailableExternally, local),
                        Placement::Elsewhere => (LlvmLinkage::External, local),
                    };
                    global.set_linkage(linkage);
                    if hidden {
                        global.set_visibility(GlobalVisibility::Hidden);
                    }
                    global.set_alignment(contents.align.unwrap_or(1) as u32);
                    if placement != Placement::Elsewhere {
                        initialized.push((index, fields));
                    }
                    global
                }
                None => {
                    let global =
                        module.add_global(context.i8_type().array_type(0), None, &decl.name);
                    global.set_linkage(LlvmLinkage::External);
                    global
                }
            };
            globals.push(global);
        }
        let lowerer = Self {
            context,
            module,
            functions,
            globals,
            bodies,
            debug,
        };
        for (index, fields) in initialized {
            if let Some(contents) = &object.data[index].contents {
                lowerer.globals[index]
                    .set_initializer(&lowerer.data_initializer(contents, &fields));
            }
        }
        Ok(lowerer)
    }

    /// The bytes of a data object, with the addresses it holds written in.
    fn data_initializer(
        &self,
        contents: &DataDescription,
        fields: &[DataField],
    ) -> BasicValueEnum<'ctx> {
        let values = fields
            .iter()
            .map(|field| match field {
                DataField::Bytes(start, end) => self
                    .context
                    .const_string(&contents.bytes[*start..*end], false)
                    .into(),
                DataField::Address(DataAddress::Function(function)) => self.functions
                    [function.0 as usize]
                    .as_global_value()
                    .as_pointer_value()
                    .into(),
                DataField::Address(DataAddress::Data(data)) => {
                    self.globals[data.0 as usize].as_pointer_value().into()
                }
            })
            .collect::<Vec<BasicValueEnum<'ctx>>>();
        match (fields, values.as_slice()) {
            ([DataField::Bytes(..)], [bytes]) => *bytes,
            _ => self.context.const_struct(&values, true).into(),
        }
    }

    fn lower_bodies(&self, object: &LlvmModule) -> Result<(), String> {
        for index in &self.bodies {
            let decl = &object.functions[*index];
            if let Some(body) = &decl.body {
                let mut lowerer = FunctionLowerer::new(self, self.functions[*index], body);
                if let (Some(debug), Some(source)) = (&self.debug, &decl.source) {
                    let subprogram = debug.subprogram(source);
                    self.functions[*index].set_subprogram(subprogram);
                    lowerer.scope = Some(FunctionScope {
                        subprogram,
                        file: debug.file(&source.path),
                        line: source.line,
                    });
                }
                lowerer.lower()?;
            }
        }
        Ok(())
    }
}

/// The LLVM values of one function body while it is lowered.
struct FunctionLowerer<'ctx, 'm, 'o> {
    object: &'o ObjectLowerer<'ctx, 'm>,
    context: &'ctx Context,
    function: FunctionValue<'ctx>,
    ir: &'o FunctionIr,
    builder: Builder<'ctx>,
    values: Vec<Option<BasicValueEnum<'ctx>>>,
    blocks: HashMap<u32, BasicBlock<'ctx>>,
    phis: HashMap<u32, Vec<PhiValue<'ctx>>>,
    vars: BTreeMap<u32, (PointerValue<'ctx>, BasicTypeEnum<'ctx>)>,
    slots: Vec<PointerValue<'ctx>>,
    trap_block: Option<BasicBlock<'ctx>>,
    /// The function's DWARF, in a debug build.
    scope: Option<FunctionScope<'ctx>>,
}

/// Where a debug build's function is defined.
#[derive(Clone, Copy)]
struct FunctionScope<'ctx> {
    subprogram: DISubprogram<'ctx>,
    file: DIFile<'ctx>,
    line: u32,
}

impl<'ctx, 'm, 'o> FunctionLowerer<'ctx, 'm, 'o> {
    fn new(
        object: &'o ObjectLowerer<'ctx, 'm>,
        function: FunctionValue<'ctx>,
        ir: &'o FunctionIr,
    ) -> Self {
        let context = object.context;
        Self {
            object,
            context,
            function,
            ir,
            builder: context.create_builder(),
            values: vec![None; ir.values.len()],
            blocks: HashMap::new(),
            phis: HashMap::new(),
            vars: BTreeMap::new(),
            slots: Vec::new(),
            trap_block: None,
            scope: None,
        }
    }

    /// Makes the instructions built next carry `location`, or the line of
    /// the function's `def` before its first statement. A function with no
    /// DWARF carries none.
    fn set_location(&self, location: Option<(u32, u32)>) {
        let (Some(scope), Some(debug)) = (self.scope, &self.object.debug) else {
            return;
        };
        let (line, column) = location.unwrap_or((scope.line, 0));
        let location = debug.builder.create_debug_location(
            self.context,
            line,
            column,
            scope.subprogram.as_debug_info_scope(),
            None,
        );
        self.builder.set_current_debug_location(location);
    }

    /// Declares the variables a debugger shows at their slots, at the end
    /// of `entry`. A record's fields each declare their part of it.
    fn declare_debug_vars(&self, entry: BasicBlock<'ctx>) {
        let (Some(scope), Some(debug)) = (self.scope, &self.object.debug) else {
            return;
        };
        let function_scope: DIScope<'ctx> = scope.subprogram.as_debug_info_scope();
        let location =
            debug
                .builder
                .create_debug_location(self.context, scope.line, 0, function_scope, None);
        let auto_variable = |name: &str, ty: DIType<'ctx>| {
            debug.builder.create_auto_variable(
                function_scope,
                name,
                scope.file,
                scope.line,
                ty,
                true,
                DIFlags::ZERO,
                0,
            )
        };
        for (var, variable) in &self.ir.debug_vars {
            let (Some((slot, _)), Some(ty)) = (self.vars.get(var), self.ir.vars.get(var)) else {
                continue;
            };
            let info = auto_variable(&variable.name, debug.variable_type(variable, *ty));
            self.declare(debug, *slot, info, Vec::new(), location, entry);
        }
        for record in &self.ir.debug_records {
            let mut members = Vec::new();
            let mut parts = Vec::new();
            let mut offset = 0u64;
            for (var, field) in &record.fields {
                let (Some((slot, _)), Some(ty)) = (self.vars.get(&var.0), self.ir.vars.get(&var.0))
                else {
                    continue;
                };
                let field_type = debug.variable_type(field, *ty);
                let bits = DebugInfo::variable_bits(field, *ty);
                members.push(
                    debug
                        .builder
                        .create_member_type(
                            function_scope,
                            &field.name,
                            scope.file,
                            0,
                            bits,
                            0,
                            offset,
                            DIFlags::ZERO,
                            field_type,
                        )
                        .as_type(),
                );
                parts.push((*slot, offset, bits));
                offset += bits;
            }
            let record_type = debug.builder.create_struct_type(
                function_scope,
                &record.type_name,
                scope.file,
                0,
                offset,
                0,
                DIFlags::ZERO,
                None,
                &members,
                0,
                None,
                "",
            );
            let info = auto_variable(&record.name, record_type.as_type());
            // A record of one field is wholly that field's slot, which LLVM
            // takes only without a fragment.
            let whole = parts.len() == 1;
            for (slot, offset, bits) in parts {
                let fragment = if whole {
                    Vec::new()
                } else {
                    vec![DW_OP_LLVM_FRAGMENT, offset as i64, bits as i64]
                };
                self.declare(debug, slot, info, fragment, location, entry);
            }
        }
    }

    /// Declares that `slot` holds `info`, or the part of it `expression`
    /// names, at the end of `entry`.
    fn declare(
        &self,
        debug: &DebugInfo<'ctx>,
        slot: PointerValue<'ctx>,
        info: DILocalVariable<'ctx>,
        expression: Vec<i64>,
        location: DILocation<'ctx>,
        entry: BasicBlock<'ctx>,
    ) {
        let expression = debug.builder.create_expression(expression);
        // inkwell's `insert_declare_at_end` treats the debug record LLVM
        // returns as an instruction, which it is not since LLVM 19.
        // SAFETY: every handle is live in this thread's context, and the
        // record LLVM inserts is owned by `entry`.
        unsafe {
            inkwell::llvm_sys::debuginfo::LLVMDIBuilderInsertDeclareRecordAtEnd(
                debug.builder.as_mut_ptr(),
                slot.as_value_ref(),
                info.as_mut_ptr(),
                expression.as_mut_ptr(),
                location.as_mut_ptr(),
                entry.as_mut_ptr(),
            );
        }
    }

    fn lower(mut self) -> Result<(), String> {
        let context = self.context;
        let entry = context.append_basic_block(self.function, "entry");
        for block in &self.ir.layout {
            let basic_block = context.append_basic_block(self.function, &format!("block{block}"));
            self.blocks.insert(*block, basic_block);
        }
        // Variables and stack slots live in the entry block, so mem2reg can
        // promote the variables. A variable read before any write reads
        // zero.
        self.builder.position_at_end(entry);
        self.set_location(None);
        for (var, ty) in &self.ir.vars {
            let llvm_ty = basic_type(context, *ty);
            let slot = built(self.builder.build_alloca(llvm_ty, &format!("var{var}")))?;
            built(self.builder.build_store(slot, llvm_ty.const_zero()))?;
            self.vars.insert(*var, (slot, llvm_ty));
        }
        for (size, align_shift) in &self.ir.slots {
            let slot = built(
                self.builder
                    .build_alloca(context.i8_type().array_type(*size), "slot"),
            )?;
            if let Some(instruction) = slot.as_instruction_value() {
                let _ = instruction.set_alignment(1u32 << align_shift);
            }
            self.slots.push(slot);
        }
        self.declare_debug_vars(entry);
        for block in &self.ir.layout {
            self.builder.position_at_end(self.blocks[block]);
            let mut phis = Vec::new();
            for param in &self.ir.blocks[*block as usize].params {
                let phi = built(
                    self.builder
                        .build_phi(basic_type(context, self.ir.ty(*param)), ""),
                )?;
                self.values[param.0 as usize] = Some(phi.as_basic_value());
                phis.push(phi);
            }
            self.phis.insert(*block, phis);
        }
        self.builder.position_at_end(entry);
        match self.ir.layout.first() {
            Some(first) => {
                if self.ir.params_block.is_some_and(|block| block != *first) {
                    return Err("the function parameters are not the entry block's".to_string());
                }
                let params = self.function.get_params();
                if self.ir.params_block.is_some() {
                    let phis = &self.phis[first];
                    if phis.len() != params.len() {
                        return Err(
                            "the entry block takes other parameters than the function".to_string()
                        );
                    }
                    for (phi, param) in phis.iter().zip(&params) {
                        phi.add_incoming(&[(param as &dyn BasicValue<'ctx>, entry)]);
                    }
                }
                built(self.builder.build_unconditional_branch(self.blocks[first]))?;
            }
            None => {
                built(self.builder.build_unreachable())?;
            }
        }
        for block in &self.ir.layout {
            self.builder.position_at_end(self.blocks[block]);
            for inst in &self.ir.blocks[*block as usize].insts {
                let inst = &self.ir.insts[*inst as usize];
                self.set_location(inst.location);
                self.lower_inst(inst)?;
                if inst.op.is_terminator() {
                    break;
                }
            }
        }
        Ok(())
    }

    fn value(&self, value: Val) -> Result<BasicValueEnum<'ctx>, String> {
        match self.values.get(value.0 as usize).copied().flatten() {
            Some(lowered) => Ok(lowered),
            None => Err(format!("value v{} is used before it is defined", value.0)),
        }
    }

    fn int(&self, value: Val) -> Result<IntValue<'ctx>, String> {
        match self.value(value)? {
            BasicValueEnum::IntValue(int) => Ok(int),
            other => Err(format!("value v{} is not an integer: {other:?}", value.0)),
        }
    }

    fn values(&self, values: &[Val]) -> Result<Vec<BasicValueEnum<'ctx>>, String> {
        values.iter().map(|value| self.value(*value)).collect()
    }

    fn args(&self, values: &[Val]) -> Result<Vec<BasicMetadataValueEnum<'ctx>>, String> {
        Ok(self
            .values(values)?
            .into_iter()
            .map(BasicMetadataValueEnum::from)
            .collect())
    }

    fn define(&mut self, inst: &InstIr, value: BasicValueEnum<'ctx>) {
        if let Some(result) = inst.results.first() {
            self.values[result.0 as usize] = Some(value);
        }
    }

    fn int_type(&self, ty: ScalarType) -> IntType<'ctx> {
        match ty {
            ScalarType::I8 => self.context.i8_type(),
            ScalarType::I16 => self.context.i16_type(),
            ScalarType::I32 => self.context.i32_type(),
            _ => self.context.i64_type(),
        }
    }

    fn result_type(&self, inst: &InstIr) -> ScalarType {
        inst.results
            .first()
            .map(|result| self.ir.ty(*result))
            .unwrap_or(ScalarType::I64)
    }

    /// An `i1` flag widened to the `I8` the interface's comparisons yield.
    fn flag(&self, flag: IntValue<'ctx>) -> Result<BasicValueEnum<'ctx>, String> {
        Ok(built(
            self.builder
                .build_int_z_extend(flag, self.context.i8_type(), ""),
        )?
        .into())
    }

    /// Whether an integer is nonzero, as `brif` and `select` test it.
    fn truth(&self, value: Val) -> Result<IntValue<'ctx>, String> {
        let value = self.int(value)?;
        built(self.builder.build_int_compare(
            IntPredicate::NE,
            value,
            value.get_type().const_zero(),
            "",
        ))
    }

    fn trap_block(&mut self) -> Result<BasicBlock<'ctx>, String> {
        if let Some(block) = self.trap_block {
            return Ok(block);
        }
        let resume = self.builder.get_insert_block();
        let block = self.context.append_basic_block(self.function, "trap");
        self.builder.position_at_end(block);
        self.build_trap()?;
        if let Some(resume) = resume {
            self.builder.position_at_end(resume);
        }
        self.trap_block = Some(block);
        Ok(block)
    }

    fn build_trap(&self) -> Result<(), String> {
        let trap = Intrinsic::find("llvm.trap")
            .and_then(|intrinsic| intrinsic.get_declaration(self.object.module, &[]))
            .ok_or_else(|| "LLVM has no `llvm.trap` intrinsic".to_string())?;
        built(self.builder.build_call(trap, &[], ""))?;
        built(self.builder.build_unreachable())?;
        Ok(())
    }

    /// Traps when `condition` holds, as a checked division does.
    fn trap_if(&mut self, condition: IntValue<'ctx>) -> Result<(), String> {
        let trap = self.trap_block()?;
        let resume = self.context.append_basic_block(self.function, "");
        built(
            self.builder
                .build_conditional_branch(condition, trap, resume),
        )?;
        self.builder.position_at_end(resume);
        Ok(())
    }

    fn address(&self, address: Val, offset: i32) -> Result<PointerValue<'ctx>, String> {
        let pointer = built(self.builder.build_int_to_ptr(
            self.int(address)?,
            self.context.ptr_type(AddressSpace::default()),
            "",
        ))?;
        if offset == 0 {
            return Ok(pointer);
        }
        let offset = self
            .context
            .i64_type()
            .const_int(offset as i64 as u64, true);
        // SAFETY: a byte offset from an integer address needs no bounds; the
        // GEP is not `inbounds`.
        built(unsafe {
            self.builder
                .build_gep(self.context.i8_type(), pointer, &[offset], "")
        })
    }

    fn pointer_to_int(&self, pointer: PointerValue<'ctx>) -> Result<BasicValueEnum<'ctx>, String> {
        Ok(built(
            self.builder
                .build_ptr_to_int(pointer, self.context.i64_type(), ""),
        )?
        .into())
    }

    fn branch_args(&self, target: Blk, args: &[Val]) -> Result<(), String> {
        let current = self
            .builder
            .get_insert_block()
            .ok_or_else(|| "the LLVM builder has no block".to_string())?;
        let phis = self
            .phis
            .get(&target.0)
            .ok_or_else(|| format!("block{} has no instructions", target.0))?;
        if phis.len() != args.len() {
            return Err(format!(
                "block{} takes {} arguments, not {}",
                target.0,
                phis.len(),
                args.len()
            ));
        }
        for (phi, arg) in phis.iter().zip(args) {
            let arg = self.value(*arg)?;
            phi.add_incoming(&[(&arg as &dyn BasicValue<'ctx>, current)]);
        }
        Ok(())
    }

    fn call_results(
        &mut self,
        inst: &InstIr,
        call: inkwell::values::CallSiteValue<'ctx>,
    ) -> Result<(), String> {
        let returned = call.try_as_basic_value();
        match inst.results.as_slice() {
            [] => Ok(()),
            [single] => match returned.basic() {
                Some(value) => {
                    self.values[single.0 as usize] = Some(value);
                    Ok(())
                }
                None => Err("a call that returns a value returned none".to_string()),
            },
            many => {
                let Some(BasicValueEnum::StructValue(returned)) = returned.basic() else {
                    return Err("a call that returns many values returned no struct".to_string());
                };
                for (index, result) in many.iter().enumerate() {
                    let value =
                        built(self.builder.build_extract_value(returned, index as u32, ""))?;
                    self.values[result.0 as usize] = Some(value);
                }
                Ok(())
            }
        }
    }

    fn lower_inst(&mut self, inst: &InstIr) -> Result<(), String> {
        let context = self.context;
        match &inst.op {
            Op::Nop => {}
            Op::Copy(value) => {
                let value = self.value(*value)?;
                self.define(inst, value);
            }
            Op::Iconst(constant) => {
                let ty = self.int_type(self.result_type(inst));
                self.define(inst, ty.const_int(*constant as u64, false).into());
            }
            Op::Fconst(constant) => {
                let value = match self.result_type(inst) {
                    ScalarType::F32 => context.f32_type().const_float(*constant),
                    _ => context.f64_type().const_float(*constant),
                };
                self.define(inst, value.into());
            }
            Op::Unary(op, operand) => {
                let value = self.lower_unary(*op, self.result_type(inst), *operand)?;
                self.define(inst, value);
            }
            Op::Binary(op, left, right) => {
                let right = self.value(*right)?;
                let value = self.lower_binary(*op, self.value(*left)?, right)?;
                self.define(inst, value);
            }
            Op::BinaryImm(op, left, imm) => {
                let left = self.int(*left)?;
                let right = left.get_type().const_int(*imm as u64, false);
                let value = self.lower_binary(*op, left.into(), right.into())?;
                self.define(inst, value);
            }
            Op::Icmp(cond, left, right) => {
                let compared = built(self.builder.build_int_compare(
                    int_predicate(*cond),
                    self.int(*left)?,
                    self.int(*right)?,
                    "",
                ))?;
                let value = self.flag(compared)?;
                self.define(inst, value);
            }
            Op::IcmpImm(cond, left, imm) => {
                let left = self.int(*left)?;
                let right = left.get_type().const_int(*imm as u64, false);
                let compared =
                    built(
                        self.builder
                            .build_int_compare(int_predicate(*cond), left, right, ""),
                    )?;
                let value = self.flag(compared)?;
                self.define(inst, value);
            }
            Op::Fcmp(cond, left, right) => {
                let (BasicValueEnum::FloatValue(left), BasicValueEnum::FloatValue(right)) =
                    (self.value(*left)?, self.value(*right)?)
                else {
                    return Err("fcmp needs two floats".to_string());
                };
                let compared = built(self.builder.build_float_compare(
                    float_predicate(*cond),
                    left,
                    right,
                    "",
                ))?;
                let value = self.flag(compared)?;
                self.define(inst, value);
            }
            Op::Select(condition, if_true, if_false) => {
                let condition = self.truth(*condition)?;
                let value = built(self.builder.build_select(
                    condition,
                    self.value(*if_true)?,
                    self.value(*if_false)?,
                    "",
                ))?;
                self.define(inst, value);
            }
            Op::Overflow(op, left, right) => {
                let left = self.int(*left)?;
                let intrinsic = Intrinsic::find(op.intrinsic())
                    .and_then(|intrinsic| {
                        intrinsic.get_declaration(self.object.module, &[left.get_type().into()])
                    })
                    .ok_or_else(|| format!("LLVM has no `{}` intrinsic", op.intrinsic()))?;
                let call = built(self.builder.build_call(
                    intrinsic,
                    &[left.into(), self.int(*right)?.into()],
                    "",
                ))?;
                let Some(BasicValueEnum::StructValue(pair)) = call.try_as_basic_value().basic()
                else {
                    return Err("an overflow intrinsic returned no pair".to_string());
                };
                let result = built(self.builder.build_extract_value(pair, 0, ""))?;
                let overflow = built(self.builder.build_extract_value(pair, 1, ""))?;
                let overflow = self.flag(overflow.into_int_value())?;
                self.values[inst.results[0].0 as usize] = Some(result);
                self.values[inst.results[1].0 as usize] = Some(overflow);
            }
            Op::Load(kind, flags, address, offset) => {
                let ty = self.result_type(inst);
                let pointer = self.address(*address, *offset)?;
                let loaded_ty = match kind {
                    LoadKind::Plain => ty,
                    LoadKind::Uload8 | LoadKind::Sload8 => ScalarType::I8,
                    LoadKind::Uload16 | LoadKind::Sload16 => ScalarType::I16,
                    LoadKind::Uload32 | LoadKind::Sload32 => ScalarType::I32,
                };
                let loaded = built(self.builder.build_load(
                    basic_type(context, loaded_ty),
                    pointer,
                    "",
                ))?;
                if let Some(instruction) = loaded.as_instruction_value() {
                    let _ = instruction.set_alignment(access_alignment(loaded_ty, *flags));
                    self.tag_alias(instruction, flags.alias);
                }
                let value = match kind {
                    LoadKind::Plain => loaded,
                    LoadKind::Uload8 | LoadKind::Uload16 | LoadKind::Uload32 => {
                        built(self.builder.build_int_z_extend(
                            loaded.into_int_value(),
                            self.int_type(ty),
                            "",
                        ))?
                        .into()
                    }
                    LoadKind::Sload8 | LoadKind::Sload16 | LoadKind::Sload32 => {
                        built(self.builder.build_int_s_extend(
                            loaded.into_int_value(),
                            self.int_type(ty),
                            "",
                        ))?
                        .into()
                    }
                };
                self.define(inst, value);
            }
            Op::Store(flags, value, address, offset) => {
                let pointer = self.address(*address, *offset)?;
                let stored = built(self.builder.build_store(pointer, self.value(*value)?))?;
                let _ = stored.set_alignment(access_alignment(self.ir.ty(*value), *flags));
                self.tag_alias(stored, flags.alias);
            }
            Op::StackAddr(slot, offset) => {
                let base = self
                    .slots
                    .get(slot.0 as usize)
                    .copied()
                    .ok_or_else(|| format!("stack slot {} does not exist", slot.0))?;
                let pointer = if *offset == 0 {
                    base
                } else {
                    let offset = context.i64_type().const_int(*offset as i64 as u64, true);
                    // SAFETY: the emitter addresses inside its own slot.
                    built(unsafe {
                        self.builder
                            .build_gep(context.i8_type(), base, &[offset], "")
                    })?
                };
                let value = self.pointer_to_int(pointer)?;
                self.define(inst, value);
            }
            Op::FuncAddr(function) => {
                let handle = self.ir.func_refs[function.0 as usize];
                let pointer = self.object.functions[handle.0 as usize]
                    .as_global_value()
                    .as_pointer_value();
                let value = self.pointer_to_int(pointer)?;
                self.define(inst, value);
            }
            Op::SymbolValue(data) => {
                let handle = self.ir.data_refs[data.0 as usize];
                let pointer = self.object.globals[handle.0 as usize].as_pointer_value();
                let value = self.pointer_to_int(pointer)?;
                self.define(inst, value);
            }
            Op::Call(callee, args) => {
                let handle = self.ir.func_refs[callee.0 as usize];
                let function = self.object.functions[handle.0 as usize];
                let call = built(self.builder.build_call(function, &self.args(args)?, ""))?;
                self.call_results(inst, call)?;
            }
            Op::CallIndirect(signature, callee, args) => {
                let signature = &self.ir.sig_refs[signature.0 as usize];
                let pointer = built(self.builder.build_int_to_ptr(
                    self.int(*callee)?,
                    context.ptr_type(AddressSpace::default()),
                    "",
                ))?;
                let call = built(self.builder.build_indirect_call(
                    function_type(context, signature),
                    pointer,
                    &self.args(args)?,
                    "",
                ))?;
                self.call_results(inst, call)?;
            }
            Op::UseVar(var) => {
                let (slot, ty) = self
                    .vars
                    .get(&var.0)
                    .copied()
                    .ok_or_else(|| format!("variable {} is not declared", var.0))?;
                let value = built(self.builder.build_load(ty, slot, ""))?;
                self.define(inst, value);
            }
            Op::DefVar(var, value) => {
                let (slot, _) = self
                    .vars
                    .get(&var.0)
                    .copied()
                    .ok_or_else(|| format!("variable {} is not declared", var.0))?;
                built(self.builder.build_store(slot, self.value(*value)?))?;
            }
            Op::Jump(target, args) => {
                self.branch_args(*target, args)?;
                built(
                    self.builder
                        .build_unconditional_branch(self.target(*target)?),
                )?;
            }
            Op::Brif(condition, then_block, then_args, else_block, else_args) => {
                let condition = self.truth(*condition)?;
                let then_target = self.target(*then_block)?;
                let mut else_target = self.target(*else_block)?;
                let current = self
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| "the LLVM builder has no block".to_string())?;
                // A phi tells edges apart by their predecessor, so the second
                // edge to one block with parameters goes through its own block.
                let split_else = then_block == else_block && !else_args.is_empty();
                if split_else {
                    else_target = context.append_basic_block(self.function, "");
                }
                self.branch_args(*then_block, then_args)?;
                if !split_else {
                    self.branch_args(*else_block, else_args)?;
                }
                let branch = built(self.builder.build_conditional_branch(
                    condition,
                    then_target,
                    else_target,
                ))?;
                self.weigh_cold_edges(branch, *then_block, *else_block);
                if split_else {
                    self.builder.position_at_end(else_target);
                    self.branch_args(*else_block, else_args)?;
                    built(self.builder.build_unconditional_branch(then_target))?;
                    self.builder.position_at_end(current);
                }
            }
            Op::Return(values) => {
                let values = self.values(values)?;
                match values.as_slice() {
                    [] => built(self.builder.build_return(None))?,
                    [single] => built(self.builder.build_return(Some(single)))?,
                    many => built(self.builder.build_aggregate_return(many))?,
                };
            }
            Op::Trap => self.build_trap()?,
        }
        Ok(())
    }

    /// Marks an access of known memory with type-based alias metadata, so
    /// LLVM knows that accesses of two different classes never overlap. An
    /// access of any memory carries none, so it may overlap every class.
    fn tag_alias(&self, access: InstructionValue<'ctx>, alias: AliasClass) {
        let name = match alias {
            AliasClass::Any => return,
            AliasClass::FrameHeader => "aura frame stack header",
            AliasClass::FrameEntry => "aura frame stack entry",
        };
        let zero = self.context.i64_type().const_zero().into();
        let root = self
            .context
            .metadata_node(&[self.context.metadata_string("aura memory").into()]);
        let class = self.context.metadata_node(&[
            self.context.metadata_string(name).into(),
            root.into(),
            zero,
        ]);
        let tag = self
            .context
            .metadata_node(&[class.into(), class.into(), zero]);
        // Only metadata that is not a node fails to attach.
        let attached = access
            .set_metadata(tag, self.context.get_kind_id("tbaa"))
            .is_ok();
        debug_assert!(attached, "alias tags are metadata nodes");
    }

    /// Weighs a branch against the edge to a cold block. A branch whose
    /// edges are both cold, or neither, keeps LLVM's own estimate.
    fn weigh_cold_edges(&self, branch: InstructionValue<'ctx>, then_block: Blk, else_block: Blk) {
        let cold = |block: Blk| self.ir.blocks[block.0 as usize].cold;
        let (rare, common) = COLD_BRANCH_WEIGHTS;
        let (then_weight, else_weight) = match (cold(then_block), cold(else_block)) {
            (true, false) => (rare, common),
            (false, true) => (common, rare),
            _ => return,
        };
        let weight = |value: u64| self.context.i32_type().const_int(value, false).into();
        let weights = self.context.metadata_node(&[
            self.context.metadata_string("branch_weights").into(),
            weight(then_weight),
            weight(else_weight),
        ]);
        // Only metadata that is not a node fails to attach.
        let attached = branch
            .set_metadata(weights, self.context.get_kind_id("prof"))
            .is_ok();
        debug_assert!(attached, "branch weights are a metadata node");
    }

    fn target(&self, block: Blk) -> Result<BasicBlock<'ctx>, String> {
        self.blocks
            .get(&block.0)
            .copied()
            .ok_or_else(|| format!("block{} has no instructions", block.0))
    }

    fn lower_unary(
        &self,
        op: UnaryOp,
        ty: ScalarType,
        operand: Val,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let context = self.context;
        let value = self.value(operand)?;
        let float_type = match ty {
            ScalarType::F32 => context.f32_type(),
            _ => context.f64_type(),
        };
        Ok(match op {
            UnaryOp::Ineg => built(self.builder.build_int_neg(value.into_int_value(), ""))?.into(),
            UnaryOp::Fneg => {
                built(self.builder.build_float_neg(value.into_float_value(), ""))?.into()
            }
            UnaryOp::Uextend => built(self.builder.build_int_z_extend(
                value.into_int_value(),
                self.int_type(ty),
                "",
            ))?
            .into(),
            UnaryOp::Ireduce => built(self.builder.build_int_truncate(
                value.into_int_value(),
                self.int_type(ty),
                "",
            ))?
            .into(),
            UnaryOp::Bitcast => built(self.builder.build_bit_cast(
                value,
                basic_type(context, ty),
                "",
            ))?,
            UnaryOp::Fpromote => built(self.builder.build_float_ext(
                value.into_float_value(),
                float_type,
                "",
            ))?
            .into(),
            UnaryOp::Fdemote => built(self.builder.build_float_trunc(
                value.into_float_value(),
                float_type,
                "",
            ))?
            .into(),
            UnaryOp::FcvtFromUint => built(self.builder.build_unsigned_int_to_float(
                value.into_int_value(),
                float_type,
                "",
            ))?
            .into(),
            UnaryOp::FcvtFromSint => built(self.builder.build_signed_int_to_float(
                value.into_int_value(),
                float_type,
                "",
            ))?
            .into(),
            UnaryOp::FcvtToUint => built(self.builder.build_float_to_unsigned_int(
                value.into_float_value(),
                self.int_type(ty),
                "",
            ))?
            .into(),
            UnaryOp::FcvtToSint => built(self.builder.build_float_to_signed_int(
                value.into_float_value(),
                self.int_type(ty),
                "",
            ))?
            .into(),
        })
    }

    fn lower_binary(
        &mut self,
        op: BinaryOp,
        left: BasicValueEnum<'ctx>,
        right: BasicValueEnum<'ctx>,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        if let (BasicValueEnum::FloatValue(left), BasicValueEnum::FloatValue(right)) = (left, right)
        {
            let value = match op {
                BinaryOp::Fadd => self.builder.build_float_add(left, right, ""),
                BinaryOp::Fsub => self.builder.build_float_sub(left, right, ""),
                BinaryOp::Fmul => self.builder.build_float_mul(left, right, ""),
                BinaryOp::Fdiv => self.builder.build_float_div(left, right, ""),
                _ => return Err(format!("{op:?} needs integers, not floats")),
            };
            return Ok(built(value)?.into());
        }
        let (BasicValueEnum::IntValue(left), BasicValueEnum::IntValue(right)) = (left, right)
        else {
            return Err(format!("{op:?} needs two integers or two floats"));
        };
        let ty = left.get_type();
        let value = match op {
            BinaryOp::Iadd => self.builder.build_int_add(left, right, ""),
            BinaryOp::Isub => self.builder.build_int_sub(left, right, ""),
            BinaryOp::Imul => self.builder.build_int_mul(left, right, ""),
            BinaryOp::Band => self.builder.build_and(left, right, ""),
            BinaryOp::Bor => self.builder.build_or(left, right, ""),
            BinaryOp::Bxor => self.builder.build_xor(left, right, ""),
            BinaryOp::Ishl => self.builder.build_left_shift(left, right, ""),
            BinaryOp::Ushr => self.builder.build_right_shift(left, right, false, ""),
            BinaryOp::Sshr => self.builder.build_right_shift(left, right, true, ""),
            BinaryOp::Udiv | BinaryOp::Urem => {
                // Division by zero traps. LLVM leaves it undefined.
                let zero = built(self.builder.build_int_compare(
                    IntPredicate::EQ,
                    right,
                    ty.const_zero(),
                    "",
                ))?;
                self.trap_if(zero)?;
                if op == BinaryOp::Udiv {
                    self.builder.build_int_unsigned_div(left, right, "")
                } else {
                    self.builder.build_int_unsigned_rem(left, right, "")
                }
            }
            BinaryOp::Sdiv => {
                // A zero divisor traps, and so does the one quotient that
                // overflows.
                let zero = built(self.builder.build_int_compare(
                    IntPredicate::EQ,
                    right,
                    ty.const_zero(),
                    "",
                ))?;
                let minimum = ty.const_int(1u64 << (ty.get_bit_width() - 1), false);
                let left_minimum =
                    built(
                        self.builder
                            .build_int_compare(IntPredicate::EQ, left, minimum, ""),
                    )?;
                let right_minus_one = built(self.builder.build_int_compare(
                    IntPredicate::EQ,
                    right,
                    ty.const_all_ones(),
                    "",
                ))?;
                let overflow = built(self.builder.build_and(left_minimum, right_minus_one, ""))?;
                let trap = built(self.builder.build_or(zero, overflow, ""))?;
                self.trap_if(trap)?;
                self.builder.build_int_signed_div(left, right, "")
            }
            BinaryOp::Srem => {
                // A zero divisor traps, and `x srem -1` is zero. LLVM leaves
                // both undefined, so -1 becomes 1, which gives the same zero.
                let zero = built(self.builder.build_int_compare(
                    IntPredicate::EQ,
                    right,
                    ty.const_zero(),
                    "",
                ))?;
                self.trap_if(zero)?;
                let minus_one = built(self.builder.build_int_compare(
                    IntPredicate::EQ,
                    right,
                    ty.const_all_ones(),
                    "",
                ))?;
                let divisor =
                    built(
                        self.builder
                            .build_select(minus_one, ty.const_int(1, false), right, ""),
                    )?
                    .into_int_value();
                self.builder.build_int_signed_rem(left, divisor, "")
            }
            BinaryOp::Fadd | BinaryOp::Fsub | BinaryOp::Fmul | BinaryOp::Fdiv => {
                return Err(format!("{op:?} needs floats, not integers"));
            }
        };
        Ok(built(value)?.into())
    }
}

#[path = "native_backend_llvm_units.rs"]
mod units;

#[cfg(test)]
#[path = "native_backend_llvm_tests.rs"]
mod tests;
