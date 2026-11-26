use inkwell::context::Context;

use super::super::{host_target_machine, LlvmModule, Op};
use crate::native_backend::{
    types, AliasClass, CallConv, FuncHandle, FunctionBackend, IntCC, Linkage, MemFlags,
    MemoryAccess, ModuleBackend, ScalarType, Signature, Val, Var,
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
    // The check reads a word of `input`, which the join reads again.
    let address = builder.iadd_imm(input, 8);
    builder.load(types::I64, access(), address, 0);
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
    let again = builder.load(types::I64, access(), input, 8);
    let twice = builder.iadd(sum, again);
    builder.return_(&[twice]);
}

/// The flags of a load of a list view's access code.
fn access() -> MemFlags {
    MemFlags::trusted_in(AliasClass::BoxAccess)
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
        builder.load(types::I64, access(), input, 0);
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
        builder.load(types::I64, access(), input, 0);
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
    builder.load(types::I64, access(), carried, 0);
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
    builder.load(types::I64, access(), carried, 0);
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
        builder.load(types::I64, access(), input, 0);
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
        builder.load(types::I64, access(), input, 0);
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

/// A check that reads the word at `input + 8` with `before`, whose cold
/// way calls `slow` and rejoins at a join that runs `between` and reads
/// the word again with `after`.
fn reread_body(
    builder: &mut dyn FunctionBackend,
    slow: FuncHandle,
    before: MemFlags,
    after: MemFlags,
    between: &dyn Fn(&mut dyn FunctionBackend, Val, FuncHandle),
) {
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    let fast = builder.create_block();
    let cold = builder.create_block();
    let join = builder.create_block();
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    let input = builder.block_params(entry)[0];
    let word = builder.load(types::I64, before, input, 8);
    builder.set_cold_block(cold);
    builder.brif(word, fast, &[], cold, &[]);
    builder.switch_to_block(fast);
    builder.seal_block(fast);
    builder.jump(join, &[]);
    builder.switch_to_block(cold);
    builder.seal_block(cold);
    let callee = builder.import_function(slow);
    builder.call(callee, &[input]);
    builder.jump(join, &[]);
    builder.switch_to_block(join);
    builder.seal_block(join);
    between(builder, input, slow);
    let again = builder.load(types::I64, after, input, 8);
    builder.return_(&[again]);
}

/// Whether `reread_body` with these loads and `between` gets a copy.
fn reread_copied(
    before: MemFlags,
    after: MemFlags,
    between: impl Fn(&mut dyn FunctionBackend, Val, FuncHandle) + 'static,
) -> bool {
    let mut module = LlvmModule::for_host().expect("the host should have an LLVM target");
    let function = define(&mut module, "reread", true, move |builder, slow| {
        reread_body(builder, slow, before, after, &between);
    });
    let copied = body(&module, function).slow_copy_insts > 0;
    lowered(&module);
    copied
}

#[test]
fn only_a_slow_way_after_which_a_word_is_read_again_leaves_the_copy() {
    assert!(reread_copied(access(), access(), |_, _, _| {}));
    // A call, an indirect call, a write of any memory, or a fence between
    // the join and the read ends what the check found anyway.
    assert!(!reread_copied(access(), access(), |builder, input, slow| {
        let callee = builder.import_function(slow);
        builder.call(callee, &[input]);
    }));
    assert!(!reread_copied(access(), access(), |builder, input, _| {
        let signature = builder.import_signature(signature(&[types::I64], &[types::I64]));
        builder.call_indirect(signature, input, &[input]);
    }));
    assert!(!reread_copied(access(), access(), |builder, input, _| {
        builder.store(MemFlags::trusted(), input, input, 0);
    }));
    assert!(!reread_copied(access(), access(), |builder, _, _| {
        builder.memory_access(MemoryAccess::Fence, types::I64, &[]);
    }));
    // A write of another class of memory keeps it.
    assert!(reread_copied(access(), access(), |builder, input, _| {
        let flags = MemFlags::trusted_in(AliasClass::ListElements);
        builder.store(flags, input, input, 0);
    }));
    // The frame stack's words and a list's elements do not count.
    let frame = MemFlags::trusted_in(AliasClass::FrameHeader);
    assert!(!reread_copied(frame, frame, |_, _, _| {}));
    let elements = MemFlags::trusted_in(AliasClass::ListElements);
    assert!(!reread_copied(elements, elements, |_, _, _| {}));
}

#[test]
fn a_word_is_the_same_through_added_offsets_and_copies() {
    let mut module = LlvmModule::for_host().expect("the host should have an LLVM target");
    let function = define(&mut module, "offsets", false, |builder, slow| {
        reread_body(builder, slow, access(), access(), &|builder, input, _| {
            builder.iadd_imm(input, 0);
        });
    });
    let mut ir = body(&module, function).clone();
    // The join's read of `input + 8` reads `copied + 0` instead, where
    // `copied` copies `input + 8`.
    let join = 3;
    let added = ir.blocks[join].insts[0] as usize;
    let input = ir.blocks[ir.layout[0] as usize].params[0];
    ir.insts[added].op = Op::BinaryImm(super::BinaryOp::Iadd, input, 8);
    let copy = ir.new_value(types::I64);
    let copied = ir.insts.len() as u32;
    ir.value_defs[copy.0 as usize] = Some(copied);
    ir.insts.push(super::InstIr {
        op: Op::Copy(ir.insts[added].results[0]),
        results: vec![copy],
        block: join as u32,
        location: None,
    });
    ir.blocks[join].insts.insert(1, copied);
    let read = ir.blocks[join].insts[2] as usize;
    let Op::Load(kind, flags, _, _) = ir.insts[read].op.clone() else {
        panic!("the join reads the word: {}", ir.display());
    };
    ir.insts[read].op = Op::Load(kind, flags, copy, 0);
    assert!(ir.continue_slow_paths(), "{}", ir.display());
}

#[test]
fn a_word_read_through_a_variable_the_slow_way_sets_is_read_again() {
    let mut module = LlvmModule::for_host().expect("the host should have an LLVM target");
    let function = define(&mut module, "variable", true, |builder, slow| {
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        let fast = builder.create_block();
        let cold = builder.create_block();
        let call = builder.create_block();
        let join = builder.create_block();
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let input = builder.block_params(entry)[0];
        let held = Var(0);
        builder.declare_var(held, types::I64);
        builder.def_var(held, input);
        builder.set_cold_block(cold);
        builder.brif(input, fast, &[], cold, &[]);
        builder.switch_to_block(fast);
        builder.seal_block(fast);
        builder.jump(join, &[]);
        builder.switch_to_block(cold);
        builder.seal_block(cold);
        builder.jump(call, &[]);
        // A warm block only the cold way reaches makes the slow way's
        // call, and sets the variable the join reads through.
        builder.switch_to_block(call);
        builder.seal_block(call);
        let callee = builder.import_function(slow);
        let called = builder.call(callee, &[input]).results[0];
        builder.def_var(held, called);
        builder.jump(join, &[]);
        builder.switch_to_block(join);
        builder.seal_block(join);
        let address = builder.use_var(held);
        let word = builder.load(types::I64, access(), address, 0);
        builder.return_(&[word]);
    });
    assert!(body(&module, function).slow_copy_insts > 0);
    lowered(&module);
}

#[test]
fn a_body_that_calls_itself_keeps_one_copy() {
    let mut module = LlvmModule::for_host().expect("the host should have an LLVM target");
    let sig = signature(&[types::I64], &[types::I64]);
    let function = module
        .declare_function("recursive", Linkage::Export, &sig)
        .expect("the function should declare");
    module
        .define_function(
            function,
            &sig,
            Box::new(move |builder| {
                checked_body(builder, function);
                builder.verify()
            }),
        )
        .expect("the function should define");
    assert_eq!(body(&module, function).slow_copy_insts, 0);
}

#[test]
fn a_copy_that_would_add_too_much_code_is_not_made() {
    let mut module = LlvmModule::for_host().expect("the host should have an LLVM target");
    let function = define(&mut module, "long", true, |builder, slow| {
        reread_body(builder, slow, access(), access(), &|builder, input, _| {
            let mut value = input;
            for _ in 0..super::MOST_GROWTH {
                value = builder.iadd_imm(value, 1);
            }
        });
    });
    assert_eq!(body(&module, function).slow_copy_insts, 0);
}
