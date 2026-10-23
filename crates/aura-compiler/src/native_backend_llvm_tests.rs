use cranelift_object::object::{Object, ObjectSection, ObjectSymbol};
use inkwell::context::Context;

use super::{host_target_machine, LlvmModule};
use crate::native_backend::{
    types, CallConv, DataDescription, DefineError, FuncHandle, FunctionBackend, IntCC, Linkage,
    ModuleBackend, ScalarType, Signature, TrapCode, Var,
};

fn signature(call_conv: CallConv, params: &[ScalarType], returns: &[ScalarType]) -> Signature {
    Signature {
        call_conv,
        params: params.to_vec(),
        returns: returns.to_vec(),
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
fn use_var_follows_sealed_single_predecessors_like_cranelift() {
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
fn declarations_merge_like_cranelift_and_definitions_happen_once() {
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
    let object =
        cranelift_object::object::File::parse(bytes.as_slice()).expect("the object should parse");
    assert!(object
        .symbols()
        .any(|symbol| symbol.name().is_ok_and(|name| name.ends_with("exported"))));
    if cfg!(target_os = "linux") {
        assert!(object
            .sections()
            .any(|section| section.name() == Ok(".eh_frame")));
    }
}
