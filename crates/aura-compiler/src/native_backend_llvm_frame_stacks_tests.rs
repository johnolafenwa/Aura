use inkwell::context::Context;

use super::super::{host_target_machine, LlvmModule};
use super::{share_frame_stacks, switches_frame_stack, Dominators};
use crate::native_backend::{
    types, CallConv, FuncHandle, FunctionBackend, IntCC, Linkage, ModuleBackend, ScalarType,
    Signature,
};
use crate::runtime_abi::{RuntimeFn, RECOVER_SET_JUMP};

fn signature(params: &[ScalarType], returns: &[ScalarType]) -> Signature {
    Signature {
        call_conv: CallConv::C,
        params: params.to_vec(),
        returns: returns.to_vec(),
        extensions: Vec::new(),
        indirects: Vec::new(),
    }
}

/// A module that imports the frame stack accessor and the function
/// `other`, which takes and returns nothing, and defines `name` with
/// `body`.
fn module_with(
    other: &str,
    name: &str,
    params: &[ScalarType],
    body: impl FnOnce(&mut dyn FunctionBackend, FuncHandle, FuncHandle) + 'static,
) -> LlvmModule {
    let mut module = LlvmModule::for_host().expect("the host should have an LLVM target");
    let stack = module
        .declare_function(
            RuntimeFn::FrameStack.symbol(),
            Linkage::Import,
            &signature(&[], &[types::I64]),
        )
        .expect("the accessor should declare");
    module.mark_read_only(stack);
    let other = module
        .declare_function(other, Linkage::Import, &signature(&[], &[]))
        .expect("the import should declare");
    let sig = signature(params, &[types::I64]);
    let function = module
        .declare_function(name, Linkage::Export, &sig)
        .expect("the function should declare");
    module
        .define_function(
            function,
            &sig,
            Box::new(move |builder| {
                body(builder, stack, other);
                builder.verify()
            }),
        )
        .expect("the function should define");
    module
}

/// How many calls of the accessor `module` makes once the pass ran, and
/// how many the pass said it removed.
fn reads_after_sharing(module: &LlvmModule) -> (usize, usize) {
    let machine = host_target_machine().expect("the host target machine should exist");
    let context = Context::create();
    let lowered = module
        .lower(&context, &machine)
        .expect("the module should lower and verify");
    let removed = share_frame_stacks(&lowered);
    lowered.verify().expect("the module should still verify");
    let ir = lowered.print_to_string().to_string();
    (
        ir.matches("call i64 @aura_direct_frame_stack()").count(),
        removed,
    )
}

/// Reads the stack, calls `other`, and reads the stack again.
fn around_a_call(other: &str) -> LlvmModule {
    module_with(other, "around", &[], |builder, stack, other| {
        let entry = builder.create_block();
        builder.switch_to_block(entry);
        let stack = builder.import_function(stack);
        let other = builder.import_function(other);
        let before = builder.call(stack, &[]).results[0];
        builder.call(other, &[]);
        let after = builder.call(stack, &[]).results[0];
        let sum = builder.iadd(before, after);
        builder.return_(&[sum]);
    })
}

#[test]
fn a_read_stands_in_for_a_later_one_across_a_call_that_keeps_the_stack() {
    let module = around_a_call(RuntimeFn::YieldNow.symbol());
    assert_eq!(reads_after_sharing(&module), (1, 1));
}

#[test]
fn a_function_that_switches_stacks_keeps_every_read() {
    for switching in [
        RuntimeFn::RecoverBegin.symbol(),
        RuntimeFn::RecoverAdopt.symbol(),
        RuntimeFn::RecoverEnd.symbol(),
        RuntimeFn::RecoverTrapped.symbol(),
        RECOVER_SET_JUMP,
        "aura_host_iter_generator_yield",
        "aura_host_iter_generator_resume",
    ] {
        assert!(switches_frame_stack(switching), "{switching}");
        let module = around_a_call(switching);
        assert_eq!(reads_after_sharing(&module), (2, 0), "{switching}");
    }
    assert!(!switches_frame_stack(RuntimeFn::FrameStack.symbol()));
    assert!(!switches_frame_stack(RuntimeFn::ReleaseValue.symbol()));
}

#[test]
fn a_read_stands_in_only_where_every_way_passes_it() {
    // Two branches read the stack, and so does the block they join in:
    // neither branch's read reaches the join on every way, so all three
    // stay. A read in a block nothing reaches stays too.
    let siblings = module_with(
        RuntimeFn::YieldNow.symbol(),
        "siblings",
        &[types::I64],
        |builder, stack, other| {
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            let left = builder.create_block();
            let right = builder.create_block();
            let join = builder.create_block();
            let unreached = builder.create_block();
            let joined = builder.append_block_param(join, types::I64);
            builder.switch_to_block(entry);
            builder.seal_block(entry);
            let stack = builder.import_function(stack);
            let other = builder.import_function(other);
            let flag = builder.block_params(entry)[0];
            let chosen = builder.icmp_imm(IntCC::NotEqual, flag, 0);
            builder.brif(chosen, left, &[], right, &[]);
            for side in [left, right] {
                builder.switch_to_block(side);
                builder.seal_block(side);
                let read = builder.call(stack, &[]).results[0];
                builder.call(other, &[]);
                builder.jump(join, &[read]);
            }
            builder.switch_to_block(unreached);
            builder.seal_block(unreached);
            let read = builder.call(stack, &[]).results[0];
            builder.jump(join, &[read]);
            builder.switch_to_block(join);
            builder.seal_block(join);
            let read = builder.call(stack, &[]).results[0];
            let sum = builder.iadd(joined, read);
            builder.return_(&[sum]);
        },
    );
    assert_eq!(reads_after_sharing(&siblings), (4, 0));

    // With a read before the branch, that one stands in for the rest that
    // can be reached, in a loop's body as well.
    let dominated = module_with(
        RuntimeFn::YieldNow.symbol(),
        "dominated",
        &[types::I64],
        |builder, stack, other| {
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            let head = builder.create_block();
            let body = builder.create_block();
            let done = builder.create_block();
            let count = builder.append_block_param(head, types::I64);
            builder.switch_to_block(entry);
            builder.seal_block(entry);
            let stack = builder.import_function(stack);
            let other = builder.import_function(other);
            let limit = builder.block_params(entry)[0];
            let first = builder.call(stack, &[]).results[0];
            builder.jump(head, &[limit]);
            builder.switch_to_block(head);
            let more = builder.icmp_imm(IntCC::NotEqual, count, 0);
            builder.brif(more, body, &[], done, &[]);
            builder.switch_to_block(body);
            builder.seal_block(body);
            builder.call(other, &[]);
            let read = builder.call(stack, &[]).results[0];
            let same = builder.icmp(IntCC::Equal, read, first);
            let same = builder.uextend(types::I64, same);
            let next = builder.isub(count, same);
            builder.jump(head, &[next]);
            builder.seal_block(head);
            builder.switch_to_block(done);
            builder.seal_block(done);
            let last = builder.call(stack, &[]).results[0];
            builder.return_(&[last]);
        },
    );
    assert_eq!(reads_after_sharing(&dominated), (1, 2));
}

#[test]
fn a_module_without_the_accessor_or_with_one_read_is_left_alone() {
    let one = module_with(
        RuntimeFn::YieldNow.symbol(),
        "one",
        &[],
        |builder, stack, _| {
            let entry = builder.create_block();
            builder.switch_to_block(entry);
            let stack = builder.import_function(stack);
            let read = builder.call(stack, &[]).results[0];
            builder.return_(&[read]);
        },
    );
    assert_eq!(reads_after_sharing(&one), (1, 0));

    let machine = host_target_machine().expect("the host target machine should exist");
    let context = Context::create();
    let empty = LlvmModule::for_host()
        .expect("the host should have an LLVM target")
        .lower(&context, &machine)
        .expect("an empty module should lower");
    assert_eq!(share_frame_stacks(&empty), 0);
}

#[test]
fn dominators_follow_every_way_from_the_entry() {
    // 0 -> 1 -> {2, 3} -> 4 -> 1 (a loop), 4 -> 5, and 6 is not reached.
    let tree = Dominators::of_successors(&[
        vec![1],
        vec![2, 3],
        vec![4],
        vec![4],
        vec![1, 5],
        vec![],
        vec![5],
    ]);
    assert_eq!(tree.order[6], None);
    assert!(tree.order[..6].iter().all(Option::is_some));
    for block in 0..6 {
        assert!(tree.dominates(0, block), "{block}");
        assert!(tree.dominates(block, block), "{block}");
    }
    assert!(tree.dominates(1, 4));
    assert!(tree.dominates(1, 5));
    assert!(tree.dominates(4, 5));
    assert!(!tree.dominates(2, 4));
    assert!(!tree.dominates(3, 4));
    assert!(!tree.dominates(2, 3));
    assert!(!tree.dominates(5, 4));
    assert!(!tree.dominates(4, 1));

    // A graph with no block has no tree to walk.
    let empty = Dominators::of_successors(&[]);
    assert!(empty.order.is_empty() && empty.immediate.is_empty());
}
