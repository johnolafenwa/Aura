//! Closures in the verifier.
//!
//! A lambda or a local `def` is lowered to a function of its own whose
//! leading parameters are what it captured. It has no contract, so a call
//! of it says nothing about its result. The executor instead follows the
//! closure value from where it is made to where it is called, and runs
//! the body there, on the captured values and the arguments of that call:
//!
//! - A capture by value is the value the local had where the closure was
//!   made.
//! - A capture by reference is read from its local at the call, and the
//!   value the body leaves is written back.
//! - State the closure owns and changes is kept in the closure value and
//!   updated by each call.
//!
//! Every check in the body is then made at the call, with what is known
//! there. A closure whose every use is such a call is not verified on its
//! own, where its captured values would be unknown.

use std::collections::{BTreeMap, BTreeSet};

use super::exec::{fit, root_of, Exec, Goal, IntTy, State, Val};
use super::{ObligationKind, VerifyContext};
use crate::mir::{
    CallTarget, Instruction, MirArg, MirClosureCapture, MirFunction, MirModule, MirReceiverKind,
    Operand, Rvalue,
};
use crate::sema::Type;
use crate::Program;

/// The name a closure body's result has in the states it returns in.
const RESULT: &str = "%closure_result";

/// What the executor of one function remembers about closures.
#[derive(Default)]
pub(super) struct Closures<'a> {
    /// What the function's own text says about the closures it makes.
    survey: Option<Survey<'a>>,
    /// The states a closure body returned in, while it runs for a call.
    returns: Option<Vec<State>>,
    /// Set when the call just made ran a closure's body, which wrote
    /// what it changed.
    ran: bool,
}

/// What the text of a function says about the closures it makes.
#[derive(Default)]
pub(super) struct Survey<'a> {
    /// The captures of each closure the function makes, by the closure's
    /// function.
    made: BTreeMap<&'a str, &'a [MirClosureCapture]>,
    /// The closure each local holds, for a closure whose every use is a
    /// call through a local.
    holders: BTreeMap<&'a str, &'a str>,
    /// Whether a closure with another use changes what it captured. Any
    /// call may run such a closure.
    loose: bool,
    /// Every closure function the function names.
    named: BTreeSet<&'a str>,
}

/// Whether `name` is a function the compiler wrote for a closure: the
/// body of a lambda or a local `def`, or the call a bound method makes.
/// Such a function has no contract.
pub(super) fn is_closure_function(name: &str) -> bool {
    name.contains("::__lambda_") || name.contains("::__bound_")
}

fn place_of(operand: &Operand) -> Option<&str> {
    match operand {
        Operand::Place(place) | Operand::MovePlace(place) => Some(place),
        _ => None,
    }
}

/// The value an instruction computes.
fn computed(instruction: &Instruction) -> Option<&Rvalue> {
    match instruction {
        Instruction::Assign { value, .. } | Instruction::WriteLoan { value, .. } => Some(value),
        _ => None,
    }
}

/// The operand `value` hands on as it is: a copy or a move, or the same
/// function value under the parameter names of another type.
fn handed_on(value: &Rvalue) -> Option<&Operand> {
    match value {
        Rvalue::Use(operand) | Rvalue::CallableAdapt { value: operand, .. } => Some(operand),
        _ => None,
    }
}

/// The closure function `value` makes a value of, with its captures.
fn made_by(value: &Rvalue) -> Option<(&str, &[MirClosureCapture])> {
    let (name, captures): (&str, &[MirClosureCapture]) = match value {
        Rvalue::Closure {
            function, captures, ..
        } => (function, captures),
        _ => match handed_on(value)? {
            Operand::Function { name, .. } => (name, &[]),
            _ => return None,
        },
    };
    is_closure_function(name).then_some((name, captures))
}

/// Whether a call of a closure may change what it captured as `capture`.
fn writes(capture: &MirClosureCapture) -> bool {
    capture.passing == MirReceiverKind::BorrowMut || capture.mutated
}

/// The local a capture reaches by reference, when it does.
fn reached(capture: &MirClosureCapture) -> Option<&str> {
    let borrowed = capture.passing == MirReceiverKind::BorrowMut;
    capture
        .source_place
        .as_deref()
        .or_else(|| place_of(&capture.value).filter(|_| borrowed))
}

/// Whether `place` is a whole local.
fn whole(place: &str) -> bool {
    root_of(place) == place
}

/// The target and the source of a copy or a move between whole locals.
fn copy(instruction: &Instruction) -> Option<(&str, &str)> {
    match instruction {
        Instruction::Assign { target, value } if whole(target) => {
            let source = handed_on(value).and_then(place_of)?;
            Some((target.as_str(), source))
        }
        _ => None,
    }
}

fn survey(function: &MirFunction) -> Survey<'_> {
    let mut survey = Survey::default();
    let instructions = || function.blocks.iter().flat_map(|block| &block.instructions);
    // Closures with a use the executor does not follow.
    let mut used: BTreeSet<&str> = BTreeSet::new();
    // The closures each local may hold: one where it is made, and the
    // same ones wherever the local is copied or moved.
    let mut held: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for instruction in instructions() {
        let Instruction::Assign { target, value } = instruction else {
            continue;
        };
        let Some((name, captures)) = made_by(value) else {
            continue;
        };
        survey.made.entry(name).or_insert(captures);
        held.entry(target).or_default().insert(name);
    }
    let mut grew = !held.is_empty();
    while grew {
        grew = false;
        for (target, source) in instructions().filter_map(copy) {
            let Some(from) = held.get(source).cloned() else {
                continue;
            };
            let to = held.entry(target).or_default();
            let before = to.len();
            to.extend(from);
            grew |= to.len() > before;
        }
    }
    // A closure has another use wherever a local that may hold it is
    // named by anything but a copy, a call of it, or its release.
    let mut named: Vec<&str> = Vec::new();
    for instruction in instructions() {
        let value = computed(instruction);
        let mut mentions = Vec::new();
        match instruction {
            Instruction::Retain { .. }
            | Instruction::Release { .. }
            | Instruction::PushCleanup { .. }
            | Instruction::PopCleanup { .. } => {}
            // A copy from a local that holds no closure of this function
            // leaves its target holding one of another origin.
            _ if copy(instruction).is_some_and(|(_, source)| held.contains_key(source)) => {}
            _ if copy(instruction).is_some() => {
                mentions.extend(copy(instruction).map(|(target, _)| target))
            }
            _ => crate::mir::instruction_place_mentions(instruction, &mut mentions),
        }
        match value {
            Some(Rvalue::Call {
                callee: CallTarget::Value(callee),
                ..
            }) => {
                let callee = place_of(callee).filter(|callee| whole(callee));
                let at = callee.and_then(|callee| mentions.iter().position(|m| *m == callee));
                if let Some(at) = at {
                    mentions.remove(at);
                }
            }
            Some(Rvalue::Call {
                callee: CallTarget::Name(name),
                ..
            }) => survey.named.extend([name.as_str()]),
            _ => {}
        }
        // Writing anything else to a local that holds a closure leaves
        // the local holding a closure of another origin.
        if let (Instruction::Assign { target, .. }, None) = (instruction, value.and_then(made_by)) {
            mentions.extend(copy(instruction).is_none().then_some(target.as_str()));
        }
        named.extend(mentions);
        let made = value.and_then(handed_on).is_some();
        for (operand, _) in crate::mir::ownership::operand_roles(instruction) {
            if let (Operand::Function { name, .. }, false) = (operand, made) {
                used.insert(name);
            }
        }
    }
    for block in &function.blocks {
        named.extend(crate::mir::terminator_place_mentions(&block.terminator));
        for (operand, _) in crate::mir::ownership::terminator_roles(&block.terminator) {
            if let Operand::Function { name, .. } = operand {
                used.insert(name);
            }
        }
    }
    for place in named {
        used.extend(held.get(root_of(place)).into_iter().flatten().copied());
    }
    // So does one put straight into a field, and each of two that one
    // local may hold.
    for (local, closures) in &held {
        if !whole(local) || closures.len() > 1 {
            used.extend(closures.iter().copied());
        }
    }
    for (local, closures) in held {
        let closure = closures.into_iter().next().expect("a held closure");
        if !used.contains(closure) {
            survey.holders.insert(local, closure);
        }
    }
    let followed: BTreeSet<&str> = survey.holders.values().copied().collect();
    survey.loose = instructions()
        .filter_map(computed)
        .any(|value| match value {
            Rvalue::Closure {
                function, captures, ..
            } => !followed.contains(function.as_str()) && captures.iter().any(writes),
            _ => false,
        });
    survey.named.extend(used);
    survey.named.extend(survey.made.keys().copied());
    survey.named.retain(|name| is_closure_function(name));
    survey
}

/// The locals of `function` that hold a closure with state of its own,
/// when every use of the closure is a call: each call writes the local.
pub(super) fn stateful_holders(function: &MirFunction) -> Vec<String> {
    let stateful = |value: &Rvalue| {
        made_by(value).is_some_and(|(_, captures)| captures.iter().any(|capture| capture.mutated))
    };
    let mut values = function.blocks.iter().flat_map(|block| &block.instructions);
    if !values.any(|instruction| computed(instruction).is_some_and(stateful)) {
        return Vec::new();
    }
    let survey = survey(function);
    survey
        .holders
        .iter()
        .filter(|(_, closure)| survey.made[*closure].iter().any(|capture| capture.mutated))
        .map(|(local, _)| local.to_string())
        .collect()
}

/// The closure functions of the program's module that are checked where
/// they are called: every use of each is a call through a local of the
/// one function that makes it, and that function is verified.
pub(super) fn checked_at_calls<'a>(
    program: &Program,
    cx: &VerifyContext<'_>,
    module: &'a MirModule,
) -> BTreeSet<&'a str> {
    let mut followed: BTreeSet<&str> = BTreeSet::new();
    let mut named: BTreeMap<&str, usize> = BTreeMap::new();
    for function in &module.functions {
        if function.module_name != program.module_name {
            continue;
        }
        let survey = survey(function);
        for name in &survey.named {
            *named.entry(name).or_default() += 1;
        }
        if super::verified_function(program, cx, function) {
            followed.extend(survey.holders.values().copied());
        }
    }
    followed.retain(|name| named[name] == 1);
    followed
}

impl<'a> Exec<'a> {
    fn survey(&mut self) -> &Survey<'a> {
        let function = self.function;
        self.closures.survey.get_or_insert_with(|| survey(function))
    }

    /// The locals a call the executor does not follow may change through
    /// a closure that captured them.
    pub(super) fn loose_captures(&mut self) -> BTreeSet<String> {
        let ran = std::mem::take(&mut self.closures.ran);
        if ran || self.captured.is_empty() || !self.survey().loose {
            return BTreeSet::new();
        }
        self.captured.clone()
    }

    /// The value of a closure made from `captures`: the function it
    /// runs, and the value of each capture that is copied or moved in.
    pub(super) fn closure_value(
        &mut self,
        state: &mut State,
        function: &str,
        captures: &[MirClosureCapture],
    ) -> Option<Val> {
        // A closure around a function with a contract of its own, and a
        // capture of whatever a view selected where the closure was made,
        // are not followed.
        let selected = |capture: &MirClosureCapture| capture.resolve_source_at_capture;
        if !is_closure_function(function) || captures.iter().any(selected) {
            return None;
        }
        let mut values = Vec::new();
        for capture in captures {
            values.push(match reached(capture) {
                Some(_) => Val::Unit,
                None => self.followed_operand(state, &capture.value, IntTy::of(&capture.ty)),
            });
        }
        Some(Val::Closure(function.to_string(), values))
    }

    /// Whether the call of `callee`, which holds `value`, is a call of a
    /// closure.
    pub(super) fn closure_callee(&mut self, callee: &Operand, value: &Val) -> bool {
        match value {
            Val::Closure(..) => true,
            Val::Func(name) => is_closure_function(name) && self.cx.function(name).is_some(),
            _ => place_of(callee).is_some_and(|place| self.survey().holders.contains_key(place)),
        }
    }

    /// A call of the closure `value` that `callee` holds: its body runs on
    /// the captured values and `args`.
    pub(super) fn closure_call(
        &mut self,
        state: &mut State,
        target: Option<&str>,
        callee: &Operand,
        value: Val,
        args: &[MirArg],
    ) -> Val {
        let place = place_of(callee);
        let holder = place.and_then(|place| self.survey().holders.get(place).copied());
        let followed = match &value {
            Val::Closure(name, values) => {
                let made = self.survey().made.get(name.as_str()).copied();
                made.filter(|captures| {
                    // State a closure keeps is known only while no other
                    // use can run it.
                    holder.is_some() || !captures.iter().any(|capture| capture.mutated)
                })
                .map(|captures| (name.clone(), captures, values.clone()))
            }
            Val::Func(name) => Some((name.clone(), &[][..], Vec::new())),
            _ => None,
        };
        let ran = followed.and_then(|(name, captures, values)| {
            self.run_closure(state, place, &name, captures, values, args)
        });
        if let Some(result) = ran {
            self.closures.ran = true;
            return result;
        }
        // The body's checks are made nowhere else when every use of the
        // closure is a call.
        if holder.is_some() {
            let (span, pc) = (self.span, state.pc.clone());
            self.oblige(
                ObligationKind::Assert,
                span,
                "the checks in the body of the closure called here",
                &pc,
                Goal::Unsupported("the verifier does not follow the closure to this call".into()),
            );
        }
        let target_type = target.and_then(|target| self.place_type(target));
        self.unmodeled("a call of a closure the verifier does not follow", |exec| {
            for root in exec.captured.clone() {
                exec.havoc_root(state, &root);
            }
            exec.havoc_writebacks(state, args);
            target_type.map_or(Val::Opaque, |ty| exec.fresh(&ty, target.unwrap_or("call")))
        })
    }

    /// Runs the body of the closure function `name` for one call, and
    /// gives its result. `None` when the executor cannot walk the body.
    fn run_closure(
        &mut self,
        state: &mut State,
        holder: Option<&str>,
        name: &str,
        captures: &[MirClosureCapture],
        mut values: Vec<Val>,
        args: &[MirArg],
    ) -> Option<Val> {
        let function = self.cx.function(name)?;
        let (hidden, declared) = function.params.split_at(captures.len());
        let mut env = BTreeMap::new();
        for ((capture, param), value) in captures.iter().zip(hidden).zip(&values) {
            let value = match reached(capture) {
                Some(place) => self.read_place(state, place),
                None => value.clone(),
            };
            let value = self.passed(&state.pc, value, &param.ty);
            env.insert(param.name.clone(), value);
        }
        let mut writebacks = Vec::new();
        let mut positional = declared.iter();
        for arg in args {
            let param = match &arg.name {
                Some(name) => declared.iter().find(|param| &param.name == name),
                None => positional.next(),
            }?;
            let value = self.followed_operand(state, &arg.value, IntTy::of(&param.ty));
            let value = self.passed(&state.pc, value, &param.ty);
            env.insert(param.name.clone(), value);
            if let (Some(place), MirReceiverKind::BorrowMut) = (&arg.writeback_place, param.passing)
            {
                writebacks.push((place.clone(), &param.name));
            }
        }
        let mut body = Exec::new(self.cx, function);
        self.share(&mut body);
        body.closures.returns = Some(Vec::new());
        body.run(State::at(state.pc.clone(), env));
        self.share(&mut body);
        // A body the executor cannot walk returned in no state it knows.
        let returned = body.closures.returns.take();
        let returned = returned.filter(|_| body.unsupported.is_none())?;
        let mut after = self.merge(returned);
        let mut left = |param: &str| after.env.remove(param).unwrap_or(Val::Opaque);
        let result = left(RESULT);
        let mut kept = false;
        for ((capture, param), value) in captures.iter().zip(hidden).zip(&mut values) {
            match reached(capture) {
                Some(place) if capture.passing == MirReceiverKind::BorrowMut => {
                    let value = left(&param.name);
                    self.write_place(state, place, value);
                }
                None if capture.mutated => {
                    *value = left(&param.name);
                    kept = true;
                }
                _ => {}
            }
        }
        for (place, param) in writebacks {
            let value = left(param);
            self.write_place(state, &place, value);
        }
        if let (true, Some(holder)) = (kept, holder) {
            self.write_place(state, holder, Val::Closure(name.to_string(), values));
        }
        state.pc = after.pc;
        Some(result)
    }

    /// `value` as the body of a closure receives it in a parameter of
    /// type `ty`. An integer is in the range of its type there: a
    /// conversion that did not fit trapped before the call.
    fn passed(&mut self, pc: &str, value: Val, ty: &Type) -> Val {
        let value = fit(value, ty);
        if let (Val::Int(term, _), Some(int)) = (&value, IntTy::of(ty)) {
            self.assume(pc, int.contains(term));
        }
        value
    }

    /// Hands what the solver is told so far to the executor of a closure
    /// body, or takes it back with what the body added.
    fn share(&mut self, body: &mut Exec<'a>) {
        use std::mem::swap;
        swap(&mut self.decls, &mut body.decls);
        swap(&mut self.facts, &mut body.facts);
        swap(&mut self.obligations, &mut body.obligations);
        swap(&mut self.counter, &mut body.counter);
        swap(&mut self.defined, &mut body.defined);
        swap(&mut self.definitions, &mut body.definitions);
        swap(&mut self.unmodeled_steps, &mut body.unmodeled_steps);
        swap(&mut self.unmodeled, &mut body.unmodeled);
        swap(&mut self.lazy, &mut body.lazy);
        swap(&mut self.guarded, &mut body.guarded);
        swap(&mut self.rounded, &mut body.rounded);
        swap(&mut self.span_positions, &mut body.span_positions);
        swap(&mut self.text, &mut body.text);
    }

    /// Notes a state the function returned in, with its result, when the
    /// function is a closure body running for a call.
    pub(super) fn closure_returned(&mut self, mut state: State, result: Val) {
        if let Some(returns) = &mut self.closures.returns {
            state.env.insert(RESULT.to_string(), result);
            returns.push(state);
        }
    }
}
