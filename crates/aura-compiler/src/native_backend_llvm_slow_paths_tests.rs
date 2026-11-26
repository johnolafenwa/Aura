use inkwell::context::Context;

use super::super::{host_target_machine, LlvmModule, Op};
use crate::native_backend::{
    types, CallConv, FuncHandle, FunctionBackend, IntCC, Linkage, ModuleBackend, ScalarType,
    Signature, Var,
};

fn signature(params: &[ScalarType], returns: &[ScalarType]) -> Signature {
    Signature {
        call_conv: CallConv::C,
        params: params.to_vec(),
        returns: returns.to_vec(),
        extensions: Vec::new(),
        indirects: Vec::new(),
    }
}

/// Defines `name` in `module` with `body`, optimized or not.
fn define(
    module: &mut LlvmModule,
    name: &str,
    optimized: bool,
    body: impl FnOnce(&mut dyn FunctionBackend, FuncHandle) + 'static,
) -> FuncHandle {
    let slow = module
        .declare_function(
            "slow",
            Linkage::Import,
            &signature(&[types::I64], &[types::I64]),
        )
        .expect("the import should declare");
    let sig = signature(&[types::I64], &[types::I64]);
    let function = module
        .declare_function(name, Linkage::Export, &sig)
        .expect("the function should declare");
    let define = |module: &mut LlvmModule| {
        module
            .define_function(
                function,
                &sig,
                Box::new(move |builder| {
                    body(builder, slow);
                    builder.verify()
                }),
            )
            .expect("the function should define");
    };
    if optimized {
        define(module);
    } else {
        crate::native_backend::without_optimization(|| define(module));
    }
    function
}

/// The module lowered and verified by LLVM, which checks that every value
/// is defined on each way to its uses.
fn lowered(module: &LlvmModule) -> String {
    let machine = host_target_machine().expect("the host target machine should exist");
    let context = Context::create();
    let lowered = module
        .lower(&context, &machine)
        .expect("the module should lower and verify");
    lowered.print_to_string().to_string()
}

fn body(module: &LlvmModule, function: FuncHandle) -> &super::super::FunctionIr {
    module.functions[function.0 as usize]
        .body
        .as_ref()
        .expect("the function has a body")
}

/// A check whose cold way calls `slow` and rejoins, after which the join
/// uses `kept`, a value the entry's successor computed before the check.
fn checked_body(builder: &mut dyn FunctionBackend, slow: FuncHandle) {
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    let start = builder.create_block();
    let fast = builder.create_block();
    let cold = builder.create_block();
    let join = builder.create_block();
    let joined = builder.append_block_param(join, types::I64);
    let colder = builder.create_block();
    let carried = builder.append_block_param(colder, types::I64);
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    let input = builder.block_params(entry)[0];
    builder.jump(start, &[]);
    builder.switch_to_block(start);
    builder.seal_block(start);
    let kept = builder.iadd_imm(input, 3);
    let small = builder.icmp_imm(IntCC::SignedLessThan, kept, 10);
    builder.set_cold_block(cold);
    builder.brif(small, fast, &[], cold, &[]);
    builder.switch_to_block(fast);
    builder.seal_block(fast);
    builder.jump(join, &[kept]);
    builder.switch_to_block(cold);
    builder.seal_block(cold);
    let callee = builder.import_function(slow);
    let called = builder.call(callee, &[kept]).results[0];
    builder.set_cold_block(colder);
    builder.jump(colder, &[called]);
    // A cold way of two blocks leaves the copy from its last.
    builder.switch_to_block(colder);
    builder.seal_block(colder);
    builder.jump(join, &[carried]);
    builder.switch_to_block(join);
    builder.seal_block(join);
    let sum = builder.iadd(joined, kept);
    let twice = builder.iadd(sum, kept);
    builder.return_(&[twice]);
}

#[test]
fn a_cold_way_continues_in_the_body_as_built_and_the_function_runs_the_copy() {
    let mut module = LlvmModule::for_host().expect("the host should have an LLVM target");
    let function = define(&mut module, "checked", true, checked_body);
    let ir = body(&module, function);
    assert!(ir.slow_copy_insts > 0, "{}", ir.display());
    // The entry jumps into the copy, whose cold way jumps back to the
    // first join: blocks 1 to 5 are the body as built.
    let entry = &ir.blocks[ir.layout[0] as usize];
    let Op::Jump(start, _) = &ir.insts[*entry.insts.last().unwrap() as usize].op else {
        panic!("the entry jumps: {}", ir.display());
    };
    assert!(start.0 > 5, "{}", ir.display());
    let exits = ir
        .layout
        .iter()
        .filter(|block| **block > 5 && ir.blocks[**block as usize].cold)
        .flat_map(|block| ir.terminator_targets(*block))
        .collect::<Vec<_>>();
    assert!(
        exits.contains(&4) && !exits.contains(&3) && !exits.contains(&5),
        "{}",
        ir.display()
    );
    // `kept` is used at the first join, which the copy's cold way reaches
    // without the first body's definition, so it goes through a variable.
    assert!(
        ir.insts.iter().any(|inst| matches!(inst.op, Op::UseVar(_))),
        "{}",
        ir.display()
    );
    // The copy weighs nothing for splitting the module.
    let weight = super::super::units::function_weight(&module.functions[function.0 as usize]);
    assert_eq!(weight, ir.insts.len() - ir.slow_copy_insts);
    let lowered = lowered(&module);
    assert!(lowered.contains("@checked"), "{lowered}");
}

#[test]
fn a_body_without_optimization_or_without_a_cold_rejoin_keeps_one_copy() {
    let mut module = LlvmModule::for_host().expect("the host should have an LLVM target");
    let unoptimized = define(&mut module, "checked", false, checked_body);
    assert_eq!(body(&module, unoptimized).slow_copy_insts, 0);
    let mut module = LlvmModule::for_host().expect("the host should have an LLVM target");
    let plain = define(&mut module, "plain", true, |builder, _| {
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let input = builder.block_params(entry)[0];
        builder.return_(&[input]);
    });
    assert_eq!(body(&module, plain).slow_copy_insts, 0);
    lowered(&module);
}

#[test]
fn a_loop_of_the_first_body_goes_back_to_the_copy() {
    let mut module = LlvmModule::for_host().expect("the host should have an LLVM target");
    let function = define(&mut module, "looped", true, |builder, slow| {
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        let header = builder.create_block();
        let count = builder.append_block_param(header, types::I64);
        let fast = builder.create_block();
        let cold = builder.create_block();
        let latch = builder.create_block();
        let exit = builder.create_block();
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let input = builder.block_params(entry)[0];
        builder.jump(header, &[input]);
        builder.switch_to_block(header);
        let odd = builder.band_imm(count, 1);
        builder.set_cold_block(cold);
        builder.brif(odd, cold, &[], fast, &[]);
        builder.switch_to_block(fast);
        builder.seal_block(fast);
        builder.jump(latch, &[]);
        builder.switch_to_block(cold);
        builder.seal_block(cold);
        let callee = builder.import_function(slow);
        builder.call(callee, &[count]);
        builder.jump(latch, &[]);
        builder.switch_to_block(latch);
        builder.seal_block(latch);
        let next = builder.iadd_imm(count, -1);
        let more = builder.icmp_imm(IntCC::SignedGreaterThan, next, 0);
        builder.brif(more, header, &[next], exit, &[]);
        builder.seal_block(header);
        builder.switch_to_block(exit);
        builder.seal_block(exit);
        builder.return_(&[next]);
    });
    let ir = body(&module, function);
    assert!(ir.slow_copy_insts > 0, "{}", ir.display());
    // The latch of the first body (block 4) branches back to the copy's
    // header, not to block 1.
    let targets = ir.terminator_targets(4);
    assert!(
        !targets.contains(&1) && targets.iter().any(|target| *target > 5),
        "{}",
        ir.display()
    );
    lowered(&module);
}

/// A deferred frame whose push a cold call needs, and a block parameter
/// that the cold way's join returns.
fn framed_body(builder: &mut dyn FunctionBackend, slow: FuncHandle) {
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    let start = builder.create_block();
    let carried = builder.append_block_param(start, types::I64);
    let cold = builder.create_block();
    let fast = builder.create_block();
    let join = builder.create_block();
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    let input = builder.block_params(entry)[0];
    let framed = Var(0);
    builder.declare_var(framed, types::I64);
    let zero = builder.iconst(types::I64, 0);
    builder.def_var(framed, zero);
    builder.jump(start, &[input]);
    builder.switch_to_block(start);
    builder.seal_block(start);
    let callee = builder.import_function(slow);
    let descriptor = builder.iconst(types::I64, 7);
    builder.defer_frame(framed, descriptor, callee, &[]);
    builder.set_cold_block(cold);
    builder.brif(carried, cold, &[], fast, &[]);
    builder.switch_to_block(fast);
    builder.seal_block(fast);
    builder.jump(join, &[]);
    builder.switch_to_block(cold);
    builder.seal_block(cold);
    // The call pushes the deferred frame first, in a cold block of
    // its own that rejoins this one.
    let other = builder.import_function(slow);
    builder.call(other, &[carried]);
    builder.jump(join, &[]);
    builder.switch_to_block(join);
    builder.seal_block(join);
    // `carried` is the first body's block parameter, which the copy's
    // cold way leaves undefined here.
    builder.return_(&[carried]);
}

#[test]
fn a_frame_push_rejoins_in_the_copy_and_a_block_parameter_goes_through_a_variable() {
    let mut module = LlvmModule::for_host().expect("the host should have an LLVM target");
    let function = define(&mut module, "framed", true, framed_body);
    let ir = body(&module, function);
    assert!(ir.slow_copy_insts > 0, "{}", ir.display());
    // The copy and its frame instructions weigh nothing.
    let mut unoptimized = LlvmModule::for_host().expect("the host should have an LLVM target");
    let built = define(&mut unoptimized, "framed", false, framed_body);
    assert_eq!(ir.weight(), body(&unoptimized, built).weight());
    let defined = ir
        .insts
        .iter()
        .filter(|inst| matches!(inst.op, Op::DefVar(var, _) if var.0 > 0))
        .count();
    assert!(defined >= 2, "{}", ir.display());
    // The push block of each body rejoins its own body.
    let members = (ir.layout.len() - 1) / 2;
    let first_copy = ir.layout[1 + members];
    let pushes = ir
        .layout
        .iter()
        .filter(|block| {
            let insts = &ir.blocks[**block as usize].insts;
            insts.iter().all(|inst| ir.frame_insts.contains(inst))
                && insts
                    .iter()
                    .any(|inst| matches!(ir.insts[*inst as usize].op, Op::Call(..)))
        })
        .collect::<Vec<_>>();
    assert_eq!(pushes.len(), 2, "{}", ir.display());
    for block in pushes {
        for target in ir.terminator_targets(*block) {
            assert_eq!(
                target >= first_copy,
                *block >= first_copy,
                "{}",
                ir.display()
            );
        }
    }
    lowered(&module);
}

#[test]
fn an_empty_body_keeps_one_copy() {
    let mut ir = super::super::FunctionIr::new(signature(&[], &[]));
    assert!(!ir.continue_slow_paths());
}

#[test]
fn a_body_past_the_size_limit_keeps_one_copy() {
    let mut module = LlvmModule::for_host().expect("the host should have an LLVM target");
    let function = define(&mut module, "large", true, |builder, slow| {
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        let fast = builder.create_block();
        let cold = builder.create_block();
        let join = builder.create_block();
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let mut value = builder.block_params(entry)[0];
        for _ in 0..super::MOST_INSTRUCTIONS {
            value = builder.iadd_imm(value, 1);
        }
        builder.set_cold_block(cold);
        builder.brif(value, cold, &[], fast, &[]);
        builder.switch_to_block(fast);
        builder.seal_block(fast);
        builder.jump(join, &[]);
        builder.switch_to_block(cold);
        builder.seal_block(cold);
        let callee = builder.import_function(slow);
        builder.call(callee, &[value]);
        builder.jump(join, &[]);
        builder.switch_to_block(join);
        builder.seal_block(join);
        builder.return_(&[value]);
    });
    assert_eq!(body(&module, function).slow_copy_insts, 0);
}

#[test]
fn a_loop_no_slow_way_leaves_keeps_its_back_edge_in_each_body() {
    let mut module = LlvmModule::for_host().expect("the host should have an LLVM target");
    let function = define(&mut module, "counted", true, |builder, slow| {
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        let fast = builder.create_block();
        let cold = builder.create_block();
        let header = builder.create_block();
        let count = builder.append_block_param(header, types::I64);
        let exit = builder.create_block();
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let input = builder.block_params(entry)[0];
        builder.set_cold_block(cold);
        builder.brif(input, cold, &[], fast, &[]);
        builder.switch_to_block(fast);
        builder.seal_block(fast);
        let start = builder.iconst(types::I64, 15);
        builder.jump(header, &[start]);
        builder.switch_to_block(cold);
        builder.seal_block(cold);
        let callee = builder.import_function(slow);
        let called = builder.call(callee, &[input]).results[0];
        builder.jump(header, &[called]);
        // The loop has no cold block: block 3 branches back to itself.
        builder.switch_to_block(header);
        let next = builder.iadd_imm(count, -1);
        let more = builder.icmp_imm(IntCC::SignedGreaterThan, next, 0);
        builder.brif(more, header, &[next], exit, &[]);
        builder.seal_block(header);
        builder.switch_to_block(exit);
        builder.seal_block(exit);
        builder.return_(&[next]);
    });
    let ir = body(&module, function);
    assert!(ir.slow_copy_insts > 0, "{}", ir.display());
    assert!(ir.terminator_targets(3).contains(&3), "{}", ir.display());
    lowered(&module);
}

#[test]
fn a_call_the_copy_repeats_leaves_its_function_to_llvm() {
    let mut module = LlvmModule::for_host().expect("the host should have an LLVM target");
    let sig = signature(&[types::I64], &[types::I64]);
    let callee = module
        .declare_function("callee", Linkage::Local, &sig)
        .expect("the callee should declare");
    module
        .define_function(
            callee,
            &sig,
            Box::new(|builder| {
                let entry = builder.create_block();
                builder.append_block_params_for_function_params(entry);
                builder.switch_to_block(entry);
                builder.seal_block(entry);
                let word = builder.block_params(entry)[0];
                builder.return_(&[word]);
                builder.verify()
            }),
        )
        .expect("the callee should define");
    let caller = module
        .declare_function("caller", Linkage::Export, &sig)
        .expect("the caller should declare");
    module
        .define_function(
            caller,
            &sig,
            Box::new(move |builder| {
                checked_body(builder, callee);
                builder.verify()
            }),
        )
        .expect("the caller should define");
    // The copy calls the callee too, but the call stands for the one the
    // first body makes.
    let calls = body(&module, caller)
        .insts
        .iter()
        .filter(|inst| matches!(inst.op, Op::Call(..)))
        .count();
    assert_eq!(calls, 2);
    module.inline_calls_copied_with_their_loops();
    assert!(!module.functions[callee.0 as usize].inline);
}
