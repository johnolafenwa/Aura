//! Specialized generic functions (Rust-speed step 2b).
//!
//! Native code compiles a generic user function once for each list of
//! concrete type arguments its calls use, as Rust does, instead of once for
//! values it can only hold boxed. A copy's parameters, locals, and result
//! have the concrete types, so the passes after this one resolve its member
//! calls, runtime operations, checks, unions, and ownership as they would
//! for code written for those types.
//!
//! `specialize_generic_functions` runs in `mir::lower`, after
//! `type_param_calls` and before runtime operations are resolved. It starts
//! from every function and follows calls:
//!
//! - a `Name` call to a generic function whose type arguments are concrete
//!   calls the copy for those arguments, named `function[arg, ...]` with the
//!   arguments in type parameter name order;
//! - a member call or a `with` exit that runs a generic method gets a copy
//!   too. The call keeps its member form, and `mir::methods` resolves it to
//!   the copy, so the method validator agrees.
//!
//! A copy brings the functions its generic original owns, its lambdas and
//! default helpers, renamed under the copy. The generic originals stay in
//! the module, and a call keeps calling one when a type argument names a
//! type parameter or a type lowering left unknown, when an argument's type
//! is not the one its parameter takes under the type arguments, when a type
//! argument nests deeper than a layout can ([`MAX_LAYOUT_TYPE_DEPTH`]), or
//! when the module already has [`SPECIALIZATION_LIMIT`] copies. So does a
//! call whose copy would merge union members, such as `T | int64` with
//! `T = int64`, since its union tags would change meaning.
//!
//! `lower_implicit` skips the pass, so `aura run` pays nothing: the
//! interpreter reads values, not layouts.

use std::collections::{BTreeMap, HashMap};

use aura_core::layout::{type_depth, MAX_LAYOUT_TYPE_DEPTH};

use super::type_args::{
    bind_type_params, callee_type_params, operand_layout_type, place_layout_type,
};
use super::{
    AssertionCapture, BasicBlock, CallTarget, Instruction, MirArg, MirClosureCapture,
    MirExternCall, MirFormatPart, MirFunction, MirLoanValidationContext, MirModule, Operand,
    Rvalue, Terminator,
};
use crate::sema::{substitute_type, Type};

/// The most copies one module may create. Polymorphic recursion, such as
/// `f[T]` calling `f[list[T]]`, stops at the depth limit first; this bounds
/// how many functions a program may make copies of.
const SPECIALIZATION_LIMIT: usize = 4096;

/// The name of `template`'s copy for `type_args`.
pub(super) fn specialized_name(template: &str, type_args: &BTreeMap<String, Type>) -> String {
    let args = type_args
        .values()
        .map(Type::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    format!("{template}[{args}]")
}

/// Whether a type fixes a layout: it names no type parameter and no type
/// lowering left unknown.
fn is_concrete(ty: &Type) -> bool {
    match ty {
        Type::TypeParam(_) | Type::Module(_) => false,
        Type::Named(name, args) => name != "Unknown" && args.iter().all(is_concrete),
        Type::Tuple(elements) => elements.iter().all(is_concrete),
        Type::Union(union) => union.members.iter().all(is_concrete),
        Type::Function {
            params,
            return_type,
        } => params.iter().all(|param| is_concrete(&param.ty)) && is_concrete(return_type),
        Type::Closure {
            params,
            return_type,
            captures,
            ..
        } => {
            params.iter().all(|param| is_concrete(&param.ty))
                && captures.iter().all(|capture| is_concrete(&capture.ty))
                && is_concrete(return_type)
        }
        Type::Callable(callable) => {
            callable.params.iter().all(|param| is_concrete(&param.ty))
                && is_concrete(&callable.return_type)
        }
        Type::ReturnedView(view) => is_concrete(&view.pointee),
        Type::Unit => true,
    }
}

/// One argument of a call, as the binding of type parameters reads it.
enum Actual {
    /// A value of a known type.
    Typed(Type),
    /// An integer literal, which takes the integer type its parameter
    /// does.
    Int,
}

/// The type arguments of a call to the generic `callee`, when the call
/// fixes every type parameter to a concrete type and each argument has
/// exactly the type its parameter takes under them.
///
/// `receiver` is the type a method runs on; a `Name` call to a method
/// passes its receiver as the first argument instead. `target` is the
/// place the call's result goes to.
pub(super) fn concrete_call_type_args(
    context: &MirLoanValidationContext<'_>,
    function: &MirFunction,
    callee: &MirFunction,
    receiver: Option<&Type>,
    args: &[MirArg],
    target: Option<&str>,
) -> Option<BTreeMap<String, Type>> {
    let type_params = callee_type_params(callee);
    if type_params.is_empty() {
        return None;
    }
    let actual = |operand: &Operand| match operand {
        Operand::Int(_) => Some(Actual::Int),
        other => operand_layout_type(context, function, other).map(Actual::Typed),
    };
    let mut pairs = Vec::new();
    let mut args = args.iter();
    if callee.receiver.is_some() {
        let self_type = &callee
            .local_types
            .iter()
            .find(|local| local.name == "self")?
            .ty;
        let receiver = match receiver {
            Some(receiver) => Actual::Typed(receiver.clone()),
            None => actual(&args.next()?.value)?,
        };
        pairs.push((self_type, receiver));
    }
    let mut positional = callee.params.iter();
    for arg in args {
        let param = match &arg.name {
            Some(name) => callee.params.iter().find(|param| param.name == *name)?,
            None => positional.next()?,
        };
        pairs.push((&param.ty, actual(&arg.value)?));
    }
    let destination = target
        .and_then(|target| place_layout_type(context, function, target))
        .filter(|_| !matches!(callee.return_type, Type::ReturnedView(_)));
    if let Some(destination) = destination {
        pairs.push((&callee.return_type, Actual::Typed(destination)));
    }

    // Values bind first, then literals as `int64`, as a literal alone
    // would infer.
    let mut bindings = HashMap::new();
    for (pattern, actual) in &pairs {
        if let Actual::Typed(actual) = actual {
            bind_members(pattern, actual, &mut bindings);
        }
    }
    for (pattern, actual) in &pairs {
        if let Actual::Int = actual {
            bind_type_params(pattern, &Type::named("int64"), &mut bindings);
        }
    }
    let type_args = type_params
        .into_iter()
        .map(|name| {
            let ty = bindings.get(&name).filter(|ty| is_concrete(ty))?.clone();
            Some((name, ty))
        })
        .collect::<Option<BTreeMap<_, _>>>()?;

    let substitutions = type_args.clone().into_iter().collect::<HashMap<_, _>>();
    let fits = pairs.iter().all(|(pattern, actual)| {
        let expected = substitute_type(pattern, &substitutions);
        match actual {
            Actual::Typed(actual) => expected == *actual,
            Actual::Int => crate::sema::integer_type_bounds(&expected).is_some(),
        }
    });
    fits.then_some(type_args)
}

/// Binds the type parameters `pattern` names to the matching parts of
/// `actual`, as `bind_type_params` does. A union pattern with one member
/// that names a type parameter, such as `T | None`, also binds that member
/// to the one member of an `actual` union its other members leave.
fn bind_members(pattern: &Type, actual: &Type, bindings: &mut HashMap<String, Type>) {
    let (Type::Union(pattern_union), Type::Union(actual_union)) = (pattern, actual) else {
        bind_type_params(pattern, actual, bindings);
        return;
    };
    let (generic, fixed): (Vec<_>, Vec<_>) = pattern_union
        .members
        .iter()
        .partition(|member| !is_concrete(member));
    let rest = actual_union
        .members
        .iter()
        .filter(|member| !fixed.contains(member))
        .collect::<Vec<_>>();
    if let ([generic], [rest]) = (generic.as_slice(), rest.as_slice()) {
        if fixed.len() + 1 == actual_union.members.len() {
            bind_type_params(generic, rest, bindings);
        }
    }
}

/// Copies each generic function once for each concrete type argument list
/// its calls use, and points the calls at the copies.
pub(super) fn specialize_generic_functions(module: &mut MirModule) {
    let (rewritten, copies) = {
        // A module the validator cannot read keeps its calls; the validator
        // reports what is wrong with it.
        let Ok(context) = MirLoanValidationContext::new(module) else {
            return;
        };
        let mut specializer = Specializer {
            context: &context,
            module,
            created: BTreeMap::new(),
            queue: Vec::new(),
        };
        let functions = module
            .functions
            .iter()
            .map(Some)
            .chain([module.top_level.as_ref()]);
        let mut rewritten = Vec::new();
        for (index, function) in functions.enumerate() {
            let Some(function) = function else {
                continue;
            };
            let replacements = specializer.replacements(function);
            if !replacements.is_empty() {
                rewritten.push((index, replacements));
            }
        }
        let mut copies = Vec::new();
        while let Some(mut function) = specializer.queue.pop() {
            let replacements = specializer.replacements(&function);
            apply(&mut function, replacements);
            copies.push(function);
        }
        (rewritten, copies)
    };
    for (index, replacements) in rewritten {
        let function = match module.functions.get_mut(index) {
            Some(function) => function,
            None => module
                .top_level
                .as_mut()
                .expect("a function past the module's functions is its top level"),
        };
        apply(function, replacements);
    }
    module.functions.extend(copies);
}

/// What changes at one instruction of a function: its block, its index,
/// and the change.
type Rewrite = (usize, usize, Replacement);

fn apply(function: &mut MirFunction, rewrites: Vec<Rewrite>) {
    for (block, index, replacement) in rewrites {
        let (Instruction::Assign { value, .. } | Instruction::WriteLoan { value, .. }) =
            &mut function.blocks[block].instructions[index]
        else {
            continue;
        };
        match replacement {
            Replacement::Callee(name) => {
                if let Rvalue::Call { callee, .. } = value {
                    *callee = CallTarget::Name(name);
                }
            }
            Replacement::Rvalue(rvalue) => *value = rvalue,
        }
    }
}

/// `name` is a function `template` owns, such as one of its lambdas
/// (`template::__lambda_3_9`) or default helpers.
fn owned_by(name: &str, template: &str) -> bool {
    name.strip_prefix(template)
        .is_some_and(|rest| rest.starts_with("::__"))
}

struct Specializer<'a, 'm> {
    context: &'a MirLoanValidationContext<'m>,
    module: &'m MirModule,
    /// Each copy made, or refused, by name.
    created: BTreeMap<String, bool>,
    /// Copies whose calls are not rewritten yet.
    queue: Vec<MirFunction>,
}

impl Specializer<'_, '_> {
    /// The copy of `template` for `type_args`, made on first use. `None`
    /// when no copy can be made.
    fn copy_of(&mut self, template: &str, type_args: &BTreeMap<String, Type>) -> Option<String> {
        let name = specialized_name(template, type_args);
        if let Some(made) = self.created.get(&name) {
            return made.then_some(name);
        }
        if self.created.len() >= SPECIALIZATION_LIMIT
            || type_args
                .values()
                .any(|ty| type_depth(ty) > MAX_LAYOUT_TYPE_DEPTH)
        {
            return None;
        }
        let substitution = Substitution {
            substitutions: type_args.clone().into_iter().collect(),
            template,
            copy: &name,
        };
        let family = self
            .module
            .functions
            .iter()
            .filter(|function| function.name == template || owned_by(&function.name, template))
            .map(|function| substitution.function(function))
            .collect::<std::result::Result<Vec<_>, String>>();
        let made = family.is_ok();
        self.created.insert(name.clone(), made);
        // The queue pops from its end, so the template's copy, first in
        // the family, goes in last and is appended first.
        self.queue.extend(family.into_iter().flatten().rev());
        made.then_some(name)
    }

    /// How `function`'s calls change to run copies, making the copies
    /// they need.
    fn replacements(&mut self, function: &MirFunction) -> Vec<Rewrite> {
        let mut rewrites = Vec::new();
        for (block_index, block) in function.blocks.iter().enumerate() {
            for (index, instruction) in block.instructions.iter().enumerate() {
                let replacement = match instruction {
                    Instruction::Assign { target, value }
                    | Instruction::WriteLoan {
                        loan: target,
                        value,
                    } => self.call_replacement(function, value, target),
                    Instruction::PushCleanup { place, action, .. } => {
                        if let Some((method, receiver)) =
                            super::methods::cleanup_method(self.context, function, place, action)
                        {
                            self.method_copy(function, &method, &receiver, &[], None);
                        }
                        None
                    }
                    _ => None,
                };
                if let Some(replacement) = replacement {
                    rewrites.push((block_index, index, replacement));
                }
            }
        }
        rewrites
    }

    /// What an rvalue in `function` that stores into `target` becomes,
    /// after making the copies it runs.
    fn call_replacement(
        &mut self,
        function: &MirFunction,
        value: &Rvalue,
        target: &str,
    ) -> Option<Replacement> {
        match value {
            Rvalue::Call {
                callee: CallTarget::Name(name),
                args,
                ..
            } => {
                let callee = self.context.functions.get(name.as_str())?;
                let type_args = concrete_call_type_args(
                    self.context,
                    function,
                    callee,
                    None,
                    args,
                    Some(target),
                )?;
                self.copy_of(name, &type_args).map(Replacement::Callee)
            }
            Rvalue::Call {
                callee: callee @ (CallTarget::Member { .. } | CallTarget::TraitMember { .. }),
                args,
                ..
            } => {
                for (method, receiver) in
                    super::methods::member_call_methods(self.context, function, callee, args)
                {
                    self.method_copy(function, &method, &receiver, args, Some(target));
                }
                None
            }
            Rvalue::NoneTest { value } => self.none_test(function, value).map(Replacement::Rvalue),
            _ => None,
        }
    }

    /// Makes the copy of the generic method `method` that a call on
    /// `receiver` runs. The method resolver points the call at it.
    fn method_copy(
        &mut self,
        function: &MirFunction,
        method: &str,
        receiver: &Type,
        args: &[MirArg],
        target: Option<&str>,
    ) {
        let Some(callee) = self.context.functions.get(method) else {
            return;
        };
        if let Some(type_args) =
            concrete_call_type_args(self.context, function, callee, Some(receiver), args, target)
        {
            self.copy_of(method, &type_args);
        }
    }

    /// A `None` test on a place a copy gave a concrete type: a constant, or
    /// a tag test of its union's `None` member.
    fn none_test(&self, function: &MirFunction, value: &Operand) -> Option<Rvalue> {
        let Operand::Place(place) = value else {
            return None;
        };
        let ty = place_layout_type(self.context, function, place).filter(is_concrete)?;
        let test = match &ty {
            Type::Union(union) => {
                match union
                    .members
                    .iter()
                    .position(|member| *member == Type::Unit)
                {
                    Some(member_index) => Rvalue::UnionTagTest {
                        place: place.clone(),
                        union_type: ty.clone(),
                        member_index,
                    },
                    None => Rvalue::Use(Operand::Bool(false)),
                }
            }
            ty => Rvalue::Use(Operand::Bool(*ty == Type::Unit)),
        };
        Some(test)
    }
}

enum Replacement {
    Callee(String),
    Rvalue(Rvalue),
}

/// How a copy differs from its template: each type parameter is replaced,
/// and each function the template owns is renamed under the copy.
struct Substitution<'a> {
    substitutions: HashMap<String, Type>,
    template: &'a str,
    copy: &'a str,
}

impl Substitution<'_> {
    /// The copy of `function`, the template or a function it owns.
    fn function(&self, function: &MirFunction) -> std::result::Result<MirFunction, String> {
        let mut out = function.clone();
        out.name = if function.name == self.template {
            self.copy.to_string()
        } else {
            self.rename(&function.name)
        };
        for param in &mut out.params {
            param.ty = self.ty(&param.ty);
            if let Some(helper) = &mut param.default_function {
                *helper = self.rename(helper);
            }
        }
        for local in &mut out.local_types {
            local.ty = self.ty(&local.ty);
        }
        out.return_type = self.ty(&out.return_type);
        for block in &mut out.blocks {
            self.block(block)?;
        }
        Ok(out)
    }

    fn ty(&self, ty: &Type) -> Type {
        substitute_type(ty, &self.substitutions)
    }

    /// A function name, moved under the copy when the template owns it. A
    /// call to the template itself keeps its name: the copy's calls are
    /// pointed at copies for their own type arguments afterwards.
    fn rename(&self, name: &str) -> String {
        if owned_by(name, self.template) {
            format!("{}{}", self.copy, &name[self.template.len()..])
        } else {
            name.to_string()
        }
    }

    /// The new union type and member index of a union operation. A
    /// substitution that merges or loses members has none.
    fn union(
        &self,
        union_type: &mut Type,
        member_index: &mut usize,
    ) -> std::result::Result<(), String> {
        let after = self.ty(union_type);
        let (Type::Union(before_union), Type::Union(after_union)) = (&*union_type, &after) else {
            return Err("a union type stops being a union".to_string());
        };
        if before_union.members.len() != after_union.members.len() {
            return Err("a union type merges members".to_string());
        }
        let member = before_union
            .members
            .get(*member_index)
            .map(|member| self.ty(member))
            .ok_or("a union member index is out of range")?;
        *member_index = after_union
            .members
            .iter()
            .position(|candidate| *candidate == member)
            .ok_or("a union member is lost")?;
        *union_type = after;
        Ok(())
    }

    fn block(&self, block: &mut BasicBlock) -> std::result::Result<(), String> {
        for instruction in &mut block.instructions {
            match instruction {
                Instruction::Assign { value, .. } | Instruction::WriteLoan { value, .. } => {
                    self.rvalue(value)?
                }
                Instruction::Eval { value } => self.operand(value),
                Instruction::BeginElementLoan { selector, .. }
                | Instruction::ReborrowElement { selector, .. } => self.operand(selector),
                Instruction::Safepoint
                | Instruction::Retain { .. }
                | Instruction::Release { .. }
                | Instruction::Lend { .. }
                | Instruction::EndLend { .. }
                | Instruction::IndexInRange { .. }
                | Instruction::BeginLoan { .. }
                | Instruction::BeginReturnedLoan { .. }
                | Instruction::Reborrow { .. }
                | Instruction::ReadLoan { .. }
                | Instruction::EndLoan { .. }
                | Instruction::ReturnLoan { .. }
                | Instruction::PushCleanup { .. }
                | Instruction::PopCleanup { .. } => {}
            }
        }
        match &mut block.terminator {
            Terminator::Return(value)
            | Terminator::Branch {
                condition: value, ..
            }
            | Terminator::ForRange {
                iterable: value, ..
            }
            | Terminator::Match {
                scrutinee: value, ..
            } => self.operand(value),
            Terminator::AssertFail {
                message, captures, ..
            } => {
                self.operands(message.iter_mut());
                for AssertionCapture { ty, value, .. } in captures {
                    self.types([ty]);
                    self.operand(value);
                }
            }
            Terminator::Goto(_) | Terminator::Unreachable => {}
        }
        Ok(())
    }

    fn operand(&self, operand: &mut Operand) {
        if let Operand::Function { name, signature } = operand {
            *name = self.rename(name);
            **signature = self.ty(signature);
        }
    }

    fn operands<'o>(&self, operands: impl IntoIterator<Item = &'o mut Operand>) {
        for operand in operands {
            self.operand(operand);
        }
    }

    fn types<'t>(&self, types: impl IntoIterator<Item = &'t mut Type>) {
        for ty in types {
            *ty = self.ty(ty);
        }
    }

    fn rvalue(&self, rvalue: &mut Rvalue) -> std::result::Result<(), String> {
        match rvalue {
            Rvalue::Use(value)
            | Rvalue::NoneTest { value }
            | Rvalue::Unary { value, .. }
            | Rvalue::Try { value, .. }
            | Rvalue::VariantPayload {
                scrutinee: value, ..
            }
            | Rvalue::Member { object: value, .. } => self.operand(value),
            Rvalue::CallableAdapt { value, destination } => {
                self.operand(value);
                self.types([destination]);
            }
            Rvalue::UnionTagTest {
                union_type,
                member_index,
                ..
            } => self.union(union_type, member_index)?,
            Rvalue::UnionTakePayload {
                union_type,
                member_type,
                member_index,
                ..
            } => {
                self.union(union_type, member_index)?;
                self.types([member_type]);
            }
            Rvalue::UnionInject {
                value,
                union_type,
                member_type,
                member_index,
            } => {
                self.operand(value);
                self.union(union_type, member_index)?;
                self.types([member_type]);
            }
            Rvalue::ModuleConstant { .. } => {}
            Rvalue::Closure {
                function,
                signature,
                captures,
                ..
            } => {
                *function = self.rename(function);
                self.types([signature]);
                for MirClosureCapture { value, ty, .. } in captures {
                    self.operand(value);
                    self.types([ty]);
                }
            }
            Rvalue::FormatString { parts } => {
                for part in parts {
                    match part {
                        MirFormatPart::Literal(_) => {}
                        MirFormatPart::Value(value) | MirFormatPart::Int64(value) => {
                            self.operand(value)
                        }
                        MirFormatPart::Formatted {
                            value, value_type, ..
                        } => {
                            self.operand(value);
                            self.types([value_type]);
                        }
                    }
                }
            }
            Rvalue::Cast { value, ty, .. } => {
                self.operand(value);
                self.types([ty]);
            }
            Rvalue::StartTask {
                stack_size,
                task_group,
                function,
                args,
                ..
            } => {
                self.operands(stack_size.iter_mut().chain([task_group, function]));
                self.operands(args.iter_mut().map(|arg| &mut arg.value));
            }
            Rvalue::Binary { left, right, .. } => self.operands([left, right]),
            Rvalue::Call {
                callee,
                args,
                type_args,
            } => {
                match callee {
                    CallTarget::Name(_) => {}
                    CallTarget::Value(value) => self.operand(value),
                    CallTarget::Extern(MirExternCall {
                        params,
                        return_type,
                        ..
                    }) => {
                        self.types(params.iter_mut().map(|param| &mut param.ty));
                        self.types([return_type]);
                    }
                    CallTarget::Member { object, .. } | CallTarget::TraitMember { object, .. } => {
                        self.operand(object)
                    }
                    CallTarget::Runtime { receiver, .. } => self.operands(receiver.iter_mut()),
                }
                self.operands(args.iter_mut().map(|arg| &mut arg.value));
                self.types(type_args.values_mut());
            }
            Rvalue::VecLiteral {
                elements,
                element_type,
            }
            | Rvalue::SetLiteral {
                elements,
                element_type,
            } => {
                self.operands(elements.iter_mut());
                self.types([element_type]);
            }
            Rvalue::TupleLiteral {
                elements,
                element_types,
            } => {
                self.operands(elements.iter_mut());
                self.types(element_types.iter_mut());
            }
            Rvalue::TupleElement {
                tuple,
                element_type,
                ..
            } => {
                self.operand(tuple);
                self.types([element_type]);
            }
            Rvalue::TupleTakeElement { element_type, .. } => self.types([element_type]),
            Rvalue::MapLiteral {
                entries,
                key_type,
                value_type,
            } => {
                for entry in entries {
                    self.operands([&mut entry.key, &mut entry.value]);
                }
                self.types([key_type, value_type]);
            }
            Rvalue::Construct { fields, .. } => {
                self.operands(fields.iter_mut().map(|field| &mut field.value))
            }
            Rvalue::EnumVariant { payloads, .. } => self.operands(payloads.iter_mut()),
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "mir_specialize_tests.rs"]
mod tests;
