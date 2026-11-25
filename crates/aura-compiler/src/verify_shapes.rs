//! Values with a shape: objects inside other values, unions, and enums.
//!
//! An object of a class with invariants holds them wherever it crosses a
//! function boundary, and so does every such object inside a value that
//! crosses one: a field of another object, a tuple item, a member of a
//! union such as `Account | None`, an enum payload such as the `Ok` of a
//! `Result`, and an element of a list. A call proves it of each argument
//! and a return of the result and the `mut` parameters, and each side
//! assumes what the other proves. Inside a function nothing is assumed of
//! an object the function builds or changes, so a field write that breaks
//! an invariant is found where the object leaves.
//!
//! This module also reads a union where a contract or a comparison names
//! one of its members, runs a method call on a union as one call for each
//! member, and names the variants of an enum in a counterexample.

use std::collections::BTreeMap;

use super::exec::{
    and, conjoin, element_sort, project, scalar_term, set_in, structural_eq, symbols, Exec, Goal,
    State, Val, MAX_DEPTH,
};
use super::ObligationKind;
use crate::ast::{ContractClause, Expr, ExprKind, ReceiverKind};
use crate::diag::Span;
use crate::mir::{CallTarget, MethodDispatch, MirArg, Operand};
use crate::sema::Type;

/// How far a search through a type for a class with invariants goes. A
/// type can hold itself, as a tree node holds its children.
const TYPE_SEARCH_DEPTH: usize = 8;

/// What this module remembers for one function.
#[derive(Default)]
pub(super) struct ShapeState {
    /// The variant names of each enum input, by the label of its tag.
    variants: BTreeMap<String, Vec<String>>,
    /// The conjuncts of each path that left an exhaustive `match` without
    /// taking an arm.
    dead: Vec<Vec<String>>,
}

/// An object inside a value, or a part of the value that should hold
/// objects and that the encoding does not model.
struct Inner {
    /// How a message names it, such as `xs[..]` or `bank.vault`.
    label: String,
    /// The variables that stand for list positions on the way to it.
    binders: Vec<String>,
    /// What holds where it exists: the positions are in range, and each
    /// union or enum on the way holds the member it is in.
    guards: Vec<String>,
    /// The object and its class, or `None` for a part that is not modeled.
    object: Option<(String, Val)>,
}

/// Where the objects inside a value are being looked for.
struct Walk {
    binders: Vec<String>,
    guards: Vec<String>,
    found: Vec<Inner>,
}

impl<'a> Exec<'a> {
    /// Whether a value of type `ty` can hold an object of a class with
    /// invariants where the encoding models one.
    fn holds_objects(&self, ty: &Type, fuel: usize) -> bool {
        let Some(fuel) = fuel.checked_sub(1) else {
            return false;
        };
        match ty {
            Type::Tuple(items) => items.iter().any(|item| self.holds_objects(item, fuel)),
            Type::Union(union) => union
                .members
                .iter()
                .any(|member| self.holds_objects(member, fuel)),
            Type::Named(name, args) if name == "list" && args.len() == 1 => {
                self.holds_objects(&args[0], fuel)
            }
            Type::Named(name, args) => {
                if let Some(fields) = self.cx.class_fields(name, args) {
                    return self.has_invariant(name)
                        || fields
                            .iter()
                            .any(|(_, field)| self.holds_objects(field, fuel));
                }
                self.cx.enum_variants(name, args).is_some_and(|variants| {
                    variants
                        .iter()
                        .flat_map(|(_, payloads)| payloads)
                        .any(|payload| self.holds_objects(payload, fuel))
                })
            }
            _ => false,
        }
    }

    fn has_invariant(&self, class: &str) -> bool {
        self.cx
            .class_invariant(class)
            .is_some_and(|(_, clauses)| !clauses.is_empty())
    }

    /// Whether a fresh list of `elem` values has its elements modeled, as
    /// `fresh_store` decides.
    fn store_models(&self, elem: &Type, depth: usize) -> bool {
        if element_sort(elem).is_some()
            || matches!(elem, Type::Named(name, args) if name == "list" && args.len() == 1)
        {
            return true;
        }
        depth <= MAX_DEPTH
            && self.element_items(elem).is_some_and(|items| {
                items
                    .iter()
                    .all(|(_, item)| self.store_models(item, depth + 1))
            })
    }

    /// Finds each object of a class with invariants inside `value`, of
    /// type `ty`. The value itself is left to its own check when it is an
    /// object.
    fn inner_objects(&mut self, ty: &Type, value: &Val, label: &str) -> Vec<Inner> {
        let mut walk = Walk {
            binders: Vec::new(),
            guards: Vec::new(),
            found: Vec::new(),
        };
        if self.holds_objects(ty, TYPE_SEARCH_DEPTH) {
            self.walk_objects(ty, value, label, None, &mut walk);
        }
        walk.found
    }

    /// `depth` is how deep the value is inside the one that crosses the
    /// boundary, counted as a fresh value of a type counts it. It is
    /// `None` at that value itself, and inside a list element, where the
    /// element store models every level it has.
    fn walk_objects(
        &mut self,
        ty: &Type,
        value: &Val,
        label: &str,
        depth: Option<usize>,
        walk: &mut Walk,
    ) {
        let root = depth.is_none() && walk.binders.is_empty();
        let inside = depth.map(|depth| depth + 1).or(Some(1));
        let in_store = !walk.binders.is_empty();
        let below = if in_store { None } else { inside };
        let modeled = match (ty, value) {
            (Type::Tuple(types), Val::Tuple(items)) if types.len() == items.len() => {
                for (index, (ty, item)) in types.iter().zip(items).enumerate() {
                    let label = format!("{label}[{index}]");
                    self.walk_objects(ty, item, &label, below, walk);
                }
                true
            }
            (Type::Union(union), Val::Union(tag, members))
                if union.members.len() == members.len() =>
            {
                for (index, (ty, member)) in union.members.iter().zip(members).enumerate() {
                    walk.guards.push(format!("(= {tag} {index})"));
                    self.walk_objects(ty, member, label, below, walk);
                    walk.guards.pop();
                }
                true
            }
            (
                Type::Named(name, args),
                Val::List {
                    len,
                    data: Some(data),
                    elem,
                },
            ) if name == "list" && args.len() == 1 => {
                let k = self.bound_name();
                let element = self.store_select(data, elem, &k);
                walk.binders.push(format!("({k} Int)"));
                walk.guards.push(format!("(and (<= 0 {k}) (< {k} {len}))"));
                let label = format!("{label}[..]");
                self.walk_objects(&args[0], &element, &label, None, walk);
                walk.guards.pop();
                walk.binders.pop();
                true
            }
            // A list inside a list element has no objects modeled, for
            // the caller or for the callee.
            (Type::Named(name, args), Val::List { .. }) if name == "list" && args.len() == 1 => {
                in_store || !self.store_models(&args[0], 0)
            }
            (Type::Named(name, args), Val::Record(class, values)) => {
                if !root && self.has_invariant(class) {
                    walk.found.push(Inner {
                        label: label.to_string(),
                        binders: walk.binders.clone(),
                        guards: walk.guards.clone(),
                        object: Some((class.clone(), value.clone())),
                    });
                }
                let fields = self.cx.class_fields(name, args).unwrap_or_default();
                for (field, ty) in &fields {
                    if let Some((_, value)) = values.iter().find(|(name, _)| name == field) {
                        let label = format!("{label}.{field}");
                        self.walk_objects(ty, value, &label, below, walk);
                    }
                }
                true
            }
            (Type::Named(name, args), Val::Enum(_, tag, payloads)) => {
                let variants = self.cx.enum_variants(name, args).unwrap_or_default();
                for (index, ((variant, types), items)) in variants.iter().zip(payloads).enumerate()
                {
                    walk.guards.push(format!("(= {tag} {index})"));
                    for (ty, item) in types.iter().zip(items) {
                        let label = format!("{label}.{variant}");
                        self.walk_objects(ty, item, &label, below, walk);
                    }
                    walk.guards.pop();
                }
                true
            }
            _ => false,
        };
        // A part that holds objects for the function on the other side of
        // the boundary and is not modeled here: nothing is known of the
        // objects, so nothing can be proved of them. A fresh value is
        // modeled down to a fixed depth, and not below it.
        let other_side_models = depth.is_none_or(|depth| depth <= MAX_DEPTH);
        if !modeled && other_side_models && self.holds_objects(ty, TYPE_SEARCH_DEPTH) {
            walk.found.push(Inner {
                label: label.to_string(),
                binders: walk.binders.clone(),
                guards: walk.guards.clone(),
                object: None,
            });
        }
    }

    /// The term that says the object `inner` holds `clause`, wherever the
    /// object exists.
    fn inner_term(
        &mut self,
        inner: &Inner,
        object: &Val,
        clause: &ContractClause,
        module_name: &str,
        proving: bool,
    ) -> Result<String, String> {
        let env: BTreeMap<String, Val> = [("self".to_string(), object.clone())].into();
        let prev_proving = self.proving;
        let prev_module = std::mem::replace(&mut self.spec_module, module_name.to_string());
        self.proving = proving;
        let mut wd = Vec::new();
        let evaluated = self.spec(&clause.condition, &env, &mut wd, 0);
        self.spec_module = prev_module;
        self.proving = prev_proving;
        let Val::Bool(term) = evaluated? else {
            return Err("the clause is not a modeled `bool`".into());
        };
        wd.push(term);
        let mut body = conjoin(&wd);
        let mut hypotheses = inner.guards.clone();
        if !inner.binders.is_empty() {
            // An integer a list element holds lies in its type's range.
            super::spec::int_ranges(object, &mut hypotheses);
        }
        if !hypotheses.is_empty() {
            body = format!("(=> {} {body})", conjoin(&hypotheses));
        }
        if !inner.binders.is_empty() {
            body = format!("(forall ({}) {body})", inner.binders.join(" "));
        }
        Ok(body)
    }

    /// Proves the invariants of each object inside `value`. `place` says
    /// where the value is, as in `passed to `f``.
    fn prove_inner_invariants(
        &mut self,
        state: &State,
        ty: &Type,
        value: &Val,
        label: &str,
        place: &str,
        span: Span,
    ) {
        let erasing = std::mem::replace(&mut self.erasing, true);
        for inner in self.inner_objects(ty, value, label) {
            let Some((class, object)) = &inner.object else {
                self.oblige(
                    ObligationKind::ClassInvariant,
                    span,
                    format!(
                        "class invariant holds for each object in `{}` {place}",
                        inner.label
                    ),
                    &state.pc,
                    Goal::Unsupported(format!(
                        "the verifier does not model what `{}` holds",
                        inner.label
                    )),
                );
                continue;
            };
            for (module_name, clause) in self.object_invariant(class) {
                let goal = match self.inner_term(&inner, object, &clause, &module_name, true) {
                    Ok(term) => Goal::Holds(term),
                    Err(reason) => Goal::Unsupported(reason),
                };
                let message = format!(
                    "class invariant holds for `{}` {place}: `{}`",
                    inner.label, clause.text
                );
                self.oblige(
                    ObligationKind::ClassInvariant,
                    span,
                    message,
                    &state.pc,
                    goal,
                );
            }
        }
        self.erasing = erasing;
    }

    /// Assumes the invariants of each object inside `value`.
    fn assume_inner_invariants(&mut self, state: &State, ty: &Type, value: &Val) {
        let found = self.inner_objects(ty, value, "");
        let objects = found
            .iter()
            .filter_map(|inner| Some((inner, inner.object.as_ref()?)));
        for (inner, (class, object)) in objects {
            for (module_name, clause) in self.object_invariant(class) {
                let Ok(term) = self.inner_term(inner, object, &clause, &module_name, false) else {
                    continue;
                };
                if inner.binders.is_empty() {
                    self.assume(&state.pc, term);
                    continue;
                }
                // A fact about every element is asked only where a check
                // can reach the list's elements.
                let mut made = Vec::new();
                element_arrays(object, &mut made);
                self.lazy.insert(self.facts.len(), made);
                self.assume(&state.pc, term);
            }
        }
    }

    /// The invariant clauses of `class`, each with the module it was
    /// written in.
    fn object_invariant(&self, class: &str) -> Vec<(String, ContractClause)> {
        self.cx
            .class_invariant(class)
            .map(|(module_name, clauses)| {
                clauses
                    .iter()
                    .map(|clause| (module_name.to_string(), clause.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether the method `callee` starts from a receiver whose inner
    /// objects hold their invariants: a public method that borrows `self`,
    /// or a trait implementation method every call of which is proved.
    fn assumes_receiver(&self, callee: &str) -> bool {
        if self.dynamic_call {
            return false;
        }
        self.cx.impl_guard(callee).is_some()
            || self.cx.decl(callee).is_some_and(|decl| {
                let borrows = matches!(
                    decl.receiver,
                    Some(ReceiverKind::Borrow | ReceiverKind::BorrowMut)
                );
                decl.public && borrows
            })
    }

    /// The type of the parameter `param` of the function `callee`.
    fn param_type(&self, callee: &str, param: &str) -> Option<Type> {
        let function = self.cx.function(callee)?;
        if param == "self" {
            if !self.assumes_receiver(callee) {
                return None;
            }
            return function
                .local_types
                .iter()
                .find(|local| local.name == "self")
                .map(|local| local.ty.clone());
        }
        function
            .params
            .iter()
            .find(|candidate| candidate.name == param)
            .map(|candidate| candidate.ty.clone())
    }

    /// A call proves the invariants of each object inside an argument.
    pub(super) fn prove_inner_arguments(
        &mut self,
        state: &State,
        callee: &str,
        pre: &[(String, Val)],
        span: Span,
    ) {
        let display = crate::mir::public_function_name(callee);
        for (param, value) in pre {
            if let Some(ty) = self.param_type(callee, param) {
                let place = format!("passed to `{display}`");
                self.prove_inner_invariants(state, &ty, value, param, &place, span);
            }
        }
    }

    /// After a call, each object inside its result and inside what it may
    /// have changed holds its invariants.
    pub(super) fn assume_inner_results(
        &mut self,
        state: &State,
        callee: &str,
        pre: &[(String, Val)],
        post: &[(String, Val)],
        result: &Val,
    ) {
        // What the call cannot change holds what was proved before it.
        for ((param, value), (_, before)) in post.iter().zip(pre) {
            if value == before {
                continue;
            }
            if let Some(ty) = self.param_type(callee, param) {
                self.assume_inner_invariants(state, &ty, value);
            }
        }
        if let Some(function) = self.cx.function(callee) {
            self.assume_inner_invariants(state, &function.return_type, result);
        }
    }

    /// On entry, each object inside a parameter holds its invariants.
    pub(super) fn assume_inner_parameters(&mut self, state: &State) {
        let name = self.function.name.clone();
        let params: Vec<String> = std::iter::once("self".to_string())
            .chain(self.function.params.iter().map(|param| param.name.clone()))
            .collect();
        for param in params {
            if let (Some(ty), Some(value)) = (self.param_type(&name, &param), state.env.get(&param))
            {
                let value = value.clone();
                self.assume_inner_invariants(state, &ty, &value);
            }
        }
    }

    /// At a return, each object inside the result and inside a `mut`
    /// parameter holds its invariants.
    pub(super) fn prove_inner_returns(&mut self, state: &State, result: &Val) {
        let name = self.function.name.clone();
        let span = self.span;
        let return_type = self.function.return_type.clone();
        self.prove_inner_invariants(state, &return_type, result, "result", "on return", span);
        let mut changed: Vec<String> = self
            .function
            .params
            .iter()
            .filter(|param| param.passing == crate::mir::MirReceiverKind::BorrowMut)
            .map(|param| param.name.clone())
            .collect();
        if self.function.receiver == Some(crate::mir::MirReceiverKind::BorrowMut) {
            changed.push("self".to_string());
        }
        for param in changed {
            if let (Some(ty), Some(value)) = (self.param_type(&name, &param), state.env.get(&param))
            {
                let value = value.clone();
                self.prove_inner_invariants(state, &ty, &value, &param, "on return", span);
            }
        }
    }

    /// The value of the compiler temporary `key` where paths join and only
    /// some of them assigned it, as the arms of a `match` expression do
    /// when its last test falls through to the end. A temporary is read
    /// only after it is assigned, so the paths that left it unassigned do
    /// not read it, and it has the value the others gave it.
    pub(super) fn merge_assigned(
        &mut self,
        pcs: &[String],
        values: &[Option<&Val>],
        key: &str,
    ) -> Option<Val> {
        if !key.starts_with('%') {
            return None;
        }
        let (pcs, values): (Vec<String>, Vec<&Val>) = pcs
            .iter()
            .zip(values)
            .filter_map(|(pc, value)| Some((pc.clone(), (*value)?)))
            .unzip();
        Some(self.merge_vals(&pcs, &values, key))
    }
}

impl<'a> Exec<'a> {
    // ----- Unions and enums in contracts and comparisons ---------------------

    /// The value a contract reads where it uses a union such as
    /// `int64 | None` as its one member that is not `None`, as in
    /// `result is None or result >= 0`. The use is well-defined where the
    /// union holds that member.
    pub(super) fn spec_member(&self, value: Val, wd: &mut Vec<String>) -> Val {
        let held = |member: &&Val| **member != Val::Unit;
        match &value {
            Val::Union(tag, members) if members.iter().filter(held).count() == 1 => {
                let index = members.iter().position(|member| *member != Val::Unit);
                let index = index.expect("one member is not `None`");
                wd.push(format!("(= {tag} {index})"));
                members[index].clone()
            }
            _ => value,
        }
    }

    /// A variant without a payload that a contract names, as in
    /// `result == Light.Green`.
    pub(super) fn spec_variant(&self, name: &str, variant: &str) -> Option<Val> {
        let index = self.cx.variant_index(name, variant)?;
        let variants = self.cx.variant_names(name);
        // A variant with a payload is built by a call, which no contract
        // may make.
        Some(Val::Enum(
            name.to_string(),
            index.to_string(),
            vec![Vec::new(); variants.len()],
        ))
    }

    /// The value `object.field` reads from in a contract: the variant
    /// `field` of the enum `object` names, or else the object, read as
    /// the member it holds when it is a union.
    pub(super) fn spec_object(
        &mut self,
        object: &Expr,
        field: &str,
        env: &BTreeMap<String, Val>,
        wd: &mut Vec<String>,
        depth: usize,
    ) -> Result<Val, String> {
        let variant = match &object.kind {
            ExprKind::Name(name) if !env.contains_key(name) => self.spec_variant(name, field),
            _ => None,
        };
        if let Some(variant) = variant {
            return Ok(variant);
        }
        let value = self.spec(object, env, wd, depth)?;
        Ok(self.spec_member(value, wd))
    }

    // ----- Calls on a union -------------------------------------------------

    /// The function each member of a union receiver runs, when every one
    /// is a function of the program.
    fn union_arms(&self, arms: &[MethodDispatch]) -> Option<Vec<&'a str>> {
        arms.iter()
            .map(|arm| match arm {
                MethodDispatch::Static { function } => self
                    .cx
                    .function(function)
                    .map(|callee| callee.name.as_str()),
                _ => None,
            })
            .collect()
    }

    /// A method call on a union runs the method of the member the value
    /// holds. Each member's call is made where the union holds it: its
    /// `requires` is proved there and its `ensures` assumed there.
    pub(super) fn union_call(
        &mut self,
        state: &mut State,
        callee: &CallTarget,
        args: &[MirArg],
        target: Option<&str>,
    ) -> Option<Val> {
        // Only a trait method can be called on a union.
        let CallTarget::TraitMember {
            object,
            receiver_place,
            dispatch: Some(MethodDispatch::Union(arms)),
            ..
        } = callee
        else {
            return None;
        };
        let receiver = self.operand(state, object, None);
        let (Some(functions), Val::Union(tag, members)) = (self.union_arms(arms), receiver) else {
            return None;
        };
        let target_type = target.and_then(|target| self.place_type(target));
        let mut actuals = Vec::new();
        for arg in args {
            actuals.push((
                arg.name.clone(),
                self.followed_operand(state, &arg.value, None),
                arg.writeback_place.clone(),
            ));
        }
        let mut states = Vec::new();
        let mut results = Vec::new();
        for (index, (function, member)) in functions.iter().zip(members).enumerate() {
            let mut arm = state.clone();
            arm.pc = and(&state.pc, &format!("(= {tag} {index})"));
            let place = receiver_place.as_ref().map(|place| {
                format!(
                    "{place}.{}{index}",
                    crate::mir::UNION_PAYLOAD_PROJECTION_PREFIX
                )
            });
            results.push(self.user_call(
                &mut arm,
                function,
                Some((member, place)),
                actuals.clone(),
                target_type.as_ref(),
                target,
            ));
            states.push(arm);
        }
        let pcs: Vec<String> = states.iter().map(|arm| arm.pc.clone()).collect();
        let results: Vec<&Val> = results.iter().collect();
        let result = self.merge_vals(&pcs, &results, target.unwrap_or("call"));
        *state = self.merge(states);
        Some(result)
    }

    // ----- Borrowed list elements ------------------------------------------

    /// The value a loan of the element at `index` of the list at `list`
    /// reads, `projection` deep inside the element, when the list's
    /// elements are modeled. The list cannot change while the loan lasts,
    /// except through the loan.
    pub(super) fn lent_element(
        &mut self,
        state: &mut State,
        list: &str,
        index: &str,
        projection: &str,
    ) -> Option<Val> {
        let list = self.read_place(state, list);
        let Val::List { data: Some(_), .. } = &list else {
            return None;
        };
        let element = self.element(&list, index, "element");
        projection
            .split('.')
            .filter(|segment| !segment.is_empty())
            .try_fold(element, |value, segment| project(&value, segment))
    }

    /// Writes `value` to `path`, a place under a loan of the element at
    /// `index` of the list at `list`, into the list itself: the element
    /// with the write applied takes its position. `false` leaves the write
    /// to the caller, when the list's elements or the value are not
    /// modeled.
    pub(super) fn write_element(
        &mut self,
        state: &mut State,
        path: &str,
        (list, index, projection): (&str, &str, &str),
        value: &Val,
    ) -> bool {
        let before = self.read_place(state, list);
        // The loan's own copy starts from the element as it was.
        let root = path.split('.').next().unwrap_or(path);
        self.read_place(state, root);
        let segments: Vec<&str> = projection
            .split('.')
            .chain(path.split('.').skip(1))
            .filter(|segment| !segment.is_empty())
            .collect();
        let mut element = self.element(&before, index, "element");
        let placed = set_in(&mut element, &segments, value.clone());
        let after = match (before, placed) {
            (
                Val::List {
                    len,
                    data: Some(store),
                    elem,
                },
                true,
            ) => self
                .store_put(&store, index, &element)
                .map(|data| Val::List {
                    len,
                    data: Some(data),
                    elem,
                }),
            _ => None,
        };
        let Some(after) = after else {
            return false;
        };
        self.write_place(state, list, after);
        true
    }

    // ----- Matches ----------------------------------------------------------

    /// The enum a `match` reads, when the place it reads holds one the
    /// encoding left out: a value deep inside a parameter. Its arms then
    /// test one value, so the match is known to take one of them.
    pub(super) fn match_scrutinee(
        &mut self,
        state: &mut State,
        scrutinee: &Operand,
        value: Val,
    ) -> Val {
        let (Val::Opaque, Operand::Place(place)) = (&value, scrutinee) else {
            return value;
        };
        let ty = self.place_type(place);
        let fresh = ty.map(|ty| self.fresh(&ty, place));
        if let Some(fresh) = &fresh {
            self.write_place(state, place, fresh.clone());
        }
        fresh.unwrap_or(value)
    }

    /// Notes the path `remaining` that took none of the arms of a `match`
    /// on `value`, when it rules out every variant of an enum: no run
    /// takes it.
    pub(super) fn note_match_end(&mut self, value: &Val, remaining: &str) {
        let Val::Enum(_, tag, payloads) = value else {
            return;
        };
        let ruled_out = conjuncts(remaining);
        let exhausted = (0..payloads.len())
            .all(|index| ruled_out.contains(&format!("(not (= {tag} {index}))").as_str()));
        if exhausted {
            let dead = ruled_out.into_iter().map(str::to_string).collect();
            self.shapes.dead.push(dead);
        }
    }

    /// Whether no run takes the path `pc`: it continues one that ruled
    /// out every variant of an enum.
    fn is_dead(&self, pc: &str) -> bool {
        let held = conjuncts(pc);
        self.shapes
            .dead
            .iter()
            .any(|dead| dead.iter().all(|term| held.contains(&term.as_str())))
    }

    /// The value where paths join with values of different shapes, such
    /// as a result and the nothing that falls out of an exhaustive
    /// `match`: the value of the paths a run can take, when those agree.
    pub(super) fn merge_live(&mut self, pcs: &[String], values: &[&Val], hint: &str) -> Val {
        let (live_pcs, live): (Vec<String>, Vec<&Val>) = pcs
            .iter()
            .zip(values)
            .filter(|(pc, _)| !self.is_dead(pc))
            .map(|(pc, value)| (pc.clone(), *value))
            .unzip();
        if live.is_empty() || live.len() == values.len() {
            return Val::Opaque;
        }
        self.merge_vals(&live_pcs, &live, hint)
    }

    // ----- Counterexamples ----------------------------------------------------

    /// Remembers the variant names of the enum input whose tag is shown
    /// as `label`.
    pub(super) fn note_variants(&mut self, label: &str, names: &[String]) {
        self.shapes
            .variants
            .insert(label.to_string(), names.to_vec());
    }

    /// The text a counterexample shows for the input `label`: a variant's
    /// name where the solver gives its index.
    pub(super) fn input_text(&self, label: &str, value: String) -> String {
        self.shapes
            .variants
            .get(label)
            .and_then(|names| names.get(value.parse::<usize>().ok()?))
            .cloned()
            .unwrap_or(value)
    }

    /// The constant that says a recursive `spec def` stays within the
    /// levels the verifier unfolds. Nothing is known of it, so a check
    /// that needs more levels is not modeled instead of failed.
    pub(super) fn spec_cutoff(&mut self, name: &str, limit: usize) -> String {
        self.unmodeled(spec_cutoff_name(name, limit), |exec| {
            exec.fresh_const("Bool", "within")
        })
    }
}

/// What a `spec def` call past the unfolding limit is called, for a report.
pub(super) fn spec_cutoff_name(name: &str, limit: usize) -> String {
    format!("`spec def {name}` past {limit} levels of recursion")
}

/// Whether two unions, two enums, or a union and a value of one of its
/// members are equal.
pub(super) fn shaped_eq(left: &Val, right: &Val) -> Option<String> {
    match (left, right) {
        (Val::Union(a, xs), Val::Union(b, ys)) if xs.len() == ys.len() => {
            let mut parts = vec![format!("(= {a} {b})")];
            for (index, (x, y)) in xs.iter().zip(ys).enumerate() {
                parts.push(format!("(=> (= {a} {index}) {})", structural_eq(x, y)?));
            }
            Some(conjoin(&parts))
        }
        (Val::Union(tag, members), other) | (other, Val::Union(tag, members))
            if !matches!(other, Val::Union(..)) =>
        {
            // The one member the other value can be.
            let mut same = members
                .iter()
                .enumerate()
                .filter_map(|(index, member)| Some((index, structural_eq(member, other)?)));
            let (index, equal) = same.next()?;
            same.next()
                .is_none()
                .then(|| and(&format!("(= {tag} {index})"), &equal))
        }
        (Val::Enum(x, a, xs), Val::Enum(y, b, ys)) if x == y && xs.len() == ys.len() => {
            // A variant known on one side is the only one to compare.
            let known = match (a.parse::<usize>(), b.parse::<usize>()) {
                (Ok(a), Ok(b)) if a != b => return Some("false".to_string()),
                (Ok(known), _) | (_, Ok(known)) => Some(known),
                _ => None,
            };
            let mut parts = vec![format!("(= {a} {b})")];
            for (index, (x, y)) in xs.iter().zip(ys).enumerate() {
                if known.is_some_and(|known| known != index) {
                    continue;
                }
                let payloads: Option<Vec<String>> =
                    x.iter().zip(y).map(|(x, y)| structural_eq(x, y)).collect();
                parts.push(format!("(=> (= {a} {index}) {})", conjoin(&payloads?)));
            }
            Some(conjoin(&parts))
        }
        _ => None,
    }
}

/// The constants the scalars of a list element are read from.
fn element_arrays(value: &Val, arrays: &mut Vec<String>) {
    let term = match value {
        Val::Tuple(items) => return items.iter().for_each(|item| element_arrays(item, arrays)),
        Val::Record(_, fields) => {
            return fields
                .iter()
                .for_each(|(_, value)| element_arrays(value, arrays))
        }
        Val::List { len, .. } => Some(len.as_str()),
        scalar => scalar_term(scalar),
    };
    arrays.extend(term.into_iter().flat_map(symbols).map(str::to_string));
}

/// The terms a path condition is the conjunction of.
fn conjuncts(term: &str) -> Vec<&str> {
    let Some(inner) = term
        .strip_prefix("(and ")
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return vec![term];
    };
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut start = 0;
    for (index, ch) in inner.char_indices() {
        match ch {
            '"' => in_string = !in_string,
            '(' if !in_string => depth += 1,
            ')' if !in_string => depth -= 1,
            ' ' if !in_string && depth == 0 => {
                parts.extend(conjuncts(&inner[start..index]));
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.extend(conjuncts(&inner[start..]));
    parts
}
