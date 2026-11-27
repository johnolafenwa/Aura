//! Calls of functions that can only trap, made without the caller's frame
//! record.
//!
//! A function that defers its frame record (see `defers_frame`) pushes it
//! before each call that may need it, and a call of a frameless leaf that
//! may trap, such as `local_seconds` in `time`, which adds with an
//! overflow check, is one: the leaf pushes its own record on its trap path,
//! and the trap's call chain must name the caller below it. Such a call
//! lends the callee a link instead: two words on the caller's stack, the
//! caller's descriptor (or 0 once the caller pushed its record) and the
//! link the caller was itself lent, or 0. The callee's second body (its
//! "chain body") takes the link as one more parameter, and each of its
//! trap paths enters the links' records, outermost first, before its own
//! (see `aura_direct_enter_caller_chain`). The callee holds only words and
//! is on no call cycle, so the trap path is the only one that reads the
//! frame stack, and the call costs two stores.
//!
//! A function whose body is a frameless leaf's in all but direct calls of
//! such functions, or of total leaves, is one too: it runs without its own
//! record and lends its callees a link naming itself and the link it was
//! lent.

use std::collections::{HashMap, HashSet};

use super::*;

/// The functions of `module` whose calls lend a caller chain link (see the
/// module documentation): frameless in body, on no call cycle, with no
/// `mut` parameter and no safepoint, whose direct calls name only `total`
/// leaves or other such functions, and that may trap, which a total leaf
/// cannot.
pub(super) fn chain_callees(
    module: &MirModule,
    classes: &HashMap<String, MirClass>,
    total: &HashSet<String>,
    recursive: &HashSet<String>,
) -> HashSet<String> {
    let candidates = module
        .functions
        .iter()
        .filter(|function| {
            !recursive.contains(&function.name)
                && !total.contains(&function.name)
                && !takes_mutable(function)
                && function.module_name != SIMD_MODULE
                && !function.blocks.iter().any(|block| {
                    block
                        .instructions
                        .iter()
                        .any(|instruction| matches!(instruction, Instruction::Safepoint))
                })
        })
        .collect::<Vec<_>>();
    let mut allowed = total.clone();
    let mut chained = HashSet::new();
    loop {
        let found = candidates
            .iter()
            .filter(|function| !chained.contains(&function.name))
            .filter(|function| leaf_body_calling(function, classes, Some(&allowed)))
            .map(|function| function.name.clone())
            .collect::<Vec<_>>();
        if found.is_empty() {
            return chained;
        }
        allowed.extend(found.iter().cloned());
        chained.extend(found);
    }
}

impl FunctionCompiler<'_> {
    /// The body a direct call of `name` enters, `framed` or its chain body,
    /// with the link the chain body takes appended to `args`. A function
    /// that pushed its record on entry, or that may push it twice, lends
    /// none.
    pub(super) fn chain_callee(
        &mut self,
        name: &str,
        framed: FuncRef,
        args: &mut Vec<Val>,
    ) -> FuncRef {
        let Some(chained) = self.chain_refs.get(name).copied() else {
            return framed;
        };
        if self.function_refs.get(name) != Some(&framed) {
            return framed;
        }
        let deferred = self.deferred_frame.zip(self.deferred_descriptor);
        let (descriptor, next) = match (self.frameless_descriptor, deferred) {
            (Some(descriptor), _) => {
                let next = match self.chain_link {
                    Some(link) => link,
                    None => self.builder.iconst(types::I64, 0),
                };
                (descriptor, next)
            }
            (None, Some((framed_var, descriptor))) => {
                let pushed = self.builder.use_var(framed_var);
                let pushed = self.builder.icmp_imm(IntCC::NotEqual, pushed, 0);
                let none = self.builder.iconst(types::I64, 0);
                let descriptor = self.builder.select(pushed, none, descriptor);
                (descriptor, none)
            }
            (None, None) => return framed,
        };
        let slot = self.builder.create_sized_stack_slot(16, 3);
        let link = self.builder.stack_addr(types::I64, slot, 0);
        let flags = MemFlags::trusted();
        self.builder.store(flags, descriptor, link, 0);
        self.builder.store(flags, next, link, 8);
        args.push(link);
        chained
    }

    /// On a trap path of a chain body, enters the records of the callers
    /// its link names, below its own.
    pub(super) fn push_caller_chain(&mut self) {
        if let Some(link) = self.chain_link {
            self.builder
                .call(self.runtime[RuntimeFn::EnterCallerChain], &[link]);
        }
    }
}
