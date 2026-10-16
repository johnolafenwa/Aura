//! Semantic type identity shared by the checker, MIR, and the runtime.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::diag::{Diagnostic, Result};
use crate::ops::ReceiverKind;
use crate::type_budget::ExpansionBudget;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TraitBound {
    pub trait_name: String,
    pub trait_args: Vec<Type>,
}

impl fmt::Display for TraitBound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.trait_args.is_empty() {
            write!(f, "{}", self.trait_name)
        } else {
            write!(
                f,
                "{}[{}]",
                self.trait_name,
                self.trait_args
                    .iter()
                    .map(Type::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Type {
    Union(Box<UnionType>),
    Named(String, Vec<Type>),
    Tuple(Vec<Type>),
    Function {
        params: Vec<FunctionParamContract>,
        return_type: Box<Type>,
    },
    Closure {
        params: Box<Vec<FunctionParamContract>>,
        return_type: Box<Type>,
        /// Kept indirect so closure-only ownership metadata does not inflate
        /// every `Type`, and consequently every typed runtime collection.
        ///
        /// `Box<Vec<_>>` remains transparent to Serde, preserving the
        /// established semantic-interface and MIR cache schema.
        captures: Box<Vec<ClosureCapture>>,
        call_kind: ClosureCallKind,
    },
    /// `Callable[...]` / `TaskCallable[...]` owned storage with an erased
    /// environment; boxed so `Type` keeps its size budget.
    Callable(Box<CallableType>),
    /// The result position of a callable value whose call returns a view of
    /// one explicit ordinary argument (C9, Q22 A): `-> view [mut] T from
    /// name`, encoded by the origin parameter's ordinal. It is legal only as
    /// the `return_type` of `Function`, `Closure`, or `Callable`; a call
    /// through such a value has the pointee type and binds a returned view.
    ReturnedView(Box<ReturnedViewType>),
    TypeParam(String),
    Module(String),
    Unit,
}

/// A stored callable's argument-origin returned-view contract (C9).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReturnedViewType {
    pub mutable: bool,
    pub pointee: Type,
    /// Ordinal of the origin parameter in the callable's parameter list.
    pub origin: usize,
}

/// Normalized members and their defining-module structural identities.
/// Keys determine tags; written aliases and source ordering do not.
#[derive(Clone, Debug, Serialize)]
pub struct UnionType {
    pub members: Vec<Type>,
    pub keys: Vec<String>,
    pub module_name: String,
}

impl<'de> Deserialize<'de> for UnionType {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct EncodedUnion {
            members: Vec<Type>,
            keys: Vec<String>,
            module_name: String,
        }
        let encoded = EncodedUnion::deserialize(deserializer)?;
        let invalid =
            |reason: &str| serde::de::Error::custom(format!("invalid union metadata: {reason}"));
        if encoded.members.len() < 2 {
            return Err(invalid("a stored union requires at least two members"));
        }
        if encoded.members.len() != encoded.keys.len() {
            return Err(invalid("member and key counts differ"));
        }
        if encoded.keys.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(invalid("keys must be strictly ordered and unique"));
        }
        for (member, key) in encoded.members.iter().zip(&encoded.keys) {
            if matches!(member, Type::Union(_)) {
                return Err(invalid("nested members must be flattened"));
            }
            ExpansionBudget::default()
                .check_canonical_key(
                    member,
                    &encoded.module_name,
                    &BTreeMap::new(),
                    crate::diag::Span::new(1, 1),
                )
                .map_err(|error| invalid(&error.message))?;
            if member.canonical_key(&encoded.module_name, &BTreeMap::new()) != *key {
                return Err(invalid("member key does not match its resolved type"));
            }
        }
        Ok(Self {
            members: encoded.members,
            keys: encoded.keys,
            module_name: encoded.module_name,
        })
    }
}

impl Type {
    pub fn canonical_key(
        &self,
        module_name: &str,
        canonical_names: &BTreeMap<String, String>,
    ) -> String {
        fn key(ty: &Type, module: &str, names: &BTreeMap<String, String>) -> serde_json::Value {
            use serde_json::json;
            match ty {
                Type::Unit => json!(["~unit"]),
                Type::Module(name) => json!(["module", name]),
                Type::TypeParam(name) => json!(["parameter", name]),
                Type::ReturnedView(view) => json!([
                    "returned_view",
                    view.mutable,
                    view.origin,
                    key(&view.pointee, module, names)
                ]),
                Type::Named(name, args) => {
                    let resolved = names.get(name).unwrap_or(name);
                    let nominal =
                        if is_builtin_type(resolved) || resolved.contains('.') || module.is_empty()
                        {
                            resolved.clone()
                        } else {
                            format!("{module}.{resolved}")
                        };
                    json!([
                        "named",
                        nominal,
                        args.iter()
                            .map(|arg| key(arg, module, names))
                            .collect::<Vec<_>>()
                    ])
                }
                Type::Tuple(elements) => json!([
                    "tuple",
                    elements
                        .iter()
                        .map(|arg| key(arg, module, names))
                        .collect::<Vec<_>>()
                ]),
                Type::Union(union) => json!([
                    "union",
                    union
                        .members
                        .iter()
                        .map(|member| key(member, &union.module_name, &BTreeMap::new()))
                        .collect::<Vec<_>>()
                ]),
                Type::Function {
                    params,
                    return_type,
                } => json!([
                    "function",
                    params
                        .iter()
                        .map(|p| json!([
                            p.name,
                            p.passing,
                            p.has_default,
                            p.keyword_only,
                            key(&p.ty, module, names)
                        ]))
                        .collect::<Vec<_>>(),
                    key(return_type, module, names)
                ]),
                Type::Callable(callable) => json!([
                    "callable",
                    callable.task,
                    callable.call_kind,
                    callable
                        .params
                        .iter()
                        .map(|p| json!([
                            p.name,
                            p.passing,
                            p.has_default,
                            p.keyword_only,
                            key(&p.ty, module, names)
                        ]))
                        .collect::<Vec<_>>(),
                    key(&callable.return_type, module, names)
                ]),
                Type::Closure {
                    params,
                    return_type,
                    captures,
                    call_kind,
                } => json!([
                    "closure",
                    call_kind,
                    params
                        .iter()
                        .map(|p| json!([
                            p.name,
                            p.passing,
                            p.has_default,
                            p.keyword_only,
                            key(&p.ty, module, names)
                        ]))
                        .collect::<Vec<_>>(),
                    key(return_type, module, names),
                    captures
                        .iter()
                        .map(|capture| json!([
                            capture.name,
                            capture.mode,
                            key(&capture.ty, module, names)
                        ]))
                        .collect::<Vec<_>>()
                ]),
            }
        }
        format!(
            "aura-type-key-v1:{}",
            key(self, module_name, canonical_names)
        )
    }

    pub fn normalize_union(
        members: Vec<Type>,
        module_name: &str,
        canonical_names: &BTreeMap<String, String>,
    ) -> Result<Type> {
        Self::normalize_union_with_budget(
            members,
            module_name,
            canonical_names,
            &ExpansionBudget::default(),
            crate::diag::Span::new(1, 1),
        )
    }

    pub fn normalize_union_with_budget(
        members: Vec<Type>,
        module_name: &str,
        canonical_names: &BTreeMap<String, String>,
        budget: &ExpansionBudget,
        span: crate::diag::Span,
    ) -> Result<Type> {
        let mut ordered = BTreeMap::new();
        for member in members {
            if let Type::Union(union) = member {
                if union.members.len() != union.keys.len() {
                    return Err(Diagnostic::coded("AU2010", "invalid union member metadata"));
                }
                for (member, key) in union.members.into_iter().zip(union.keys) {
                    ordered.entry(key).or_insert(member);
                }
            } else {
                budget.check_canonical_key(&member, module_name, canonical_names, span)?;
                ordered
                    .entry(member.canonical_key(module_name, canonical_names))
                    .or_insert(member);
            }
        }
        match ordered.len() {
            0 => Err(Diagnostic::coded(
                "AU2010",
                "a union requires at least one member",
            )),
            1 => Ok(ordered.into_values().next().expect("one normalized member")),
            _ => {
                let (keys, members) = ordered.into_iter().unzip();
                Ok(Type::Union(Box::new(UnionType {
                    members,
                    keys,
                    module_name: module_name.to_string(),
                })))
            }
        }
    }
}

/// Renders a complete callable contract: slot names, the keyword-only
/// boundary, capabilities, types, and default availability.
fn write_contract_params(
    f: &mut fmt::Formatter<'_>,
    params: &[FunctionParamContract],
) -> fmt::Result {
    let mut boundary_written = false;
    for (index, param) in params.iter().enumerate() {
        if index > 0 {
            write!(f, ", ")?;
        }
        if param.keyword_only && !boundary_written {
            write!(f, "*, ")?;
            boundary_written = true;
        }
        if !param.name.is_empty() {
            write!(f, "{}: ", param.name)?;
        }
        match param.passing {
            ReceiverKind::Borrow => {}
            ReceiverKind::BorrowMut => write!(f, "mut ")?,
            ReceiverKind::Value => write!(f, "own ")?,
        }
        write!(f, "{}", param.ty)?;
        if param.has_default {
            write!(f, " = ...")?;
        }
    }
    Ok(())
}

/// Writes `) -> R`, parenthesizing a union result, or `) -> view [mut] T
/// from name` for a stored argument-origin view contract (C9).
fn write_return_contract(
    f: &mut fmt::Formatter<'_>,
    params: &[FunctionParamContract],
    return_type: &Type,
) -> fmt::Result {
    match return_type {
        Type::ReturnedView(view) => {
            let origin = params
                .get(view.origin)
                .map(|param| param.name.as_str())
                .filter(|name| !name.is_empty())
                .unwrap_or("?");
            write!(
                f,
                ") -> view {}{} from {origin}",
                if view.mutable { "mut " } else { "" },
                view.pointee
            )
        }
        Type::Union(_) => write!(f, ") -> ({return_type})"),
        _ => write!(f, ") -> {return_type}"),
    }
}

impl PartialEq for Type {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Union(left), Self::Union(right)) => left.keys == right.keys,
            (Self::Named(left_name, left_args), Self::Named(right_name, right_args)) => {
                left_name == right_name && left_args == right_args
            }
            (Self::Tuple(left), Self::Tuple(right)) => left == right,
            (
                Self::Function {
                    params: left_params,
                    return_type: left_return,
                },
                Self::Function {
                    params: right_params,
                    return_type: right_return,
                },
            ) => {
                left_params.len() == right_params.len()
                    && left_params
                        .iter()
                        .zip(right_params.iter())
                        .all(|(left, right)| left.ty == right.ty && left.passing == right.passing)
                    && left_return == right_return
            }
            (
                Self::Closure {
                    params: left_params,
                    return_type: left_return,
                    captures: left_captures,
                    call_kind: left_call_kind,
                },
                Self::Closure {
                    params: right_params,
                    return_type: right_return,
                    captures: right_captures,
                    call_kind: right_call_kind,
                },
            ) => {
                left_params.len() == right_params.len()
                    && left_params
                        .iter()
                        .zip(right_params.iter())
                        .all(|(left, right)| left.ty == right.ty && left.passing == right.passing)
                    && left_return == right_return
                    && left_captures == right_captures
                    && left_call_kind == right_call_kind
            }
            (Self::Callable(left), Self::Callable(right)) => {
                left.task == right.task
                    && left.call_kind == right.call_kind
                    && left.params.len() == right.params.len()
                    && left
                        .params
                        .iter()
                        .zip(right.params.iter())
                        .all(|(left, right)| left.ty == right.ty && left.passing == right.passing)
                    && left.return_type == right.return_type
            }
            (Self::ReturnedView(left), Self::ReturnedView(right)) => {
                left.mutable == right.mutable
                    && left.origin == right.origin
                    && left.pointee == right.pointee
            }
            (Self::TypeParam(left), Self::TypeParam(right))
            | (Self::Module(left), Self::Module(right)) => left == right,
            (Self::Unit, Self::Unit) => true,
            _ => false,
        }
    }
}

impl Eq for Type {}

impl Type {
    pub fn named(name: impl Into<String>) -> Self {
        Self::Named(name.into(), Vec::new())
    }

    /// Complete-contract identity (C5, Q17 A, Q19 A): ABI equality plus,
    /// at every callable position reachable through the type, equal slot
    /// names, keyword-only boundaries, default availability, capabilities,
    /// closure captures and call kinds, and the task flag of an erased
    /// contract. `PartialEq` deliberately stays ABI-only for the runtimes;
    /// the shared validator uses this comparison wherever runtime-selected
    /// candidates merge, so a disagreement in any part of the contract
    /// poisons the merged identity instead of silently adopting one side.
    pub fn identical_contract(&self, other: &Self) -> bool {
        fn params_identical(
            left: &[FunctionParamContract],
            right: &[FunctionParamContract],
        ) -> bool {
            left.len() == right.len()
                && left.iter().zip(right).all(|(left, right)| {
                    left.name == right.name
                        && left.keyword_only == right.keyword_only
                        && left.has_default == right.has_default
                        && left.passing == right.passing
                        && left.ty.identical_contract(&right.ty)
                })
        }
        fn all_identical(left: &[Type], right: &[Type]) -> bool {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| left.identical_contract(right))
        }
        if self != other {
            return false;
        }
        match (self, other) {
            (
                Self::Function {
                    params: left_params,
                    return_type: left_return,
                },
                Self::Function {
                    params: right_params,
                    return_type: right_return,
                },
            ) => {
                params_identical(left_params, right_params)
                    && left_return.identical_contract(right_return)
            }
            (
                Self::Closure {
                    params: left_params,
                    return_type: left_return,
                    captures: left_captures,
                    call_kind: left_call_kind,
                },
                Self::Closure {
                    params: right_params,
                    return_type: right_return,
                    captures: right_captures,
                    call_kind: right_call_kind,
                },
            ) => {
                left_call_kind == right_call_kind
                    && params_identical(left_params, right_params)
                    && left_return.identical_contract(right_return)
                    && left_captures.len() == right_captures.len()
                    && left_captures
                        .iter()
                        .zip(right_captures.iter())
                        .all(|(left, right)| {
                            left.name == right.name
                                && left.mode == right.mode
                                && left.mutated == right.mutated
                                && left.ty.identical_contract(&right.ty)
                        })
            }
            (Self::Callable(left), Self::Callable(right)) => {
                left.task == right.task
                    && left.call_kind == right.call_kind
                    && params_identical(&left.params, &right.params)
                    && left.return_type.identical_contract(&right.return_type)
            }
            (Self::Named(_, left_args), Self::Named(_, right_args)) => {
                all_identical(left_args, right_args)
            }
            (Self::Tuple(left), Self::Tuple(right)) => all_identical(left, right),
            (Self::Union(left), Self::Union(right)) => all_identical(&left.members, &right.members),
            (Self::ReturnedView(left), Self::ReturnedView(right)) => {
                left.pointee.identical_contract(&right.pointee)
            }
            _ => true,
        }
    }

    pub fn is_copy(&self) -> bool {
        match self {
            Type::Union(_) => false,
            Type::Unit => true,
            Type::Module(_) => false,
            Type::ReturnedView(_) => false,
            Type::TypeParam(_) => false,
            Type::Tuple(elements) => elements.iter().all(Type::is_copy),
            Type::Function { .. } => true,
            Type::Closure { .. } | Type::Callable(_) => false,
            Type::Named(name, args) if name == "Task" && args.len() == 1 => args[0].is_copy(),
            Type::Named(name, args) => is_builtin_copy_named_type(name, args),
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Union(union) => {
                for (index, member) in union.members.iter().enumerate() {
                    if index != 0 {
                        write!(f, " | ")?;
                    }
                    write!(f, "{member}")?;
                }
                Ok(())
            }
            Type::Unit => write!(f, "None"),
            Type::Module(name) => write!(f, "module {}", name),
            Type::ReturnedView(view) => write!(
                f,
                "view {}{} from #{}",
                if view.mutable { "mut " } else { "" },
                view.pointee,
                view.origin
            ),
            Type::TypeParam(name) => write!(f, "{}", name),
            Type::Tuple(elements) => {
                write!(f, "(")?;
                for (index, element) in elements.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", element)?;
                }
                if elements.len() == 1 {
                    write!(f, ",")?;
                }
                write!(f, ")")
            }
            Type::Function {
                params,
                return_type,
            } => {
                write!(f, "def(")?;
                write_contract_params(f, params)?;
                write_return_contract(f, params, return_type)
            }
            Type::Callable(callable) => {
                write!(
                    f,
                    "{}[{}def(",
                    callable.constructor_name(),
                    callable.call_kind.spelling()
                )?;
                write_contract_params(f, &callable.params)?;
                write_return_contract(f, &callable.params, &callable.return_type)?;
                write!(f, "]")
            }
            Type::Closure {
                params,
                return_type,
                call_kind,
                ..
            } => {
                match call_kind {
                    ClosureCallKind::Consuming => write!(f, "consuming ")?,
                    ClosureCallKind::MutableRepeatable => write!(f, "mutable ")?,
                    ClosureCallKind::Repeatable => {}
                }
                write!(f, "closure def(")?;
                write_contract_params(f, params)?;
                write_return_contract(f, params, return_type)
            }
            Type::Named(name, args) if args.is_empty() => write!(f, "{}", name),
            Type::Named(name, args) => {
                write!(f, "{}[", name)?;
                for (index, arg) in args.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, "]")
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FunctionParamContract {
    pub keyword_only: bool,
    /// Empty for an explicit positional-only slot of a written
    /// `def(T) -> R` type; a named slot or an inferred value carries the
    /// exposed name. Names, the keyword-only boundary, and default
    /// availability are part of the complete callable contract (Q17 A,
    /// Q19 A); ABI equality of `Type` ignores them.
    pub name: String,
    pub ty: Type,
    pub passing: ReceiverKind,
    /// True when the contract promises a default for this slot (`= ...` in
    /// a written type, or a declaration default). Only the selected target
    /// supplies the default expression.
    pub has_default: bool,
}

/// The callable body that lexically owns a lambda expression.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum ClosureOwner {
    Function(String),
    ClassMethod {
        class_name: String,
        method_name: String,
    },
    TraitMethod {
        trait_name: String,
        method_name: String,
    },
    TraitImplMethod {
        trait_name: String,
        for_type: String,
        method_name: String,
    },
    TopLevel,
}

/// Stable semantic identity for one lambda expression.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ClosureId {
    pub module_name: String,
    pub owner: ClosureOwner,
    pub line: usize,
    pub column: usize,
}

impl ClosureId {
    pub fn new(module_name: &str, owner: ClosureOwner, span: crate::diag::Span) -> Self {
        Self {
            module_name: module_name.to_string(),
            owner,
            line: span.line,
            column: span.column,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ClosureCaptureMode {
    Copy,
    Move,
    SharedView,
    MutableView,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ClosureCallKind {
    Repeatable,
    MutableRepeatable,
    Consuming,
}

impl ClosureCallKind {
    /// Shared < Mutable < Consuming: a value may be packed into a callable
    /// type of the same or a weaker kind, never a stronger one (C2).
    pub fn rank(self) -> u8 {
        match self {
            Self::Repeatable => 0,
            Self::MutableRepeatable => 1,
            Self::Consuming => 2,
        }
    }

    pub fn admits(self, source: Self) -> bool {
        source.rank() <= self.rank()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Repeatable => "Shared",
            Self::MutableRepeatable => "Mutable",
            Self::Consuming => "Consuming",
        }
    }

    pub fn spelling(self) -> &'static str {
        match self {
            Self::Repeatable => "",
            Self::MutableRepeatable => "mut ",
            Self::Consuming => "own ",
        }
    }
}

/// An owned, environment-erased callable storage type (C1, Q13 A): the
/// complete call contract plus the call kind, with the capture set erased.
/// Non-Copy and non-cloneable; Transfer only as a checked `TaskCallable`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CallableType {
    pub task: bool,
    pub call_kind: ClosureCallKind,
    pub params: Vec<FunctionParamContract>,
    pub return_type: Type,
}

impl CallableType {
    /// The thin contract every packed value must meet.
    pub fn contract(&self) -> Type {
        Type::Function {
            params: self.params.clone(),
            return_type: Box::new(self.return_type.clone()),
        }
    }

    pub fn constructor_name(&self) -> &'static str {
        if self.task {
            "TaskCallable"
        } else {
            "Callable"
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ClosureCapture {
    pub name: String,
    pub ty: Type,
    pub mode: ClosureCaptureMode,
    pub span: crate::diag::Span,
    /// An owned (`Copy`/`Move`) capture the body mutates in place (C2):
    /// the closure is Mutable and the environment keeps the updated value.
    #[serde(default)]
    pub mutated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ClosureInfo {
    pub id: ClosureId,
    pub span: crate::diag::Span,
    pub params: Vec<FunctionParamContract>,
    pub return_type: Type,
    /// Explicit lists preserve source order; implicit captures use lexical
    /// first-use order.
    pub captures: Vec<ClosureCapture>,
    pub call_kind: ClosureCallKind,
}

impl ClosureInfo {
    pub fn ty(&self) -> Type {
        if self.captures.is_empty() {
            Type::Function {
                params: self.params.clone(),
                return_type: Box::new(self.return_type.clone()),
            }
        } else {
            Type::Closure {
                params: Box::new(self.params.clone()),
                return_type: Box::new(self.return_type.clone()),
                captures: Box::new(self.captures.clone()),
                call_kind: self.call_kind,
            }
        }
    }
}

/// Whether `name` is a builtin type name rather than a user declaration.
pub fn is_builtin_type(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "int"
            | "int8"
            | "int16"
            | "int32"
            | "int64"
            | "int128"
            | "intsize"
            | "uint8"
            | "uint16"
            | "uint32"
            | "uint64"
            | "uint128"
            | "uintsize"
            | "float32"
            | "float64"
            | "str"
            | "Array"
            | "list"
            | "set"
            | "dict"
            | "Range"
            | "Queue"
            | "Task"
            | "Lookup"
            | "Poll"
            | "Result"
            | "SendError"
            | "QueueReceive"
            | "TaskResult"
            | "WaitAny"
            | "WaitAll"
            | "SelectOutcome"
            | "TaskGroup"
            | "Duration"
            | "Span"
    )
}

/// Whether a builtin named type is `Copy` with these arguments.
pub fn is_builtin_copy_named_type(name: &str, args: &[Type]) -> bool {
    match name {
        "Queue" => args.len() == 1,
        _ => {
            args.is_empty()
                && matches!(
                    name,
                    "bool"
                        | "int8"
                        | "int16"
                        | "int32"
                        | "int64"
                        | "int128"
                        | "intsize"
                        | "uint8"
                        | "uint16"
                        | "uint32"
                        | "uint64"
                        | "uint128"
                        | "uintsize"
                        | "float32"
                        | "float64"
                        | "Duration"
                )
        }
    }
}
