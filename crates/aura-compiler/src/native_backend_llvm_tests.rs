use inkwell::context::Context;
use inkwell::targets::{InitializationConfig, Target, TargetTriple};
use object::{Object, ObjectSection, ObjectSymbol};

use super::units::{Partition, Placement};
use super::{host_target_machine, target_machine, LlvmModule};
use crate::native_backend::{
    types, AliasClass, CallConv, DataDescription, DefineError, Extension, FuncHandle,
    FunctionBackend, IntCC, Linkage, MemFlags, ModuleBackend, NoWrapOp, ScalarType, Signature,
    TrapCode, Var,
};

fn signature(call_conv: CallConv, params: &[ScalarType], returns: &[ScalarType]) -> Signature {
    Signature {
        call_conv,
        params: params.to_vec(),
        returns: returns.to_vec(),
        extensions: Vec::new(),
        indirects: Vec::new(),
    }
}

fn module() -> LlvmModule {
    LlvmModule::for_host().expect("the host should have an LLVM target")
}

fn define(
    module: &mut LlvmModule,
    name: &str,
    linkage: Linkage,
    signature: &Signature,
    body: impl FnOnce(&mut dyn FunctionBackend) + 'static,
) -> FuncHandle {
    let function = module
        .declare_function(name, linkage, signature)
        .expect("the function should declare");
    module
        .define_function(
            function,
            signature,
            Box::new(move |builder| {
                body(builder);
                builder.verify()
            }),
        )
        .expect("the function should define");
    function
}

/// The module's LLVM IR before any pass runs.
fn ir(module: &LlvmModule) -> String {
    let machine = host_target_machine().expect("the host target machine should exist");
    let context = Context::create();
    let lowered = module
        .lower(&context, &machine)
        .expect("the module should lower and verify");
    lowered.print_to_string().to_string()
}

#[test]
fn block_params_become_phis_with_one_incoming_per_edge() {
    let mut module = module();
    let sig = signature(CallConv::C, &[types::I64], &[types::I64]);
    define(&mut module, "choose", Linkage::Export, &sig, |builder| {
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        let join = builder.create_block();
        let joined = builder.append_block_param(join, types::I64);
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let flag = builder.block_params(entry)[0];
        let one = builder.iconst(types::I64, 1);
        let two = builder.iconst(types::I64, 2);
        // Both edges reach one block, so the second goes through its own
        // block to give the phi two predecessors.
        builder.brif(flag, join, &[one], join, &[two]);
        builder.switch_to_block(join);
        builder.seal_block(join);
        builder.return_(&[joined]);
    });

    let ir = ir(&module);
    assert!(ir.contains("define i64 @choose(i64 %0)"), "{ir}");
    assert!(ir.contains("phi i64 [ %0, %entry ]"), "{ir}");
    assert!(ir.contains("phi i64 [ 1, %block0 ], [ 2, %"), "{ir}");
}

#[test]
fn aura_functions_return_many_values_as_one_struct_for_direct_and_indirect_calls() {
    let mut module = module();
    let triple = signature(CallConv::Aura, &[], &[types::I64, types::F64, types::I32]);
    let three = define(&mut module, "three", Linkage::Local, &triple, |builder| {
        let entry = builder.create_block();
        builder.switch_to_block(entry);
        let first = builder.iconst(types::I64, 7);
        let second = builder.f64const(1.5);
        let third = builder.iconst(types::I32, 9);
        builder.return_(&[first, second, third]);
    });
    let caller = signature(CallConv::C, &[], &[types::I64]);
    define(
        &mut module,
        "caller",
        Linkage::Export,
        &caller,
        move |builder| {
            let entry = builder.create_block();
            builder.switch_to_block(entry);
            let callee = builder.import_function(three);
            let direct = builder.call(callee, &[]);
            assert_eq!(direct.results.len(), 3);
            assert_eq!(builder.value_type(direct.results[1]), types::F64);
            let address = builder.func_addr(types::I64, callee);
            let indirect_signature = builder.import_signature(signature(
                CallConv::Aura,
                &[],
                &[types::I64, types::F64, types::I32],
            ));
            let indirect = builder.call_indirect(indirect_signature, address, &[]);
            let sum = builder.iadd(direct.results[0], indirect.results[0]);
            builder.return_(&[sum]);
        },
    );

    let ir = ir(&module);
    assert!(
        ir.contains("define internal { i64, double, i32 } @three()"),
        "{ir}"
    );
    assert!(
        ir.contains("ret { i64, double, i32 } { i64 7, double 1.500000e+00, i32 9 }"),
        "{ir}"
    );
    assert!(ir.contains("call { i64, double, i32 } @three()"), "{ir}");
    assert!(ir.contains("ptrtoint (ptr @three to i64)"), "{ir}");
    assert!(ir.contains("call { i64, double, i32 } inttoptr"), "{ir}");
    assert!(ir.contains("extractvalue { i64, double, i32 }"), "{ir}");
}

#[test]
fn only_calls_since_allows_the_named_callees_in_the_current_block() {
    let mut module = module();
    let unary = signature(CallConv::C, &[types::I64], &[types::I64]);
    let tag = module
        .declare_function("tag", Linkage::Import, &unary)
        .unwrap();
    let retain = module
        .declare_function("retain", Linkage::Import, &unary)
        .unwrap();
    let other = module
        .declare_function("other", Linkage::Import, &unary)
        .unwrap();
    define(
        &mut module,
        "peephole",
        Linkage::Export,
        &unary,
        move |builder| {
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            let handle = builder.block_params(entry)[0];
            let tag = builder.import_function(tag);
            let retain = builder.import_function(retain);
            let other = builder.import_function(other);
            let tagged = builder.call(tag, &[handle]);
            let sum = builder.iadd_imm(handle, 1);
            assert!(builder.only_calls_since(tagged.inst, &[retain]));
            builder.call(retain, &[sum]);
            assert!(builder.only_calls_since(tagged.inst, &[retain]));
            assert!(!builder.only_calls_since(tagged.inst, &[]));
            builder.call(other, &[sum]);
            assert!(!builder.only_calls_since(tagged.inst, &[retain]));
            let next = builder.create_block();
            builder.jump(next, &[]);
            builder.switch_to_block(next);
            builder.seal_block(next);
            assert!(!builder.only_calls_since(tagged.inst, &[retain, other]));
            builder.return_(&[sum]);
        },
    );
}

#[test]
fn rewrites_turn_calls_into_other_calls_copies_and_nothing() {
    let mut module = module();
    let unary = signature(CallConv::C, &[types::I64], &[types::I64]);
    let retain = module
        .declare_function("retain", Linkage::Import, &unary)
        .unwrap();
    let release = module
        .declare_function("release", Linkage::Import, &unary)
        .unwrap();
    let clone = module
        .declare_function("clone", Linkage::Import, &unary)
        .unwrap();
    define(
        &mut module,
        "rewrite",
        Linkage::Export,
        &unary,
        move |builder| {
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            let handle = builder.block_params(entry)[0];
            let retain = builder.import_function(retain);
            let release = builder.import_function(release);
            let clone = builder.import_function(clone);
            let copied = builder.call(retain, &[handle]);
            let cloned = builder.call(retain, &[handle]);
            let released = builder.call(release, &[handle]);
            builder.replace_with_copy(copied.inst, handle);
            builder.replace_with_call(cloned.inst, clone, &[handle]);
            builder.replace_with_nop(released.inst);
            let sum = builder.iadd(copied.results[0], cloned.results[0]);
            builder.return_(&[sum]);
        },
    );

    let ir = ir(&module);
    assert!(!ir.contains("call i64 @retain"), "{ir}");
    assert!(!ir.contains("call i64 @release"), "{ir}");
    assert!(ir.contains("%2 = call i64 @clone(i64 %1)"), "{ir}");
    assert!(ir.contains("add i64 %1, %2"), "{ir}");
}

#[test]
fn use_var_follows_sealed_single_predecessors() {
    let mut module = module();
    let sig = signature(CallConv::C, &[types::I64], &[types::I64]);
    define(&mut module, "vars", Linkage::Export, &sig, |builder| {
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let flag = builder.block_params(entry)[0];
        let (zeroed, never_defined) = (Var(0), Var(1));
        builder.declare_var(zeroed, types::I64);
        builder.declare_var(never_defined, types::I64);
        let unset = builder.use_var(never_defined);
        assert_eq!(builder.integer_constant(unset), Some(0));
        let zero = builder.iconst(types::I64, 0);
        builder.def_var(zeroed, zero);
        assert_eq!(builder.use_var(zeroed), zero);

        let left = builder.create_block();
        let right = builder.create_block();
        let join = builder.create_block();
        builder.brif(flag, left, &[], right, &[]);
        builder.switch_to_block(left);
        builder.seal_block(left);
        assert_eq!(builder.use_var(zeroed), zero);
        builder.jump(join, &[]);
        builder.switch_to_block(right);
        builder.seal_block(right);
        let one = builder.iconst(types::I64, 1);
        builder.def_var(zeroed, one);
        builder.jump(join, &[]);
        builder.switch_to_block(join);
        builder.seal_block(join);
        // Two predecessors define it differently, so the value comes from
        // its stack slot.
        let merged = builder.use_var(zeroed);
        assert_eq!(builder.integer_constant(merged), None);
        builder.return_(&[merged]);
    });

    let ir = ir(&module);
    assert!(ir.contains("%var0 = alloca i64"), "{ir}");
    assert!(ir.contains("load i64, ptr %var0"), "{ir}");
}

#[test]
fn traps_and_checked_division_reach_llvm_trap() {
    let mut module = module();
    let sig = signature(CallConv::C, &[types::I64, types::I64], &[types::I64]);
    define(&mut module, "divide", Linkage::Export, &sig, |builder| {
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let params = builder.block_params(entry);
        let quotient = builder.sdiv(params[0], params[1]);
        let remainder = builder.srem(params[0], params[1]);
        let unsigned = builder.urem(quotient, params[1]);
        let negative = builder.icmp_imm(IntCC::SignedLessThan, remainder, 0);
        let fail = builder.create_block();
        let done = builder.create_block();
        builder.brif(negative, fail, &[], done, &[]);
        builder.switch_to_block(fail);
        builder.trap(TrapCode::User(1));
        builder.switch_to_block(done);
        builder.return_(&[unsigned]);
    });

    let ir = ir(&module);
    assert!(ir.contains("call void @llvm.trap()"), "{ir}");
    assert!(ir.contains("sdiv i64 %2, %3"), "{ir}");
    assert!(ir.contains("srem i64 %2, %"), "{ir}");
    assert!(ir.contains("select i1"), "{ir}");
    assert!(ir.contains("trap:"), "{ir}");
    assert!(ir.contains("unreachable"), "{ir}");
}

#[test]
fn verify_rejects_a_block_without_a_terminator() {
    let mut module = module();
    let sig = signature(CallConv::C, &[], &[]);
    let function = module
        .declare_function("open", Linkage::Export, &sig)
        .unwrap();
    let error = module
        .define_function(
            function,
            &sig,
            Box::new(|builder| {
                let entry = builder.create_block();
                builder.switch_to_block(entry);
                builder.iconst(types::I64, 1);
                builder.verify()
            }),
        )
        .unwrap_err();
    let DefineError::Body(error) = error else {
        panic!("the body's own error should come back: {error:?}");
    };
    assert!(error.contains("does not end with a terminator"), "{error}");
    assert!(error.contains("Iconst(1)"), "{error}");
}

#[test]
fn data_holds_bytes_and_the_addresses_written_into_it() {
    let mut module = module();
    let sig = signature(CallConv::C, &[], &[]);
    let function = define(&mut module, "target", Linkage::Local, &sig, |builder| {
        let entry = builder.create_block();
        builder.switch_to_block(entry);
        builder.return_(&[]);
    });
    let text = module.declare_data("text", Linkage::Local, false).unwrap();
    module
        .define_data(text, DataDescription::new(b"abc".to_vec()))
        .unwrap();
    let table = module.declare_data("table", Linkage::Local, true).unwrap();
    let mut contents = DataDescription::new(vec![0; 20]);
    contents.align = Some(8);
    contents.write_data_addr(8, text);
    contents.write_function_addr(0, function);
    module.define_data(table, contents).unwrap();

    let ir = ir(&module);
    assert!(
        ir.contains("@text = internal constant [3 x i8] c\"abc\", align 1"),
        "{ir}"
    );
    assert!(
        ir.contains(
            "@table = internal global <{ ptr, ptr, [4 x i8] }> <{ ptr @target, ptr @text, [4 x i8] zeroinitializer }>, align 8"
        ),
        "{ir}"
    );
}

#[test]
fn declarations_merge_and_definitions_happen_once() {
    let mut module = module();
    let sig = signature(CallConv::C, &[], &[]);
    let imported = module
        .declare_function("twice", Linkage::Import, &sig)
        .unwrap();
    let local = module
        .declare_function("twice", Linkage::Local, &sig)
        .unwrap();
    assert_eq!(imported, local);
    assert_eq!(module.functions[local.0 as usize].linkage, Linkage::Local);
    let other = signature(CallConv::C, &[types::I64], &[]);
    assert!(module
        .declare_function("twice", Linkage::Local, &other)
        .unwrap_err()
        .contains("incompatible"));
    assert!(module.declare_data("twice", Linkage::Local, false).is_err());

    let external = module
        .declare_function("external", Linkage::Import, &sig)
        .unwrap();
    let body = || -> crate::native_backend::FunctionBody<'static> { Box::new(|_| Ok(())) };
    assert!(matches!(
        module.define_function(external, &sig, body()),
        Err(DefineError::Backend(_))
    ));
    define(&mut module, "once", Linkage::Export, &sig, |builder| {
        let entry = builder.create_block();
        builder.switch_to_block(entry);
        builder.return_(&[]);
    });
    let once = module
        .declare_function("once", Linkage::Export, &sig)
        .unwrap();
    assert!(matches!(
        module.define_function(once, &sig, body()),
        Err(DefineError::Backend(_))
    ));

    let data = module.declare_data("bytes", Linkage::Local, false).unwrap();
    module
        .define_data(data, DataDescription::new(vec![1]))
        .unwrap();
    assert!(module
        .define_data(data, DataDescription::new(vec![1]))
        .is_err());
}

#[test]
fn finished_objects_carry_symbols_and_unwind_tables() {
    let mut module = module();
    let sig = signature(CallConv::C, &[types::I64], &[types::I64]);
    define(&mut module, "exported", Linkage::Export, &sig, |builder| {
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let value = builder.block_params(entry)[0];
        let doubled = builder.imul_imm(value, 2);
        builder.return_(&[doubled]);
    });
    let bytes = Box::new(module).finish().expect("the object should write");
    let object = object::File::parse(bytes.as_slice()).expect("the object should parse");
    assert!(object
        .symbols()
        .any(|symbol| symbol.name().is_ok_and(|name| name.ends_with("exported"))));
    if cfg!(target_os = "linux") {
        assert!(object
            .sections()
            .any(|section| section.name() == Ok(".eh_frame")));
    }
}

/// A module with two exported functions, each returning its argument plus
/// a different constant, so LLVM keeps both.
fn two_function_module() -> LlvmModule {
    let mut module = module();
    let sig = signature(CallConv::C, &[types::I64], &[types::I64]);
    for (name, addend) in [("first", 1), ("second", 2)] {
        define(&mut module, name, Linkage::Export, &sig, move |builder| {
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            let value = builder.block_params(entry)[0];
            let sum = builder.iadd_imm(value, addend);
            builder.return_(&[sum]);
        });
    }
    module
}

fn object_for_triple(module: &LlvmModule, triple: &str) -> Vec<u8> {
    let config = InitializationConfig::default();
    Target::initialize_x86(&config);
    Target::initialize_aarch64(&config);
    let machine = target_machine(&TargetTriple::create(triple)).expect("the target should exist");
    module
        .object_for(&machine)
        .expect("the object should write")
}

#[test]
fn elf_objects_put_every_function_in_its_own_section() {
    let module = two_function_module();
    for triple in ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"] {
        let bytes = object_for_triple(&module, triple);
        let object = object::File::parse(bytes.as_slice()).expect("the object should parse");
        let mut sections = Vec::new();
        for name in ["first", "second"] {
            let symbol = object
                .symbol_by_name(name)
                .unwrap_or_else(|| panic!("{triple} should define `{name}`"));
            let index = symbol
                .section_index()
                .expect("a defined symbol has a section");
            let section = object.section_by_index(index).unwrap();
            assert_eq!(
                section.name(),
                Ok(format!(".text.{name}").as_str()),
                "{triple}"
            );
            sections.push(index);
        }
        assert_ne!(sections[0], sections[1], "{triple}");
    }
}

#[test]
fn mach_o_objects_are_split_by_symbol() {
    use object::FileFlags;
    const MH_SUBSECTIONS_VIA_SYMBOLS: u32 = 0x2000;
    let module = two_function_module();
    for triple in ["arm64-apple-macosx11.0.0", "x86_64-apple-macosx10.12.0"] {
        let bytes = object_for_triple(&module, triple);
        let object = object::File::parse(bytes.as_slice()).expect("the object should parse");
        let FileFlags::MachO { flags } = object.flags() else {
            panic!("{triple} should write Mach-O");
        };
        assert_ne!(flags & MH_SUBSECTIONS_VIA_SYMBOLS, 0, "{triple}");
        // Both functions share one text section, split by symbol.
        let first = object.symbol_by_name("_first").unwrap().section_index();
        let second = object.symbol_by_name("_second").unwrap().section_index();
        assert_eq!(first, second, "{triple}");
    }
}

/// A module whose one exported function keeps a 64 KiB frame: it hands the
/// address of a stack slot that large to a function LLVM cannot see.
fn large_frame_module() -> LlvmModule {
    let mut module = module();
    let effect = signature(CallConv::C, &[types::I64], &[]);
    let sink = module
        .declare_function("opaque_sink", Linkage::Import, &effect)
        .expect("the import should declare");
    let sig = signature(CallConv::C, &[], &[]);
    define(
        &mut module,
        "large_frame",
        Linkage::Export,
        &sig,
        move |builder| {
            let entry = builder.create_block();
            builder.switch_to_block(entry);
            let slot = builder.create_sized_stack_slot(64 * 1024, 3);
            let address = builder.stack_addr(types::I64, slot, 0);
            let sink = builder.import_function(sink);
            builder.call(sink, &[address]);
            builder.return_(&[]);
        },
    );
    module
}

#[test]
fn every_defined_function_probes_frames_larger_than_a_page() {
    let module = large_frame_module();
    let ir = ir(&module);
    assert!(ir.contains("\"probe-stack\"=\"inline-asm\""), "{ir}");

    // A task stack has one guard page, so x86-64 code must move the stack
    // pointer a page at a time (`sub $0x1000, %rsp`) instead of past the
    // whole frame in one step.
    let bytes = object_for_triple(&module, "x86_64-unknown-linux-gnu");
    let object = object::File::parse(bytes.as_slice()).expect("the object should parse");
    let symbol = object
        .symbol_by_name("large_frame")
        .expect("the function should be defined");
    let section = object
        .section_by_index(
            symbol
                .section_index()
                .expect("a defined symbol has a section"),
        )
        .unwrap();
    let code = section.data().expect("the section should have code");
    let page_step = [0x48, 0x81, 0xec, 0x00, 0x10, 0x00, 0x00];
    assert!(
        code.windows(page_step.len())
            .any(|window| window == page_step),
        "the frame should be probed a page at a time: {code:02x?}"
    );
}

/// The module's LLVM IR after the object's pass pipeline.
fn optimized_ir(module: &LlvmModule) -> String {
    let machine = host_target_machine().expect("the host target machine should exist");
    let context = Context::create();
    let lowered = module
        .lower(&context, &machine)
        .expect("the module should lower and verify");
    lowered
        .run_passes(
            super::PASS_PIPELINE,
            &machine,
            inkwell::passes::PassBuilderOptions::create(),
        )
        .expect("the passes should run");
    lowered.print_to_string().to_string()
}

#[test]
fn task_invariant_imports_let_llvm_merge_their_calls() {
    let mut module = module();
    let getter = signature(CallConv::C, &[], &[types::I64]);
    let effect = signature(CallConv::C, &[types::I64], &[]);
    let stack = module
        .declare_function("aura_direct_frame_stack", Linkage::Import, &getter)
        .expect("the import should declare");
    module.mark_task_invariant(stack);
    let opaque = module
        .declare_function("opaque_effect", Linkage::Import, &effect)
        .expect("the import should declare");
    let sig = signature(CallConv::C, &[], &[types::I64]);
    define(
        &mut module,
        "twice",
        Linkage::Export,
        &sig,
        move |builder| {
            let entry = builder.create_block();
            builder.switch_to_block(entry);
            let stack = builder.import_function(stack);
            let opaque = builder.import_function(opaque);
            let first = builder.call(stack, &[]).results[0];
            builder.call(opaque, &[first]);
            let second = builder.call(stack, &[]).results[0];
            let sum = builder.iadd(first, second);
            builder.return_(&[sum]);
        },
    );

    let ir = ir(&module);
    assert!(
        ir.contains("declare i64 @aura_direct_frame_stack() #"),
        "{ir}"
    );
    assert!(
        ir.contains("memory(none)") && ir.contains("nounwind") && ir.contains("willreturn"),
        "{ir}"
    );
    let optimized = optimized_ir(&module);
    assert_eq!(
        optimized
            .matches("call i64 @aura_direct_frame_stack()")
            .count(),
        1,
        "a call between them does not keep both: {optimized}"
    );
}

#[test]
fn frame_stack_accesses_of_different_classes_never_overlap() {
    let mut module = module();
    let sig = signature(CallConv::C, &[types::I64, types::I64], &[types::I64]);
    for (name, header, entry) in [
        (
            "tagged",
            MemFlags::trusted_in(AliasClass::FrameHeader),
            MemFlags::trusted_in(AliasClass::FrameEntry),
        ),
        ("untagged", MemFlags::trusted(), MemFlags::trusted()),
    ] {
        define(&mut module, name, Linkage::Export, &sig, move |builder| {
            let entry_block = builder.create_block();
            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);
            let stack = builder.block_params(entry_block)[0];
            let slot = builder.block_params(entry_block)[1];
            let len = builder.iconst(types::I64, 7);
            builder.store(header, len, stack, 0);
            let word = builder.iconst(types::I64, 9);
            builder.store(entry, word, slot, 0);
            let reloaded = builder.load(types::I64, header, stack, 0);
            builder.return_(&[reloaded]);
        });
    }

    let ir = ir(&module);
    assert!(ir.contains("!tbaa"), "{ir}");
    assert!(ir.contains("aura frame stack header"), "{ir}");
    assert!(ir.contains("aura frame stack entry"), "{ir}");
    let optimized = optimized_ir(&module);
    let body = |name: &str| {
        let start = optimized
            .find(&format!("@{name}("))
            .expect("the function is defined");
        let end = optimized[start..].find("\n}").expect("the body ends") + start;
        optimized[start..end].to_string()
    };
    assert!(
        body("tagged").contains("ret i64 7"),
        "the entry store cannot change the header: {optimized}"
    );
    assert!(
        body("untagged").contains("load i64"),
        "an untagged store may change the header: {optimized}"
    );
}

#[test]
fn branches_to_cold_blocks_carry_branch_weights() {
    let mut module = module();
    let effect = signature(CallConv::C, &[], &[]);
    let slow = module
        .declare_function("slow_path", Linkage::Import, &effect)
        .expect("the import should declare");
    let sig = signature(CallConv::C, &[types::I64], &[]);
    for (name, cold_first) in [
        ("cold_then", true),
        ("cold_else", false),
        ("neither", false),
    ] {
        define(&mut module, name, Linkage::Export, &sig, move |builder| {
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            let flag = builder.block_params(entry)[0];
            let cold = builder.create_block();
            let hot = builder.create_block();
            let done = builder.create_block();
            if name != "neither" {
                builder.set_cold_block(cold);
            }
            if cold_first {
                builder.brif(flag, cold, &[], hot, &[]);
            } else {
                builder.brif(flag, hot, &[], cold, &[]);
            }
            builder.switch_to_block(cold);
            let slow = builder.import_function(slow);
            builder.call(slow, &[]);
            builder.jump(done, &[]);
            builder.switch_to_block(hot);
            builder.jump(done, &[]);
            builder.switch_to_block(done);
            builder.seal_all_blocks();
            builder.return_(&[]);
        });
    }

    let ir = ir(&module);
    assert!(
        ir.contains(r#"!{!"branch_weights", i32 1, i32 2000}"#),
        "{ir}"
    );
    assert!(
        ir.contains(r#"!{!"branch_weights", i32 2000, i32 1}"#),
        "{ir}"
    );
    assert_eq!(ir.matches("!prof").count(), 2, "{ir}");
}

/// The handles of `large_module`.
struct LargeModule {
    module: LlvmModule,
    count: usize,
}

/// A module over the split weight. `count` local functions `step_i`, in
/// order, each add one `length` times, call the small `tiny` and the
/// inline-marked `bump`, read the constant `table`, write the writable
/// `counter`, and call the next step. The exported `entry` calls the first.
fn large_module(count: usize, length: usize) -> LargeModule {
    let mut module = module();
    let unary = signature(CallConv::C, &[types::I64], &[types::I64]);
    let tiny = define(&mut module, "tiny", Linkage::Local, &unary, |builder| {
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let value = builder.block_params(entry)[0];
        let next = builder.iadd_imm(value, 1);
        builder.return_(&[next]);
    });
    // Larger than a copy, but marked for inlining.
    let bump = define(&mut module, "bump", Linkage::Local, &unary, |builder| {
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let mut value = builder.block_params(entry)[0];
        for _ in 0..super::units::COPY_WEIGHT {
            value = builder.bxor_imm(value, 3);
        }
        builder.return_(&[value]);
    });
    module.mark_inline(bump);
    let table = module.declare_data("table", Linkage::Local, false).unwrap();
    module
        .define_data(table, DataDescription::new(7u64.to_le_bytes().to_vec()))
        .unwrap();
    let counter = module
        .declare_data("counter", Linkage::Local, true)
        .unwrap();
    module
        .define_data(counter, DataDescription::new(vec![0; 8]))
        .unwrap();
    let steps = (0..count)
        .map(|index| {
            module
                .declare_function(&format!("step_{index}"), Linkage::Local, &unary)
                .unwrap()
        })
        .collect::<Vec<_>>();
    for (index, step) in steps.iter().enumerate() {
        let next = steps.get(index + 1).copied();
        module
            .define_function(
                *step,
                &unary,
                Box::new(move |builder| {
                    let entry = builder.create_block();
                    builder.append_block_params_for_function_params(entry);
                    builder.switch_to_block(entry);
                    let mut value = builder.block_params(entry)[0];
                    for _ in 0..length {
                        value = builder.iadd_imm(value, 1);
                    }
                    let tiny = builder.import_function(tiny);
                    let bump = builder.import_function(bump);
                    value = builder.call(tiny, &[value]).results[0];
                    value = builder.call(bump, &[value]).results[0];
                    let table = builder.import_data(table);
                    let table = builder.symbol_value(types::I64, table);
                    let read = builder.load(types::I64, MemFlags::trusted(), table, 0);
                    let counter = builder.import_data(counter);
                    let counter = builder.symbol_value(types::I64, counter);
                    builder.store(MemFlags::trusted(), value, counter, 0);
                    value = builder.iadd(value, read);
                    if let Some(next) = next {
                        let next = builder.import_function(next);
                        value = builder.call(next, &[value]).results[0];
                    }
                    builder.return_(&[value]);
                    builder.verify()
                }),
            )
            .unwrap();
    }
    let first = steps[0];
    define(
        &mut module,
        "entry",
        Linkage::Export,
        &unary,
        move |builder| {
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            let value = builder.block_params(entry)[0];
            let first = builder.import_function(first);
            let result = builder.call(first, &[value]).results[0];
            builder.return_(&[result]);
        },
    );
    LargeModule { module, count }
}

/// The index of the declared function `name`.
fn function_index(module: &LlvmModule, name: &str) -> usize {
    match module.names[name] {
        super::Symbol::Function(index) => index as usize,
        super::Symbol::Data(_) => panic!("{name} is data"),
    }
}

/// The index of the declared data object `name`.
fn data_index(module: &LlvmModule, name: &str) -> usize {
    match module.names[name] {
        super::Symbol::Data(index) => index as usize,
        super::Symbol::Function(_) => panic!("{name} is a function"),
    }
}

/// The unit that defines function `name`.
fn defining_unit(partition: &Partition, module: &LlvmModule, name: &str) -> usize {
    let index = function_index(module, name);
    (0..partition.units())
        .find(|unit| {
            matches!(
                partition.function(*unit, index),
                Placement::Private | Placement::Shared
            )
        })
        .unwrap_or_else(|| panic!("no unit defines {name}"))
}

#[test]
fn unit_counts_follow_the_recorded_weight_alone() {
    use super::units::{unit_count, MAX_UNITS, SPLIT_WEIGHT, UNIT_WEIGHT};
    assert_eq!(unit_count(0), 1);
    assert_eq!(unit_count(SPLIT_WEIGHT - 1), 1);
    assert_eq!(
        unit_count(SPLIT_WEIGHT),
        SPLIT_WEIGHT.div_ceil(UNIT_WEIGHT).max(2)
    );
    assert_eq!(unit_count(UNIT_WEIGHT * 5), 5);
    assert_eq!(unit_count(UNIT_WEIGHT * MAX_UNITS * 10), MAX_UNITS);
}

#[test]
fn small_modules_write_the_one_object_finish_writes() {
    let partition = Partition::split(&two_function_module());
    assert_eq!(partition.units(), 1);
    let units = Box::new(two_function_module())
        .finish_units()
        .expect("the objects should write");
    let whole = Box::new(two_function_module())
        .finish()
        .expect("the object should write");
    assert_eq!(units, vec![whole]);
}

#[test]
fn large_modules_split_into_runs_of_consecutive_functions() {
    let LargeModule { module, count } = large_module(40, 250);
    let partition = Partition::split(&module);
    assert!(partition.units() > 2, "{} units", partition.units());
    // Each unit defines one run of functions, in declaration order.
    let units = (0..count)
        .map(|index| defining_unit(&partition, &module, &format!("step_{index}")))
        .collect::<Vec<_>>();
    assert!(units.windows(2).all(|pair| pair[0] <= pair[1]), "{units:?}");
    assert_eq!(units.first(), Some(&0));
    assert_eq!(units.last(), Some(&(partition.units() - 1)));
    // Data belongs to the first unit that reads or writes it.
    for name in ["table", "counter"] {
        let index = data_index(&module, name);
        assert_eq!(partition.data(0, index), Placement::Shared, "{name}");
    }
    // The largest units come first.
    let mut order = partition.units_by_weight();
    assert_eq!(order.len(), partition.units());
    order.sort_unstable();
    assert_eq!(order, (0..partition.units()).collect::<Vec<_>>());
}

#[test]
fn a_function_larger_than_a_unit_leaves_no_empty_unit() {
    let mut module = module();
    let unary = signature(CallConv::C, &[types::I64], &[types::I64]);
    for (name, length) in [
        ("large", super::units::UNIT_WEIGHT * 3),
        ("after", 10),
        ("last", 10),
    ] {
        define(&mut module, name, Linkage::Export, &unary, move |builder| {
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            let mut value = builder.block_params(entry)[0];
            for _ in 0..length {
                value = builder.iadd_imm(value, 1);
            }
            builder.return_(&[value]);
        });
    }
    let partition = Partition::split(&module);
    assert_eq!(partition.units(), 2);
    assert_eq!(defining_unit(&partition, &module, "large"), 0);
    assert_eq!(defining_unit(&partition, &module, "after"), 1);
    assert_eq!(defining_unit(&partition, &module, "last"), 1);
}

#[test]
fn units_reach_each_other_through_hidden_symbols_and_copy_small_code() {
    let LargeModule { module, count } = large_module(40, 250);
    let partition = Partition::split(&module);
    let machine = host_target_machine().expect("the host target machine should exist");
    let last = partition.units() - 1;
    let irs = (0..partition.units())
        .map(|unit| {
            let context = Context::create();
            let lowered = module
                .lower_unit(&context, &machine, &partition, unit)
                .expect("the unit should lower and verify");
            lowered.print_to_string().to_string()
        })
        .collect::<Vec<_>>();
    // The first step of the last unit is called from the unit before it.
    let first_of_last = (0..count)
        .find(|index| defining_unit(&partition, &module, &format!("step_{index}")) == last)
        .unwrap();
    let shared = format!("step_{first_of_last}");
    let caller = format!("step_{}", first_of_last - 1);
    let caller_unit = defining_unit(&partition, &module, &caller);
    assert!(
        irs[last].contains(&format!("define hidden i64 @{shared}(")),
        "{}",
        irs[last]
    );
    assert!(
        irs[caller_unit].contains(&format!("declare hidden i64 @{shared}(")),
        "{}",
        irs[caller_unit]
    );
    // A step that only its own unit calls stays internal.
    let private = format!("step_{}", first_of_last + 1);
    assert!(
        irs[last].contains(&format!("define internal i64 @{private}(")),
        "{}",
        irs[last]
    );
    assert_eq!(
        partition.function(last, function_index(&module, &private)),
        Placement::Private
    );
    // The small function and the inline one are copied into the other
    // units, and constant data too. Writable data never is.
    assert!(irs[0].contains("define hidden i64 @tiny("), "{}", irs[0]);
    assert!(irs[0].contains("@table = hidden constant"), "{}", irs[0]);
    assert!(irs[0].contains("@counter = hidden global"), "{}", irs[0]);
    for ir in &irs[1..] {
        assert!(
            ir.contains("define available_externally hidden i64 @tiny("),
            "{ir}"
        );
        assert!(
            ir.contains("define available_externally hidden i64 @bump("),
            "{ir}"
        );
        assert!(
            ir.contains("@table = available_externally hidden constant"),
            "{ir}"
        );
        assert!(ir.contains("@counter = external hidden global"), "{ir}");
    }
    assert_eq!(
        partition.function(last, function_index(&module, "tiny")),
        Placement::Copy
    );
    assert_eq!(
        partition.data(last, data_index(&module, "counter")),
        Placement::Elsewhere
    );
}

#[test]
fn an_import_without_a_call_ties_no_units_together() {
    let LargeModule { mut module, count } = large_module(40, 250);
    let unary = signature(CallConv::C, &[types::I64], &[types::I64]);
    let steps = (0..count)
        .map(|index| FuncHandle(function_index(&module, &format!("step_{index}")) as u32))
        .collect::<Vec<_>>();
    // The emitter imports every function into every body, called or not.
    define(
        &mut module,
        "importer",
        Linkage::Export,
        &unary,
        move |builder| {
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            for step in &steps {
                builder.import_function(*step);
            }
            let value = builder.block_params(entry)[0];
            builder.return_(&[value]);
        },
    );
    let partition = Partition::split(&module);
    let importer_unit = defining_unit(&partition, &module, "importer");
    assert_ne!(defining_unit(&partition, &module, "step_1"), importer_unit);
    assert_eq!(
        partition.function(0, function_index(&module, "step_1")),
        Placement::Private,
        "only step_0 calls step_1"
    );
    assert_eq!(
        partition.function(importer_unit, function_index(&module, "step_1")),
        Placement::Elsewhere
    );
}

/// Every unit object's symbols, by name: whether it defines each and in
/// which scope.
fn unit_symbols(objects: &[Vec<u8>]) -> Vec<Vec<(String, bool, object::SymbolScope)>> {
    objects
        .iter()
        .map(|bytes| {
            let object = object::File::parse(bytes.as_slice()).expect("the object should parse");
            object
                .symbols()
                .filter_map(|symbol| {
                    let name = symbol.name().ok()?.trim_start_matches('_').to_string();
                    (name == "entry"
                        || name == "tiny"
                        || name == "bump"
                        || name == "table"
                        || name == "counter"
                        || name.starts_with("step"))
                    .then(|| (name, !symbol.is_undefined(), symbol.scope()))
                })
                .collect()
        })
        .collect()
}

#[test]
fn unit_objects_join_at_link_time_whatever_the_thread_count() {
    let LargeModule { module, count } = large_module(40, 250);
    let partition = Partition::split(&module);
    let one_thread = module
        .unit_objects(&partition, 1)
        .expect("the objects should write");
    let four_threads = module
        .unit_objects(&partition, 4)
        .expect("the objects should write");
    assert_eq!(one_thread.len(), partition.units());
    assert_eq!(one_thread, four_threads);
    let finished = Box::new(large_module(count, 250).module)
        .finish_units()
        .expect("the objects should write");
    assert_eq!(finished, one_thread);

    let symbols = unit_symbols(&one_thread);
    let defined = |name: &str| {
        symbols
            .iter()
            .flatten()
            .filter(|(symbol, defined, _)| symbol == name && *defined)
            .map(|(_, _, scope)| *scope)
            .collect::<Vec<_>>()
    };
    assert_eq!(defined("entry"), vec![object::SymbolScope::Dynamic]);
    // Copies are gone: the owner alone defines the small function and the
    // constant data, as hidden symbols the other units reach.
    for name in ["tiny", "table", "counter"] {
        assert_eq!(defined(name), vec![object::SymbolScope::Linkage], "{name}");
    }
    // Each symbol a unit leaves undefined, another unit defines once.
    for (name, _, _) in symbols.iter().flatten().filter(|(_, defined, _)| !defined) {
        assert_eq!(defined(name).len(), 1, "{name}");
    }
}

#[test]
fn unoptimized_unit_objects_keep_every_function() {
    let LargeModule { module, count } = large_module(40, 250);
    let partition = Partition::split(&module);
    let objects = crate::native_backend::without_optimization(|| {
        module
            .unit_objects(&partition, 4)
            .expect("the objects should write")
    });
    let symbols = unit_symbols(&objects);
    for index in 0..count {
        let name = format!("step_{index}");
        assert_eq!(
            symbols
                .iter()
                .flatten()
                .filter(|(symbol, defined, _)| *symbol == name && *defined)
                .count(),
            1,
            "{name}"
        );
    }
    // The inline function's copies are dropped unoptimized too.
    assert_eq!(
        symbols
            .iter()
            .flatten()
            .filter(|(symbol, defined, _)| symbol == "bump" && *defined)
            .count(),
        1
    );
}

#[test]
fn proved_integer_operations_tell_llvm_they_do_not_wrap() {
    let mut module = module();
    let binary = signature(CallConv::C, &[types::I64, types::I64], &[types::I64]);
    define(
        &mut module,
        "no_wrap",
        Linkage::Export,
        &binary,
        move |builder| {
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            let (a, b) = (
                builder.block_params(entry)[0],
                builder.block_params(entry)[1],
            );
            let mut value = a;
            for op in [NoWrapOp::Add, NoWrapOp::Sub, NoWrapOp::Mul] {
                for signed in [true, false] {
                    value = builder.no_wrap(op, signed, value, b);
                }
            }
            builder.return_(&[value]);
        },
    );
    let ir = ir(&module);
    for op in ["add", "sub", "mul"] {
        for flag in ["nsw", "nuw"] {
            assert!(ir.contains(&format!("{op} {flag} i64")), "{ir}");
        }
    }
}

/// A loop over `count` that adds the select of `flag` between 7 and the
/// loaded word at `address` into a variable, entered through a block that
/// tests `flag`. With `escape`, the block after the loop uses a value the
/// loop defines. The flag says whether the loop got its copy.
fn define_versioned_loop(
    module: &mut LlvmModule,
    name: &str,
    escape: bool,
) -> std::rc::Rc<std::cell::Cell<bool>> {
    let sig = signature(
        CallConv::C,
        &[types::I64, types::I64, types::I64],
        &[types::I64],
    );
    let copied = std::rc::Rc::new(std::cell::Cell::new(false));
    let result = std::rc::Rc::clone(&copied);
    define(module, name, Linkage::Export, &sig, move |builder| {
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let [count, flag, address] = builder.block_params(entry)[..] else {
            unreachable!("three parameters")
        };
        let total = Var(0);
        builder.declare_var(total, types::I64);
        let header = builder.create_block();
        let index = builder.append_block_param(header, types::I64);
        let body = builder.create_block();
        let exit = builder.create_block();
        let zero = builder.iconst(types::I64, 0);
        builder.def_var(total, zero);
        builder.jump(header, &[zero]);

        builder.switch_to_block(header);
        let more = builder.icmp(IntCC::SignedLessThan, index, count);
        builder.brif(more, body, &[], exit, &[]);

        builder.switch_to_block(body);
        builder.seal_block(body);
        let seven = builder.iconst(types::I64, 7);
        let loaded = builder.load(types::I64, MemFlags::trusted(), address, 0);
        let holds = builder.icmp_imm(IntCC::NotEqual, flag, 0);
        let select = builder.select(holds, seven, loaded);
        let sum = builder.use_var(total);
        let added = builder.iadd(sum, select);
        builder.def_var(total, added);
        let next = builder.iadd_imm(index, 1);
        builder.jump(header, &[next]);
        builder.seal_block(header);

        builder.switch_to_block(exit);
        builder.seal_block(exit);
        let returned = if escape {
            builder.uextend(types::I64, more)
        } else {
            builder.use_var(total)
        };
        builder.return_(&[returned]);
        result.set(builder.version_blocks(
            &[header, body],
            &[(select, None)],
            &[(entry, flag, Some((flag, 1)))],
        ));
    });
    copied
}

#[test]
fn versioned_blocks_get_a_copy_that_keeps_the_true_side_of_their_selects() {
    let mut module = module();
    let copied = define_versioned_loop(&mut module, "versioned", false);
    let ir = ir(&module);
    assert!(copied.get(), "{ir}");
    // The entry tests the flag and that it equals 1 before it picks a copy.
    assert!(ir.contains("icmp eq i64"), "{ir}");
    // The copy adds 7 and the original the load, with no select left.
    assert!(!ir.contains("select"), "{ir}");
    assert!(ir.contains(", 7\n"), "{ir}");
    assert_eq!(ir.matches("icmp slt i64").count(), 2, "{ir}");
    // The entry favors the copy, so its values keep the registers.
    assert!(
        ir.contains("!{!\"branch_weights\", i32 2000, i32 1}"),
        "{ir}"
    );
}

#[test]
fn versioned_blocks_keep_the_order_their_instructions_were_built_in() {
    // The latch is created before the body and built after it, and it uses
    // a value of the body. Its copy must follow the body's copy.
    let mut module = module();
    let sig = signature(CallConv::C, &[types::I64, types::I64], &[types::I64]);
    let copied = std::rc::Rc::new(std::cell::Cell::new(false));
    let result = std::rc::Rc::clone(&copied);
    define(&mut module, "late", Linkage::Export, &sig, move |builder| {
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let [count, flag] = builder.block_params(entry)[..] else {
            unreachable!("two parameters")
        };
        let header = builder.create_block();
        let index = builder.append_block_param(header, types::I64);
        let latch = builder.create_block();
        let body = builder.create_block();
        let exit = builder.create_block();
        let zero = builder.iconst(types::I64, 0);
        builder.jump(header, &[zero]);

        builder.switch_to_block(header);
        let more = builder.icmp(IntCC::SignedLessThan, index, count);
        builder.brif(more, body, &[], exit, &[]);

        builder.switch_to_block(body);
        builder.seal_block(body);
        let next = builder.iadd_imm(index, 1);
        builder.jump(latch, &[]);

        builder.switch_to_block(latch);
        builder.seal_block(latch);
        builder.jump(header, &[next]);
        builder.seal_block(header);

        builder.switch_to_block(exit);
        builder.seal_block(exit);
        builder.return_(&[count]);
        result.set(builder.version_blocks(&[header, latch, body], &[], &[(entry, flag, None)]));
    });
    assert!(copied.get());
    // The module lowers, which fails on a value used before its block.
    let ir = ir(&module);
    assert_eq!(ir.matches("icmp slt i64").count(), 2, "{ir}");
}

#[test]
fn element_addresses_are_offsets_from_one_pointer() {
    let mut module = module();
    let sig = signature(CallConv::C, &[types::I64, types::I64], &[types::I64]);
    define(&mut module, "element", Linkage::Export, &sig, |builder| {
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let [base, index] = builder.block_params(entry)[..] else {
            unreachable!("two parameters")
        };
        let offset = builder.imul_imm(index, 8);
        let address = builder.element_address(base, offset);
        let element = builder.load(types::I64, MemFlags::trusted(), address, 0);
        // The address is still a word a caller can pass on.
        let sum = builder.iadd(element, address);
        builder.return_(&[sum]);
    });
    let ir = ir(&module);
    // The access indexes from the base as a pointer, which LLVM's loop
    // passes follow, and the word is the plain sum.
    assert!(ir.contains("getelementptr i8, ptr"), "{ir}");
    assert_eq!(ir.matches("inttoptr").count(), 1, "{ir}");
}

#[test]
fn square_roots_are_the_llvm_intrinsic() {
    let mut module = module();
    let sig = signature(CallConv::C, &[types::F64], &[types::F64]);
    define(&mut module, "root", Linkage::Export, &sig, |builder| {
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let value = builder.block_params(entry)[0];
        let root = builder.fsqrt(value);
        builder.return_(&[root]);
    });
    let ir = ir(&module);
    assert!(ir.contains("call double @llvm.sqrt.f64(double"), "{ir}");
}

#[test]
fn blocks_whose_values_are_used_after_them_keep_one_copy_and_the_false_side() {
    let mut module = module();
    let copied = define_versioned_loop(&mut module, "escaping", true);
    let ir = ir(&module);
    assert!(!copied.get(), "{ir}");
    assert!(!ir.contains("select"), "{ir}");
    assert!(!ir.contains("icmp eq i64"), "{ir}");
    assert_eq!(ir.matches("icmp slt i64").count(), 1, "{ir}");
}

#[test]
fn foreign_symbols_declare_once_and_calls_extend_narrow_arguments() {
    let mut module = module();
    let foreign = module
        .declare_foreign_symbol("toupper")
        .expect("a new foreign symbol should declare");
    assert_eq!(module.declare_foreign_symbol("toupper"), Ok(foreign));
    assert_eq!(
        module.functions[foreign.0 as usize].linkage,
        Linkage::Import
    );

    // A name the module already gives a function resolves to that function,
    // whatever its signature, and a name it gives data is refused.
    let sig = signature(CallConv::C, &[types::I64], &[types::I64]);
    let known = module
        .declare_function("known", Linkage::Import, &sig)
        .unwrap();
    assert_eq!(module.declare_foreign_symbol("known"), Ok(known));
    module.declare_data("table", Linkage::Local, false).unwrap();
    assert_eq!(
        module.declare_foreign_symbol("table"),
        Err("Incompatible declaration of identifier: table".to_string())
    );

    let caller = signature(CallConv::C, &[types::I64], &[types::I64]);
    define(
        &mut module,
        "caller",
        Linkage::Export,
        &caller,
        move |builder| {
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            let word = builder.block_params(entry)[0];
            let callee = builder.import_function(foreign);
            let address = builder.func_addr(types::I64, callee);
            let narrow = builder.ireduce(types::I32, word);
            let byte = builder.ireduce(types::I8, word);
            let mut foreign_signature = signature(
                CallConv::C,
                &[types::I32, types::I8, types::I64],
                &[types::I32],
            );
            foreign_signature.extensions = vec![Extension::Sign, Extension::Zero, Extension::None];
            let foreign_signature = builder.import_signature(foreign_signature);
            let result = builder.call_indirect(foreign_signature, address, &[narrow, byte, word]);
            let wide = builder.uextend(types::I64, result.results[0]);
            builder.return_(&[wide]);
        },
    );

    let ir = ir(&module);
    assert!(ir.contains("declare void @toupper()"), "{ir}");
    assert!(
        ir.contains("(i32 signext %") && ir.contains(", i8 zeroext %"),
        "narrow C arguments carry their extension: {ir}"
    );
}

/// A local `callee` that adds one, `extra` times over, and an exported
/// `looper` whose loop calls it once and has a copy.
fn define_copied_call(module: &mut LlvmModule, extra: usize, recursive: bool) -> FuncHandle {
    let sig = signature(CallConv::C, &[types::I64], &[types::I64]);
    let callee = module
        .declare_function("callee", Linkage::Local, &sig)
        .expect("the callee should declare");
    module
        .define_function(
            callee,
            &sig,
            Box::new(move |builder| {
                let entry = builder.create_block();
                builder.append_block_params_for_function_params(entry);
                builder.switch_to_block(entry);
                let mut word = builder.block_params(entry)[0];
                for _ in 0..=extra {
                    word = builder.iadd_imm(word, 1);
                }
                if recursive {
                    let this = builder.import_function(callee);
                    word = builder.call(this, &[word]).results[0];
                }
                builder.return_(&[word]);
                builder.verify()
            }),
        )
        .expect("the callee should define");

    let looper = signature(CallConv::C, &[types::I64, types::I64], &[types::I64]);
    define(module, "looper", Linkage::Export, &looper, move |builder| {
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let [count, flag] = builder.block_params(entry)[..] else {
            unreachable!("two parameters")
        };
        let total = Var(0);
        builder.declare_var(total, types::I64);
        let header = builder.create_block();
        let index = builder.append_block_param(header, types::I64);
        let body = builder.create_block();
        let exit = builder.create_block();
        let zero = builder.iconst(types::I64, 0);
        builder.def_var(total, zero);
        builder.jump(header, &[zero]);

        builder.switch_to_block(header);
        let more = builder.icmp(IntCC::SignedLessThan, index, count);
        builder.brif(more, body, &[], exit, &[]);

        builder.switch_to_block(body);
        builder.seal_block(body);
        let seven = builder.iconst(types::I64, 7);
        let holds = builder.icmp_imm(IntCC::NotEqual, flag, 0);
        let select = builder.select(holds, seven, index);
        let target = builder.import_function(callee);
        let called = builder.call(target, &[select]).results[0];
        let sum = builder.use_var(total);
        let added = builder.iadd(sum, called);
        builder.def_var(total, added);
        let next = builder.iadd_imm(index, 1);
        builder.jump(header, &[next]);
        builder.seal_block(header);

        builder.switch_to_block(exit);
        builder.seal_block(exit);
        let returned = builder.use_var(total);
        builder.return_(&[returned]);
        assert!(builder.version_blocks(
            &[header, body],
            &[(select, None)],
            &[(entry, flag, Some((flag, 1)))],
        ));
    });
    callee
}

/// A function named `name` that calls `callee` once, or takes its address.
fn define_user_of(
    module: &mut LlvmModule,
    name: &str,
    linkage: Linkage,
    callee: FuncHandle,
    address: bool,
) {
    let sig = signature(CallConv::C, &[types::I64], &[types::I64]);
    define(module, name, linkage, &sig, move |builder| {
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let word = builder.block_params(entry)[0];
        let target = builder.import_function(callee);
        let result = if address {
            builder.func_addr(types::I64, target)
        } else {
            builder.call(target, &[word]).results[0]
        };
        builder.return_(&[result]);
    });
}

#[test]
fn a_function_whose_one_call_a_loop_copy_doubled_is_inlined() {
    let mut module = module();
    let callee = define_copied_call(&mut module, 0, false);
    // A thunk nothing reaches does not count as a second call.
    define_user_of(&mut module, "thunk", Linkage::Local, callee, false);
    assert!(!module.functions[callee.0 as usize].inline);
    module.inline_calls_copied_with_their_loops();
    assert!(module.functions[callee.0 as usize].inline);

    let before = ir(&module);
    assert_eq!(before.matches("call i64 @callee").count(), 3, "{before}");
    let after = optimized_ir(&module);
    assert!(!after.contains("@callee"), "{after}");
}

#[test]
fn a_copied_call_is_not_inlined_when_its_function_has_other_uses() {
    // A second call from a function the program reaches.
    let mut called_twice = module();
    let callee = define_copied_call(&mut called_twice, 0, false);
    define_user_of(&mut called_twice, "other", Linkage::Export, callee, false);
    called_twice.inline_calls_copied_with_their_loops();
    assert!(!called_twice.functions[callee.0 as usize].inline);

    // A function that takes its address.
    let mut address_taken = module();
    let callee = define_copied_call(&mut address_taken, 0, false);
    define_user_of(&mut address_taken, "other", Linkage::Export, callee, true);
    address_taken.inline_calls_copied_with_their_loops();
    assert!(!address_taken.functions[callee.0 as usize].inline);

    // A data object that holds its address.
    let mut in_data = module();
    let callee = define_copied_call(&mut in_data, 0, false);
    let table = in_data
        .declare_data("table", Linkage::Local, false)
        .unwrap();
    let mut contents = DataDescription::new(vec![0; 8]);
    contents.write_function_addr(0, callee);
    in_data.define_data(table, contents).unwrap();
    in_data.inline_calls_copied_with_their_loops();
    assert!(!in_data.functions[callee.0 as usize].inline);

    // A function that calls itself.
    let mut recursive = module();
    let callee = define_copied_call(&mut recursive, 0, true);
    recursive.inline_calls_copied_with_their_loops();
    assert!(!recursive.functions[callee.0 as usize].inline);

    // A function over the weight.
    let mut large = module();
    let callee = define_copied_call(&mut large, super::COPIED_CALL_INLINE_WEIGHT, false);
    large.inline_calls_copied_with_their_loops();
    assert!(!large.functions[callee.0 as usize].inline);

    // A debug build, which keeps every function.
    let mut debug = module();
    let callee = define_copied_call(&mut debug, 0, false);
    debug.enable_debug_info();
    debug.inline_calls_copied_with_their_loops();
    assert!(!debug.functions[callee.0 as usize].inline);
}

#[test]
fn a_call_no_loop_copied_leaves_its_function_to_llvm() {
    let mut module = module();
    let sig = signature(CallConv::C, &[types::I64], &[types::I64]);
    let callee = define(&mut module, "callee", Linkage::Local, &sig, |builder| {
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let word = builder.block_params(entry)[0];
        builder.return_(&[word]);
    });
    define_user_of(&mut module, "caller", Linkage::Export, callee, false);
    module.inline_calls_copied_with_their_loops();
    assert!(!module.functions[callee.0 as usize].inline);
}

#[test]
fn lane_checks_join_from_avx2_on() {
    use crate::native_backend::{joins_lane_checks, lane_checks_join_on, with_joined_lane_checks};

    let host = |features: &'static str| move || format!("native:cpu:{features}");
    assert!(lane_checks_join_on("x86_64", "x86-64-v3", host("")));
    assert!(lane_checks_join_on("x86_64", "x86-64-v4", host("")));
    assert!(lane_checks_join_on(
        "x86_64",
        "native",
        host("+sse2,+avx2,-avx512f")
    ));
    assert!(!lane_checks_join_on(
        "x86_64",
        "native",
        host("+sse2,-avx2")
    ));
    assert!(!lane_checks_join_on("x86_64", "generic", host("+avx2")));
    assert!(!lane_checks_join_on("x86_64", "x86-64-v2", host("+avx2")));
    // Other architectures are not measured.
    assert!(!lane_checks_join_on("aarch64", "native", host("+avx2")));
    // The default target CPU of a test is the generic one.
    assert!(!joins_lane_checks());
    assert!(with_joined_lane_checks(joins_lane_checks));
    assert!(!joins_lane_checks());
}

#[test]
fn target_cpu_names_follow_the_architecture() {
    use crate::native_backend::{target_cpu_names, target_cpu_names_for};

    assert_eq!(target_cpu_names_for("aarch64"), ["generic", "native"]);
    assert_eq!(
        target_cpu_names_for("x86_64"),
        ["generic", "native", "x86-64-v2", "x86-64-v3", "x86-64-v4"]
    );
    assert_eq!(
        target_cpu_names(),
        target_cpu_names_for(std::env::consts::ARCH)
    );
}

#[test]
fn target_cpu_is_checked_and_names_the_host_for_native() {
    use crate::native_backend::{set_target_cpu, target_cpu, target_cpu_identity};

    let error = set_target_cpu("pentium-9").expect_err("an unknown CPU should be refused");
    assert!(
        error.starts_with("unknown target CPU `pentium-9`; this host accepts `generic`, `native`"),
        "{error}"
    );
    assert_eq!(target_cpu(), "generic");
    assert_eq!(target_cpu_identity(), "generic");

    // Other tests compile objects while this one runs. A `native` object
    // runs on this machine, so the window is harmless to them.
    set_target_cpu("native").expect("`native` is accepted on every host");
    let name = target_cpu();
    let identity = target_cpu_identity();
    let machine = host_target_machine();
    set_target_cpu("generic").expect("`generic` is accepted on every host");

    assert_eq!(name, "native");
    assert!(identity.starts_with("native:"), "{identity}");
    assert_eq!(identity, super::host_cpu_identity());
    let machine = machine.expect("the host CPU should make a machine");
    assert_eq!(
        machine.get_cpu().to_string_lossy(),
        inkwell::targets::TargetMachine::get_host_cpu_name().to_string_lossy()
    );
    assert_eq!(target_cpu(), "generic");
}

#[test]
fn optimized_ir_is_written_beside_the_named_prefix() {
    let context = Context::create();
    let module = context.create_module("dumped");
    let function = module.add_function("answer", context.i64_type().fn_type(&[], false), None);
    let builder = context.create_builder();
    builder.position_at_end(context.append_basic_block(function, "entry"));
    builder
        .build_return(Some(&context.i64_type().const_int(42, false)))
        .expect("the return should build");

    let directory = std::env::temp_dir().join(format!("aura-ir-dump-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("the directory should be created");
    let prefix = directory.join("program");
    super::dump_unit_ir(&module, prefix.clone().into_os_string(), 3)
        .expect("the IR should be written");
    let text = std::fs::read_to_string(directory.join("program.3.ll"))
        .expect("the unit's IR file should exist");
    assert!(text.contains("define i64 @answer()"), "{text}");

    let missing = directory.join("absent").join("program");
    let error = super::dump_unit_ir(&module, missing.into_os_string(), 0)
        .expect_err("a missing directory should fail");
    assert!(error.starts_with("failed to write LLVM IR"), "{error}");
    std::fs::remove_dir_all(&directory).expect("the directory should be removed");
}
