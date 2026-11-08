//! Symbolic execution of one MIR function into SMT-LIB obligations
//! (checkpoint C4).
//!
//! The executor walks the control-flow graph in reverse postorder and
//! keeps, for every reachable block, a path condition and a symbolic value
//! for each local. Where paths join it merges them with fresh constants
//! defined by `ite`, so no term grows with the number of paths. A loop is
//! cut at its header: the executor proves the loop's `invariant` and
//! `decreases` checks on entry, replaces every local the loop writes with a
//! fresh value, assumes those checks for the iteration it then runs, and
//! proves them again for the state each back edge carries.
//!
//! Obligations come from the checks MIR names: the trap of every numeric
//! `Binary`, `Unary`, and `Cast`, every checked list element access, every
//! `AssertFail` (asserts and contract clauses), and the `requires` of each
//! call. A failed `requires` check of the function itself is assumed not
//! to happen: its callers prove it. Every value the encoding does not model
//! is a fresh, unconstrained constant, so an unmodeled step can only cost a
//! proof, never fake one.

use std::collections::{BTreeMap, BTreeSet};

use super::{ObligationKind, VerifyContext};
use crate::ast::{BinaryOp, UnaryOp};
use crate::diag::Span;
use crate::mir::{
    ArithmeticTrap, AssertKind, BasicBlock, CallTarget, Instruction, MethodDispatch, MirArg,
    MirFunction, MirReceiverKind, Operand, Rvalue, Terminator,
};
use crate::runtime_ops::{
    DictOp, FunctionOp, ListOp, RuntimeOp, ScalarOp, SetOp, StrOp, TaskOp, ValueOp,
};
use crate::sema::Type;

/// The inclusive range of an integer type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct IntTy {
    pub(super) lo: i128,
    pub(super) hi: i128,
}

impl IntTy {
    pub(super) const INT64: IntTy = IntTy {
        lo: i64::MIN as i128,
        hi: i64::MAX as i128,
    };

    pub(super) fn of(ty: &Type) -> Option<Self> {
        let Type::Named(name, args) = ty else {
            return None;
        };
        if !args.is_empty() {
            return None;
        }
        let (lo, hi): (i128, i128) = match name.as_str() {
            "int8" => (i8::MIN as i128, i8::MAX as i128),
            "int16" => (i16::MIN as i128, i16::MAX as i128),
            "int32" => (i32::MIN as i128, i32::MAX as i128),
            "int64" => (i64::MIN as i128, i64::MAX as i128),
            "uint8" => (0, u8::MAX as i128),
            "uint16" => (0, u16::MAX as i128),
            "uint32" => (0, u32::MAX as i128),
            "uint64" => (0, u64::MAX as i128),
            _ => return None,
        };
        Some(Self { lo, hi })
    }

    pub(super) fn contains(self, term: &str) -> String {
        format!(
            "(and (<= {} {term}) (<= {term} {}))",
            lit(self.lo),
            lit(self.hi)
        )
    }
}

/// An SMT integer literal.
pub(super) fn lit(value: i128) -> String {
    if value < 0 {
        format!("(- {})", value.unsigned_abs())
    } else {
        value.to_string()
    }
}

/// The SMT sort of a list's element store, when its elements are scalars
/// the encoding models.
fn element_sort(element: &Type) -> Option<&'static str> {
    if let Some(float) = FloatTy::of(element) {
        Some(float.sort())
    } else if IntTy::of(element).is_some() {
        Some("Int")
    } else if is_bool(element) {
        Some("Bool")
    } else if is_str(element) {
        Some("String")
    } else {
        None
    }
}

/// The SMT sort of a `dict` key or `set` element the encoding models: an
/// integer or a `str`.
pub(super) fn key_sort(key: &Type) -> Option<&'static str> {
    if IntTy::of(key).is_some() {
        Some("Int")
    } else if is_str(key) {
        Some("String")
    } else {
        None
    }
}

pub(super) fn is_str(ty: &Type) -> bool {
    matches!(ty, Type::Named(name, args) if name == "str" && args.is_empty())
}

/// `text` as an SMT-LIB string literal, or `None` when it holds a
/// character past what SMT-LIB strings hold (`U+2FFFF`).
pub(super) fn smt_string(text: &str) -> Option<String> {
    let mut literal = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => literal.push_str("\"\""),
            ' '..='~' if character != '\\' => literal.push(character),
            _ if u32::from(character) <= 0x2FFFF => {
                literal.push_str(&format!("\\u{{{:x}}}", u32::from(character)));
            }
            _ => return None,
        }
    }
    literal.push('"');
    Some(literal)
}

/// An IEEE-754 binary floating-point type, as an SMT `FloatingPoint` sort.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FloatTy {
    F32,
    F64,
}

impl FloatTy {
    pub(super) fn of(ty: &Type) -> Option<Self> {
        match ty {
            Type::Named(name, args) if args.is_empty() && name == "float32" => Some(Self::F32),
            Type::Named(name, args) if args.is_empty() && name == "float64" => Some(Self::F64),
            _ => None,
        }
    }

    fn of_sort(sort: &str) -> Option<Self> {
        match sort {
            "Float32" => Some(Self::F32),
            "Float64" => Some(Self::F64),
            _ => None,
        }
    }

    pub(super) fn sort(self) -> &'static str {
        match self {
            Self::F32 => "Float32",
            Self::F64 => "Float64",
        }
    }

    /// The `(_ to_fp e s)` indexes of the sort.
    fn indexes(self) -> &'static str {
        match self {
            Self::F32 => "8 24",
            Self::F64 => "11 53",
        }
    }

    /// The exact SMT literal of a `float64` value.
    pub(super) fn literal(value: f64) -> String {
        let bits = value.to_bits();
        format!(
            "(fp #b{} #b{:011b} #b{:052b})",
            bits >> 63,
            (bits >> 52) & 0x7ff,
            bits & ((1 << 52) - 1)
        )
    }

    /// `term`, a float of type `from`, rounded to this type.
    pub(super) fn convert(self, term: &str, from: FloatTy) -> String {
        if self == from {
            term.to_string()
        } else {
            format!("((_ to_fp {}) RNE {term})", self.indexes())
        }
    }

    /// The largest integer below which every integer has an exact value
    /// of this type.
    fn exact_ints(self) -> u64 {
        match self {
            Self::F32 => 1 << 24,
            Self::F64 => 1 << 53,
        }
    }

    /// The SMT literal of the value of this type nearest `value`, and how
    /// it compares with `value`.
    fn nearest(self, value: i128) -> (String, std::cmp::Ordering) {
        match self {
            Self::F64 => {
                let near = value as f64;
                (Self::literal(near), (near as i128).cmp(&value))
            }
            Self::F32 => {
                let near = value as f32;
                let bits = near.to_bits();
                (
                    format!(
                        "(fp #b{} #b{:08b} #b{:023b})",
                        bits >> 31,
                        (bits >> 23) & 0xff,
                        bits & ((1 << 23) - 1)
                    ),
                    (near as i128).cmp(&value),
                )
            }
        }
    }

    /// `term > value`, for an integer `value` this type may not hold.
    fn above(self, term: &str, value: i128) -> String {
        match self.nearest(value) {
            // No value of the type lies between `value` and a nearest one
            // above it.
            (near, std::cmp::Ordering::Greater) => format!("(fp.geq {term} {near})"),
            (near, _) => format!("(fp.gt {term} {near})"),
        }
    }

    /// `term < value`, for an integer `value` this type may not hold.
    fn below(self, term: &str, value: i128) -> String {
        match self.nearest(value) {
            (near, std::cmp::Ordering::Less) => format!("(fp.leq {term} {near})"),
            (near, _) => format!("(fp.lt {term} {near})"),
        }
    }

    /// The integer `term` rounded to this type.
    pub(super) fn round_int(self, term: &str) -> String {
        format!("((_ to_fp {}) RNE (to_real {term}))", self.indexes())
    }
}

/// `value` as a value of `ty`: a `float64` literal written where a
/// `float32` is expected rounds to it.
pub(super) fn fit(value: Val, ty: &Type) -> Val {
    match (value, FloatTy::of(ty)) {
        (Val::Float(term, from), Some(to)) if from != to => Val::Float(to.convert(&term, from), to),
        (value, _) => value,
    }
}

/// The value a term of `ty`'s sort stands for.
pub(super) fn scalar_val(term: String, ty: &Type) -> Val {
    if let Some(float) = FloatTy::of(ty) {
        Val::Float(term, float)
    } else if let Some(int) = IntTy::of(ty) {
        Val::Int(term, int)
    } else if is_str(ty) {
        Val::Str(term)
    } else {
        Val::Bool(term)
    }
}

/// An empty `(Array K Bool)`: no key is held.
pub(super) fn no_keys(sort: &str) -> String {
    format!("((as const (Array {sort} Bool)) false)")
}

fn is_bool(ty: &Type) -> bool {
    matches!(ty, Type::Named(name, args) if name == "bool" && args.is_empty())
}

/// A symbolic value.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Val {
    Int(String, IntTy),
    Bool(String),
    /// A `str`, as an SMT string: a sequence of Unicode scalar values, as
    /// `len` counts them.
    Str(String),
    /// A float, as an IEEE-754 SMT `FloatingPoint` term.
    Float(String, FloatTy),
    /// A function value that names one function: a call through it is a
    /// call of that function.
    Func(String),
    /// A `Task[T]` handle, with the value its task returns when it
    /// finishes normally.
    Task(Box<Val>),
    /// A function value of a callable alias with a contract, named by the
    /// alias: a call through it has that contract.
    Contracted(String),
    /// A `dict`: its length, which keys it holds as an `(Array K Bool)`,
    /// and its values as an `(Array K S)` when the encoding models them.
    Map {
        len: String,
        keys: String,
        values: Option<String>,
        key: Type,
        value: Type,
    },
    /// A `set`: its length, and which elements it holds as an
    /// `(Array K Bool)`.
    Set {
        len: String,
        members: String,
        elem: Type,
    },
    /// A list: its length, and the store of its elements when the
    /// encoding models them.
    List {
        len: String,
        data: Option<Store>,
        elem: Type,
    },
    Tuple(Vec<Val>),
    Record(String, Vec<(String, Val)>),
    /// An enum value: its type name, its variant index, and the payloads
    /// each variant would hold.
    Enum(String, String, Vec<Vec<Val>>),
    /// A union value: its member index, and the value each member would
    /// hold.
    Union(String, Vec<Val>),
    Unit,
    /// A value the encoding does not model.
    Opaque,
}

/// A list's element store: an `(Array Int S)` for each scalar an element
/// holds, nested as the element's tuple items or class fields are.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Store {
    Leaf(String, &'static str),
    Items(Vec<Store>),
}

/// A loan's target, as far as the encoding follows it.
#[derive(Clone, Debug, PartialEq)]
enum Loan {
    Place(String),
    Element {
        list: String,
        index: String,
        projection: String,
    },
    /// A loan the encoding does not follow. A write through it makes the
    /// root local unknown.
    Unknown(String),
}

#[derive(Clone, Debug)]
pub(super) struct State {
    pub(super) pc: String,
    pub(super) env: BTreeMap<String, Val>,
    loans: BTreeMap<String, Loan>,
}

/// What an obligation asks the solver to show.
#[derive(Clone, Debug)]
pub(super) enum Goal {
    /// The term holds whenever the path condition does.
    Holds(String),
    /// The step is outside the subset the encoding models.
    Unsupported(String),
}

#[derive(Clone, Debug)]
pub(super) struct Obligation {
    pub(super) kind: ObligationKind,
    pub(super) span: Span,
    pub(super) message: String,
    pub(super) pc: String,
    pub(super) goal: Goal,
    /// How many facts were known when the check was reached. A fact
    /// assumed later, such as a callee's `ensures`, holds only after it.
    pub(super) facts: usize,
    /// For the stricter check an index proof for a build needs, the
    /// element access it covers: its block and position. Such checks are
    /// not reported.
    pub(super) site: Option<(usize, usize)>,
    /// A check no run performs: one in a `lemma def`, or the `requires` of
    /// an erased lemma call. Until it is proved, what follows from it is
    /// not known to hold when the program runs.
    pub(super) erased: bool,
}

/// The loops of a function's control-flow graph.
struct Loop {
    header: usize,
    body: BTreeSet<usize>,
    /// The `invariant` and `decreases` failure blocks this loop checks
    /// itself, outside any nested loop.
    check_fails: BTreeSet<usize>,
    /// The blocks a check of this loop can run in after the header.
    check_region: BTreeSet<usize>,
    /// Every root local the loop may write.
    writes: BTreeSet<String>,
    /// The locals every write of the loop steps by a constant in one
    /// direction.
    counters: BTreeMap<String, Step>,
    /// Counters that step by one only right after a test that they are
    /// below a bound the loop does not write, by counter: they never pass
    /// the larger of their entry value and the bound.
    bounds: BTreeMap<String, Operand>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Up,
    Down,
}

struct Cfg {
    rpo: Vec<usize>,
    loops: BTreeMap<usize, Loop>,
}

/// The executor of one function. Its SMT text is a list of declarations
/// and facts, and the obligations the solver checks against them.
pub(super) struct Exec<'a> {
    pub(super) cx: &'a VerifyContext<'a>,
    pub(super) function: &'a MirFunction,
    pub(super) decls: Vec<String>,
    pub(super) facts: Vec<String>,
    pub(super) obligations: Vec<Obligation>,
    /// The entry value of each scalar parameter, for counterexamples.
    pub(super) inputs: Vec<(String, String)>,
    counter: usize,
    local_types: BTreeMap<String, Type>,
    /// The source position of the statement being executed.
    pub(super) span: Span,
    /// Check failure blocks whose paths the current iteration assumes away.
    pub(super) assumed_fails: BTreeSet<usize>,
    /// Set while the `requires` of a `lemma def` call are proved.
    pub(super) erasing: bool,
    /// Set while an operand is read where a function value stays one the
    /// verifier follows: copied to a place, or called.
    func_ok: bool,
    /// Set while a block of `ghost` code runs.
    in_ghost: bool,
    /// Set, for a build, while the integer operation at the current
    /// position is checked: each of its checks is also a site.
    trap_site: bool,
    /// The declaration whose contract the call being made uses instead of
    /// its callee's own: a trait method's, for a call through the trait.
    pub(super) contract_override: Option<&'a crate::ast::FunctionDecl>,
    labels: BTreeMap<&'a str, usize>,
    /// Set when the function is outside what the executor can walk.
    pub(super) unsupported: Option<String>,
    /// Locals a closure of this function captures by reference. Any call
    /// may run the closure and change them.
    captured: BTreeSet<String>,
    /// Whether to add the stricter check of each element access a build
    /// can leave unchecked.
    pub(super) prove_sites: bool,
    /// The block and position of the instruction being executed.
    position: (usize, usize),
}

const MAX_DEPTH: usize = 3;

impl<'a> Exec<'a> {
    pub(super) fn new(cx: &'a VerifyContext<'a>, function: &'a MirFunction) -> Self {
        let mut local_types = BTreeMap::new();
        for local in &function.local_types {
            local_types.insert(local.name.clone(), local.ty.clone());
        }
        for param in &function.params {
            local_types.insert(param.name.clone(), param.ty.clone());
        }
        let labels = function
            .blocks
            .iter()
            .enumerate()
            .map(|(index, block)| (block.label.as_str(), index))
            .collect();
        Self {
            cx,
            function,
            decls: Vec::new(),
            facts: Vec::new(),
            obligations: Vec::new(),
            inputs: Vec::new(),
            counter: 0,
            local_types,
            span: function.span,
            assumed_fails: BTreeSet::new(),
            erasing: false,
            func_ok: false,
            in_ghost: false,
            trap_site: false,
            contract_override: None,
            labels,
            unsupported: None,
            captured: closure_captures(function),
            prove_sites: false,
            position: (0, 0),
        }
    }

    // ----- SMT text --------------------------------------------------------

    /// A fresh name for a variable a quantifier binds; it is not declared.
    pub(super) fn bound_name(&mut self) -> String {
        self.counter += 1;
        format!("k!q{}", self.counter)
    }

    pub(super) fn fresh_const(&mut self, sort: &str, hint: &str) -> String {
        self.counter += 1;
        let clean: String = hint
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
            .take(24)
            .collect();
        let name = format!("{clean}!{}", self.counter);
        self.decls.push(format!("(declare-const {name} {sort})"));
        name
    }

    /// A fresh constant equal to `term`, to keep terms small.
    pub(super) fn define(&mut self, sort: &str, hint: &str, term: String) -> String {
        if is_atom(&term) {
            return term;
        }
        let name = self.fresh_const(sort, hint);
        self.facts.push(format!("(= {name} {term})"));
        name
    }

    pub(super) fn fact(&mut self, term: String) {
        if term != "true" {
            self.facts.push(term);
        }
    }

    /// Assumes `term` on the paths `pc` describes.
    pub(super) fn assume(&mut self, pc: &str, term: String) {
        if pc == "true" {
            self.fact(term);
        } else {
            self.fact(format!("(=> {pc} {term})"));
        }
    }

    /// Copies the obligation just made as one for the build site `site`.
    pub(super) fn copy_as_site(&mut self, site: (usize, usize)) {
        if let Some(last) = self.obligations.last() {
            let mut copy = last.clone();
            copy.site = Some(site);
            self.obligations.push(copy);
        }
    }

    pub(super) fn oblige(
        &mut self,
        kind: ObligationKind,
        span: Span,
        message: impl Into<String>,
        pc: &str,
        goal: Goal,
    ) {
        self.obligations.push(Obligation {
            kind,
            span,
            message: message.into(),
            pc: pc.to_string(),
            goal,
            facts: self.facts.len(),
            site: None,
            erased: self.erasing
                || self.in_ghost
                || self
                    .cx
                    .decl(&self.function.name)
                    .is_some_and(|decl| decl.lemma),
        });
        if self.trap_site {
            self.copy_as_site(self.position);
        }
    }

    // ----- Types and fresh values ------------------------------------------

    /// A float divisor is not zero of either sign: a float division by
    /// zero stops the program.
    fn oblige_float_divisor(&mut self, pc: &str, span: Span, divisor: &str, trap: ArithmeticTrap) {
        if trap.checks_divisor() || trap == ArithmeticTrap::Runtime {
            self.oblige(
                ObligationKind::DivisionByZero,
                span,
                "divisor is not zero",
                pc,
                Goal::Holds(format!("(not (fp.isZero {divisor}))")),
            );
        }
    }

    /// A fresh length and key set of a `dict` or `set` whose keys have
    /// `sort`: an empty one holds no key.
    fn fresh_keys(&mut self, sort: &str, hint: &str) -> (String, String) {
        let len = self.fresh_const("Int", &format!("{hint}_len"));
        self.fact(format!("(and (<= 0 {len}) (<= {len} {}))", i64::MAX));
        let keys = self.fresh_const(&format!("(Array {sort} Bool)"), &format!("{hint}_keys"));
        self.fact(format!("(=> (= {len} 0) (= {keys} {}))", no_keys(sort)));
        (len, keys)
    }

    /// The length of a `dict` or `set` of `len` keys once `key` is added
    /// to `keys`.
    fn len_after_add(&mut self, len: &str, keys: &str, key: &str) -> String {
        let after = self.define(
            "Int",
            "len",
            format!("(ite (select {keys} {key}) {len} (+ {len} 1))"),
        );
        // One more key than `int64` can count stops the program first.
        self.fact(format!("(<= {after} {})", i64::MAX));
        after
    }

    pub(super) fn fresh(&mut self, ty: &Type, hint: &str) -> Val {
        self.fresh_at(ty, hint, 0)
    }

    fn fresh_at(&mut self, ty: &Type, hint: &str, depth: usize) -> Val {
        if let Some(int) = IntTy::of(ty) {
            let name = self.fresh_const("Int", hint);
            self.fact(int.contains(&name));
            return Val::Int(name, int);
        }
        if is_bool(ty) {
            return Val::Bool(self.fresh_const("Bool", hint));
        }
        if is_str(ty) {
            return Val::Str(self.fresh_const("String", hint));
        }
        if let Some(float) = FloatTy::of(ty) {
            return Val::Float(self.fresh_const(float.sort(), hint), float);
        }
        if depth > MAX_DEPTH {
            return Val::Opaque;
        }
        match ty {
            Type::Unit => Val::Unit,
            Type::Named(name, args) if name == "dict" && args.len() == 2 => {
                let Some(sort) = key_sort(&args[0]) else {
                    return Val::Opaque;
                };
                let (len, keys) = self.fresh_keys(sort, hint);
                let values = element_sort(&args[1])
                    .map(|value| self.fresh_const(&format!("(Array {sort} {value})"), hint));
                Val::Map {
                    len,
                    keys,
                    values,
                    key: args[0].clone(),
                    value: args[1].clone(),
                }
            }
            Type::Named(name, args) if name == "set" && args.len() == 1 => {
                let Some(sort) = key_sort(&args[0]) else {
                    return Val::Opaque;
                };
                let (len, members) = self.fresh_keys(sort, hint);
                Val::Set {
                    len,
                    members,
                    elem: args[0].clone(),
                }
            }
            Type::Named(name, args) if name == "list" && args.len() == 1 => {
                let len = self.fresh_const("Int", &format!("{hint}_len"));
                self.fact(format!("(and (<= 0 {len}) (<= {len} {}))", i64::MAX));
                let data = self.fresh_store(&args[0], hint);
                Val::List {
                    len,
                    data,
                    elem: args[0].clone(),
                }
            }
            Type::Tuple(items) => Val::Tuple(
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| self.fresh_at(item, &format!("{hint}_{index}"), depth + 1))
                    .collect(),
            ),
            Type::Union(union) => {
                let tag = self.fresh_const("Int", &format!("{hint}_tag"));
                self.fact(format!(
                    "(and (<= 0 {tag}) (< {tag} {}))",
                    union.members.len()
                ));
                let members = union
                    .members
                    .iter()
                    .enumerate()
                    .map(|(index, member)| {
                        self.fresh_at(member, &format!("{hint}_m{index}"), depth + 1)
                    })
                    .collect();
                Val::Union(tag, members)
            }
            Type::Named(name, args) => {
                if let Some(fields) = self.cx.class_fields(name, args) {
                    let fields = fields
                        .into_iter()
                        .map(|(field, ty)| {
                            let value = self.fresh_at(&ty, &format!("{hint}_{field}"), depth + 1);
                            (field, value)
                        })
                        .collect();
                    return Val::Record(name.clone(), fields);
                }
                if let Some(variants) = self.cx.enum_variants(name, args) {
                    let tag = self.fresh_const("Int", &format!("{hint}_tag"));
                    self.fact(format!("(and (<= 0 {tag}) (< {tag} {}))", variants.len()));
                    let payloads = variants
                        .into_iter()
                        .map(|(variant, payloads)| {
                            payloads
                                .iter()
                                .enumerate()
                                .map(|(index, ty)| {
                                    self.fresh_at(
                                        ty,
                                        &format!("{hint}_{variant}{index}"),
                                        depth + 1,
                                    )
                                })
                                .collect()
                        })
                        .collect();
                    return Val::Enum(name.clone(), tag, payloads);
                }
                Val::Opaque
            }
            _ => Val::Opaque,
        }
    }

    /// The type of a place path such as `self.balance` or `pair.0`.
    pub(super) fn place_type(&self, path: &str) -> Option<Type> {
        let mut segments = path.split('.');
        let mut ty = self.local_types.get(segments.next()?)?.clone();
        for segment in segments {
            ty = self.cx.project_type(&ty, segment)?;
        }
        Some(ty)
    }

    // ----- Places ----------------------------------------------------------

    pub(super) fn read_place(&mut self, state: &mut State, path: &str) -> Val {
        let mut segments = path.split('.');
        let root = segments.next().unwrap_or(path).to_string();
        let rest: Vec<&str> = segments.collect();
        if !state.env.contains_key(&root) {
            let value = self.fresh_local(&root);
            state.env.insert(root.clone(), value);
        }
        if rest.is_empty() {
            return state.env[&root].clone();
        }
        let mut prefix = root.clone();
        let mut current = state.env[&root].clone();
        for segment in &rest {
            if current == Val::Opaque {
                if let Some(ty) = self.place_type(&prefix) {
                    current = self.fresh(&ty, &prefix);
                    self.write_place(state, &prefix, current.clone());
                }
            }
            prefix = format!("{prefix}.{segment}");
            current = match project(&current, segment) {
                Some(value) => value,
                None => self.fresh_local(&prefix),
            };
        }
        current
    }

    fn write_place(&mut self, state: &mut State, path: &str, value: Val) {
        let value = match (&value, self.place_type(path)) {
            (Val::Float(..), Some(ty)) => fit(value, &ty),
            _ => value,
        };
        let mut segments = path.split('.');
        let root = segments.next().unwrap_or(path).to_string();
        let rest: Vec<&str> = segments.collect();
        if rest.is_empty() {
            state.env.insert(root, value);
            return;
        }
        let mut root_value = match state.env.get(&root) {
            Some(value) if *value != Val::Opaque => value.clone(),
            _ => self.fresh_local(&root),
        };
        if set_in(&mut root_value, &rest, value) {
            state.env.insert(root, root_value);
        } else {
            self.havoc_root(state, &root);
        }
    }

    fn havoc_root(&mut self, state: &mut State, root: &str) {
        let value = self.fresh_local(root);
        state.env.insert(root.to_string(), value);
    }

    fn havoc_place(&mut self, state: &mut State, path: &str) {
        let value = self.fresh_local(path);
        self.write_place(state, path, value);
    }

    // ----- Operands --------------------------------------------------------

    fn operand(&mut self, state: &mut State, operand: &Operand, hint: Option<IntTy>) -> Val {
        let value = self.operand_value(state, operand, hint);
        // A function value that goes where the verifier cannot follow it,
        // such as into a call or a collection, may be called with
        // arguments its `requires` does not allow.
        if !self.func_ok {
            let pc = state.pc.clone();
            self.oblige_escape(&value, &pc);
        }
        value
    }

    /// Reports what a function value needs where it goes somewhere the
    /// verifier stops following it.
    fn oblige_escape(&mut self, value: &Val, pc: &str) {
        match value {
            Val::Func(name) => self.oblige_unfollowed(name, pc),
            Val::Contracted(alias) => {
                let span = self.span;
                self.oblige(
                    ObligationKind::Requires,
                    span,
                    format!("the contract of `{alias}` holds where it is called through a value"),
                    pc,
                    Goal::Unsupported(
                        "a function value passed where its contract is not known is not followed"
                            .into(),
                    ),
                );
            }
            _ => {}
        }
    }

    /// What a function value passed as `param` of `callee` needs: one
    /// passed as a callable alias with a contract meets it, and one passed
    /// as anything else is no longer followed.
    fn pass_function_value(&mut self, state: &State, callee: &str, param: &str, value: &Val) {
        let contracted = self
            .cx
            .decl(callee)
            .and_then(|decl| decl.params.iter().find(|p| p.name == param))
            .and_then(|p| self.cx.callable_contract(&p.ty));
        let pc = state.pc.clone();
        match (value, contracted) {
            (Val::Contracted(from), Some((alias, _))) if *from == alias => {}
            (Val::Func(function), Some((alias, _))) if self.cx.decl(function).is_some() => {
                self.prove_callable_refinement(state, function, &alias);
            }
            (_, Some((alias, _))) => {
                let span = self.span;
                self.oblige(
                    ObligationKind::Requires,
                    span,
                    format!("the value passed as `{param}` meets the contract of `{alias}`"),
                    &pc,
                    Goal::Unsupported(
                        "only a named function, or a value of the same alias, is checked against a callable contract"
                            .into(),
                    ),
                );
            }
            (value, None) => self.oblige_escape(value, &pc),
        }
    }

    /// Reports that the `requires` of `name`, if it has one, is checked
    /// where its function value is called, since the verifier no longer
    /// knows which value that is.
    fn oblige_unfollowed(&mut self, name: &str, pc: &str) {
        let has_requires = self.cx.decl(name).is_some_and(|decl| {
            decl.contracts
                .iter()
                .any(|clause| clause.kind == crate::ast::ContractKind::Requires)
        });
        if has_requires {
            let span = self.span;
            self.oblige(
                ObligationKind::Requires,
                span,
                format!(
                    "`requires` of `{}` holds where it is called through a value",
                    crate::mir::public_function_name(name)
                ),
                pc,
                Goal::Unsupported(
                    "a call through a function value checks `requires` when it runs".into(),
                ),
            );
        }
    }

    /// `operand`, read where a function value stays one the verifier
    /// follows.
    fn followed_operand(
        &mut self,
        state: &mut State,
        operand: &Operand,
        hint: Option<IntTy>,
    ) -> Val {
        self.func_ok = true;
        let value = self.operand(state, operand, hint);
        self.func_ok = false;
        value
    }

    fn operand_value(&mut self, state: &mut State, operand: &Operand, hint: Option<IntTy>) -> Val {
        match operand {
            Operand::Place(place) | Operand::MovePlace(place) => self.read_place(state, place),
            Operand::Int(value) => Val::Int(value.to_string(), hint.unwrap_or(IntTy::INT64)),
            Operand::Bool(value) => Val::Bool(value.to_string()),
            Operand::String(text) => smt_string(text).map_or(Val::Opaque, Val::Str),
            Operand::Float(value) => Val::Float(FloatTy::literal(*value), FloatTy::F64),
            Operand::Unit => Val::Unit,
            Operand::Function { name, .. } => Val::Func(name.clone()),
            _ => Val::Opaque,
        }
    }

    fn operand_type(&self, operand: &Operand) -> Option<Type> {
        match operand {
            Operand::Place(place) | Operand::MovePlace(place) => self.place_type(place),
            Operand::Int(_) => Some(Type::named("int64")),
            Operand::Float(_) => Some(Type::named("float64")),
            Operand::String(_) => Some(Type::named("str")),
            _ => None,
        }
    }

    fn operand_int_ty(&self, operand: &Operand) -> Option<IntTy> {
        match operand {
            Operand::Place(place) | Operand::MovePlace(place) => {
                self.place_type(place).as_ref().and_then(IntTy::of)
            }
            _ => None,
        }
    }

    // ----- Driver ----------------------------------------------------------

    /// Executes the whole function from its entry.
    pub(super) fn run(&mut self, entry_state: State) {
        let cfg = match self.cfg() {
            Ok(cfg) => cfg,
            Err(reason) => {
                self.unsupported = Some(reason);
                return;
            }
        };
        let entry = self.labels[self.function.entry.as_str()];
        let all: BTreeSet<usize> = (0..self.function.blocks.len()).collect();
        self.region(&cfg, &all, entry, entry_state, None);
    }

    /// The state on entry: each parameter a fresh value of its type.
    pub(super) fn entry_state(&mut self) -> State {
        let mut state = State {
            pc: "true".to_string(),
            env: BTreeMap::new(),
            loans: BTreeMap::new(),
        };
        if self.function.receiver.is_some() {
            if let Some(ty) = self.local_types.get("self").cloned() {
                let value = self.fresh(&ty, "self");
                self.record_inputs("self", &value);
                state.env.insert("self".to_string(), value);
            }
        }
        let declared = self.cx.decl(&self.function.name);
        for param in &self.function.params {
            // A parameter of a callable alias with a contract has it.
            let contracted = declared
                .and_then(|decl| decl.params.iter().find(|p| p.name == param.name))
                .and_then(|p| self.cx.callable_contract(&p.ty));
            let value = match contracted {
                Some((alias, _)) => Val::Contracted(alias),
                None => self.fresh(&param.ty, &param.name),
            };
            self.record_inputs(&param.name, &value);
            state.env.insert(param.name.clone(), value);
        }
        state
    }

    fn record_inputs(&mut self, label: &str, value: &Val) {
        match value {
            Val::Int(term, _) | Val::Bool(term) | Val::Str(term) | Val::Float(term, _) => {
                self.inputs.push((label.to_string(), term.clone()))
            }
            Val::List { len, .. } | Val::Map { len, .. } | Val::Set { len, .. } => {
                self.inputs.push((format!("len({label})"), len.clone()))
            }
            Val::Record(_, fields) => {
                for (field, value) in fields {
                    self.record_inputs(&format!("{label}.{field}"), value);
                }
            }
            Val::Tuple(items) => {
                for (index, value) in items.iter().enumerate() {
                    self.record_inputs(&format!("{label}[{index}]"), value);
                }
            }
            Val::Enum(name, tag, payloads) => {
                self.inputs
                    .push((format!("variant of {label}"), tag.clone()));
                let names = self
                    .cx
                    .enum_variants(name, &[])
                    .map(|variants| {
                        variants
                            .into_iter()
                            .map(|(name, _)| name)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                for (variant, items) in payloads.iter().enumerate() {
                    let variant = names
                        .get(variant)
                        .cloned()
                        .unwrap_or_else(|| variant.to_string());
                    for (index, value) in items.iter().enumerate() {
                        self.record_inputs(&format!("{label}.{variant}[{index}]"), value);
                    }
                }
            }
            _ => {}
        }
    }

    fn cfg(&self) -> Result<Cfg, String> {
        let blocks = &self.function.blocks;
        let count = blocks.len();
        let mut succs = vec![Vec::new(); count];
        for (index, block) in blocks.iter().enumerate() {
            for label in terminator_targets(&block.terminator) {
                succs[index].push(self.labels[label]);
            }
        }
        let entry = *self
            .labels
            .get(self.function.entry.as_str())
            .ok_or("the function has no entry block")?;
        // Postorder by an explicit stack.
        let mut visited = vec![false; count];
        let mut post = Vec::new();
        let mut stack = vec![(entry, 0usize)];
        visited[entry] = true;
        while let Some((node, next)) = stack.last_mut() {
            if let Some(&succ) = succs[*node].get(*next) {
                *next += 1;
                if !visited[succ] {
                    visited[succ] = true;
                    stack.push((succ, 0));
                }
            } else {
                post.push(*node);
                stack.pop();
            }
        }
        let rpo: Vec<usize> = post.iter().rev().copied().collect();
        let mut order = vec![usize::MAX; count];
        for (position, &node) in rpo.iter().enumerate() {
            order[node] = position;
        }
        let mut preds = vec![Vec::new(); count];
        for (node, targets) in succs.iter().enumerate() {
            if !visited[node] {
                continue;
            }
            for &target in targets {
                preds[target].push(node);
            }
        }
        // Dominators (Cooper, Harvey, Kennedy).
        let mut idom = vec![usize::MAX; count];
        idom[entry] = entry;
        let intersect = |idom: &Vec<usize>, mut a: usize, mut b: usize| {
            while a != b {
                while order[a] > order[b] {
                    a = idom[a];
                }
                while order[b] > order[a] {
                    b = idom[b];
                }
            }
            a
        };
        let mut changed = true;
        while changed {
            changed = false;
            for &node in rpo.iter().skip(1) {
                let mut new = usize::MAX;
                for &pred in &preds[node] {
                    if idom[pred] == usize::MAX {
                        continue;
                    }
                    new = if new == usize::MAX {
                        pred
                    } else {
                        intersect(&idom, pred, new)
                    };
                }
                if new != usize::MAX && idom[node] != new {
                    idom[node] = new;
                    changed = true;
                }
            }
        }
        let dominates = |a: usize, mut b: usize| loop {
            if a == b {
                return true;
            }
            if b == entry || idom[b] == usize::MAX {
                return false;
            }
            b = idom[b];
        };
        let mut loops: BTreeMap<usize, Loop> = BTreeMap::new();
        for &node in &rpo {
            for &target in &succs[node] {
                if order[target] > order[node] {
                    continue;
                }
                if !dominates(target, node) {
                    return Err("the control-flow graph is irreducible".to_string());
                }
                let body = &mut loops
                    .entry(target)
                    .or_insert_with(|| Loop {
                        header: target,
                        body: BTreeSet::from([target]),
                        check_fails: BTreeSet::new(),
                        check_region: BTreeSet::new(),
                        writes: BTreeSet::new(),
                        counters: BTreeMap::new(),
                        bounds: BTreeMap::new(),
                    })
                    .body;
                let mut work = vec![node];
                while let Some(block) = work.pop() {
                    if body.insert(block) {
                        work.extend(preds[block].iter().copied());
                    }
                }
            }
        }
        // A block that ends the function belongs to every loop that holds
        // all of its predecessors, so its check runs where they do.
        for (index, targets) in succs.iter().enumerate() {
            if !targets.is_empty() || !visited[index] || preds[index].is_empty() {
                continue;
            }
            for lp in loops.values_mut() {
                if preds[index].iter().all(|pred| lp.body.contains(pred)) {
                    lp.body.insert(index);
                }
            }
        }
        // Each loop's own checks: failure blocks whose innermost loop it is.
        let headers: Vec<usize> = loops.keys().copied().collect();
        let innermost = |block: usize, loops: &BTreeMap<usize, Loop>| {
            headers
                .iter()
                .filter(|header| loops[header].body.contains(&block))
                .min_by_key(|header| loops[header].body.len())
                .copied()
        };
        let mut own_fails: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        for (index, block) in blocks.iter().enumerate() {
            if let Terminator::AssertFail {
                kind: AssertKind::Invariant | AssertKind::Decreases,
                ..
            } = block.terminator
            {
                if let Some(header) = innermost(index, &loops) {
                    own_fails.entry(header).or_default().insert(index);
                }
            }
        }
        for (header, fails) in own_fails {
            let lp = loops.get_mut(&header).expect("loop of a failure block");
            let mut region = BTreeSet::new();
            let mut work: Vec<usize> = fails.iter().copied().collect();
            while let Some(block) = work.pop() {
                if !lp.body.contains(&block) || !region.insert(block) || block == header {
                    continue;
                }
                work.extend(preds[block].iter().copied());
            }
            region.insert(header);
            lp.check_region = region;
            lp.check_fails = fails;
        }
        let loan_roots = loan_roots(blocks);
        for lp in loops.values_mut() {
            for &block in &lp.body {
                collect_writes(&blocks[block], &loan_roots, true, &mut lp.writes);
            }
            lp.writes.extend(self.captured.iter().cloned());
            let mut defs: BTreeMap<&str, Vec<&Rvalue>> = BTreeMap::new();
            for &block in &lp.body {
                for instruction in &blocks[block].instructions {
                    if let Instruction::Assign { target, value } = instruction {
                        defs.entry(target.as_str()).or_default().push(value);
                    }
                }
            }
            let mut steps: BTreeMap<String, Option<Step>> = BTreeMap::new();
            for &block in &lp.body {
                for instruction in &blocks[block].instructions {
                    if let Instruction::Assign { target, value } = instruction {
                        if target.contains('.') {
                            continue;
                        }
                        let step = counter_step(target, value, &defs, 0);
                        let slot = steps.entry(target.clone()).or_insert(step);
                        if *slot != step {
                            *slot = None;
                        }
                    }
                }
            }
            lp.counters = steps
                .into_iter()
                .filter_map(|(name, step)| step.map(|step| (name, step)))
                .filter(|(name, _)| {
                    lp.writes.contains(name) && !loan_roots.values().any(|root| root == name)
                })
                .collect();
            // A local some other instruction writes is no counter.
            let mut other = BTreeSet::new();
            for &block in &lp.body {
                let mut writes = BTreeSet::new();
                collect_writes(&blocks[block], &loan_roots, false, &mut writes);
                other.extend(writes);
            }
            lp.counters.retain(|name, _| !other.contains(name));
            lp.bounds = self.counter_bounds(lp, &preds);
        }
        Ok(Cfg { rpo, loops })
    }

    /// The loop's counters `c` whose only write is `c = c + 1` in a block
    /// entered only when a test `c < bound` holds, with `bound` a constant or
    /// a local the loop does not write: the shape of a counted `range` loop.
    fn counter_bounds(&self, lp: &Loop, preds: &[Vec<usize>]) -> BTreeMap<String, Operand> {
        let blocks = &self.function.blocks;
        let mut bounds = BTreeMap::new();
        for &block in &lp.body {
            let Terminator::Branch {
                condition: Operand::Place(test),
                then_label,
                ..
            } = &blocks[block].terminator
            else {
                continue;
            };
            let Some(Instruction::Assign {
                value:
                    Rvalue::Binary {
                        op: BinaryOp::Less,
                        left: Operand::Place(counter),
                        right: bound @ (Operand::Place(_) | Operand::Int(_)),
                        ..
                    },
                ..
            }) = blocks[block].instructions.iter().rev().find(
                |instruction| matches!(instruction, Instruction::Assign { target, .. } if target == test),
            )
            else {
                continue;
            };
            if lp.counters.get(counter) != Some(&Step::Up)
                || matches!(bound, Operand::Place(place) if lp.writes.contains(place.split('.').next().unwrap_or(place)))
            {
                continue;
            }
            let step = self.labels[then_label.as_str()];
            if !lp.body.contains(&step) || preds[step] != [block] {
                continue;
            }
            let writes: Vec<&Rvalue> = lp
                .body
                .iter()
                .flat_map(|&body| &blocks[body].instructions)
                .filter_map(|instruction| match instruction {
                    Instruction::Assign { target, value } if target == counter => Some(value),
                    _ => None,
                })
                .collect();
            let unit_step = matches!(
                writes.as_slice(),
                [Rvalue::Binary {
                    op: BinaryOp::Add,
                    left: Operand::Place(left),
                    right: Operand::Int(1),
                    ..
                }] if left == counter
            );
            let in_step = blocks[step].instructions.iter().any(|instruction| {
                matches!(instruction, Instruction::Assign { target, .. } if target == counter)
            });
            if unit_step && in_step {
                bounds.insert(counter.clone(), bound.clone());
            }
        }
        bounds
    }

    /// Executes the blocks of `region` reachable from `entry`, in reverse
    /// postorder. Returns the states that leave the region, by target, and
    /// the states that jump back to `header`.
    fn region(
        &mut self,
        cfg: &Cfg,
        region: &BTreeSet<usize>,
        entry: usize,
        state: State,
        header: Option<usize>,
    ) -> (Vec<(usize, State)>, Vec<State>) {
        let mut pending: BTreeMap<usize, Vec<State>> = BTreeMap::from([(entry, vec![state])]);
        let mut exits = Vec::new();
        let mut backs = Vec::new();
        for &block in &cfg.rpo {
            if !region.contains(&block) {
                continue;
            }
            let Some(states) = pending.remove(&block) else {
                continue;
            };
            let state = self.merge(states);
            if state.pc == "false" {
                continue;
            }
            let successors = if Some(block) != header && cfg.loops.contains_key(&block) {
                self.run_loop(cfg, &cfg.loops[&block], state)
            } else {
                self.block(block, state)
            };
            for (target, state) in successors {
                if Some(target) == header {
                    backs.push(state);
                } else if region.contains(&target) {
                    pending.entry(target).or_default().push(state);
                } else {
                    exits.push((target, state));
                }
            }
        }
        (exits, backs)
    }

    fn run_loop(&mut self, cfg: &Cfg, lp: &Loop, state: State) -> Vec<(usize, State)> {
        let checked = !lp.check_fails.is_empty();
        if checked {
            self.loop_checks(cfg, lp, state.clone());
        }
        let mut havoc = state.clone();
        for root in &lp.writes {
            self.havoc_root(&mut havoc, root);
        }
        // A counter the loop only steps up stays at least its entry value,
        // and one it only steps down at most that value.
        for (root, direction) in &lp.counters {
            if let (Some(Val::Int(before, _)), Some(Val::Int(after, _))) =
                (state.env.get(root), havoc.env.get(root))
            {
                let fact = match direction {
                    Step::Up => format!("(<= {before} {after})"),
                    Step::Down => format!("(<= {after} {before})"),
                };
                self.assume(&havoc.pc.clone(), fact);
            }
        }
        // A bounded counter never passes the larger of its entry value and
        // its bound.
        for (root, bound) in &lp.bounds {
            let bound = match self.operand(&mut havoc, bound, None) {
                Val::Int(term, _) if !term.starts_with('-') => Some(term),
                _ => None,
            };
            if let (Some(bound), Some(Val::Int(before, _)), Some(Val::Int(after, _))) =
                (bound, state.env.get(root), havoc.env.get(root))
            {
                let fact = format!("(or (= {after} {before}) (<= {after} {bound}))");
                self.assume(&havoc.pc.clone(), fact);
            }
        }
        havoc.loans.retain(|loan, _| !lp.writes.contains(loan));
        let added: Vec<usize> = lp
            .check_fails
            .iter()
            .copied()
            .filter(|fail| self.assumed_fails.insert(*fail))
            .collect();
        let (exits, backs) = self.region(cfg, &lp.body, lp.header, havoc, Some(lp.header));
        for fail in added {
            self.assumed_fails.remove(&fail);
        }
        if checked && !backs.is_empty() {
            let back = self.merge(backs);
            self.loop_checks(cfg, lp, back);
        }
        exits
    }

    /// Proves the loop's own checks for one state that reaches its header.
    fn loop_checks(&mut self, cfg: &Cfg, lp: &Loop, state: State) {
        let mut region = lp.check_region.clone();
        region.extend(lp.check_fails.iter().copied());
        self.region(cfg, &region, lp.header, state, Some(lp.header));
    }

    // ----- Merging ---------------------------------------------------------

    fn merge(&mut self, mut states: Vec<State>) -> State {
        states.retain(|state| state.pc != "false");
        match states.len() {
            0 => {
                return State {
                    pc: "false".to_string(),
                    env: BTreeMap::new(),
                    loans: BTreeMap::new(),
                }
            }
            1 => return states.pop().expect("one state"),
            _ => {}
        }
        let pcs: Vec<String> = states.iter().map(|state| state.pc.clone()).collect();
        let pc = self.define("Bool", "pc", format!("(or {})", pcs.join(" ")));
        let mut keys: BTreeSet<String> = BTreeSet::new();
        for state in &states {
            keys.extend(state.env.keys().cloned());
        }
        let mut env = BTreeMap::new();
        for key in keys {
            let values: Vec<Option<&Val>> =
                states.iter().map(|state| state.env.get(&key)).collect();
            if values.iter().any(Option::is_none) {
                continue;
            }
            let values: Vec<&Val> = values.into_iter().flatten().collect();
            let merged = self.merge_vals(&pcs, &values, &key);
            env.insert(key, merged);
        }
        let first = &states[0].loans;
        let loans = first
            .iter()
            .filter(|(name, loan)| {
                states
                    .iter()
                    .all(|state| state.loans.get(*name) == Some(*loan))
            })
            .map(|(name, loan)| (name.clone(), loan.clone()))
            .collect();
        State { pc, env, loans }
    }

    fn merge_terms(&mut self, sort: &str, pcs: &[String], terms: &[&str], hint: &str) -> String {
        if terms.iter().all(|term| *term == terms[0]) {
            return terms[0].to_string();
        }
        let mut merged = terms[terms.len() - 1].to_string();
        for index in (0..terms.len() - 1).rev() {
            merged = format!("(ite {} {} {merged})", pcs[index], terms[index]);
        }
        self.define(sort, hint, merged)
    }

    fn merge_vals(&mut self, pcs: &[String], values: &[&Val], hint: &str) -> Val {
        let first = values[0];
        if values.iter().all(|value| *value == first) {
            return first.clone();
        }
        // Different function values merge to one the verifier does not
        // follow.
        for (pc, value) in pcs.iter().zip(values) {
            self.oblige_escape(value, pc);
        }
        // Values of different shapes, such as a havocked `Opaque` and a
        // modeled value, merge to a value the encoding knows nothing about.
        if !values.iter().all(|value| same_shape(first, value)) {
            return Val::Opaque;
        }
        let scalars: Vec<&str> = values
            .iter()
            .filter_map(|value| scalar_term(value))
            .collect();
        match first {
            Val::Int(_, ty) => Val::Int(self.merge_terms("Int", pcs, &scalars, hint), *ty),
            Val::Bool(_) => Val::Bool(self.merge_terms("Bool", pcs, &scalars, hint)),
            Val::Str(_) => Val::Str(self.merge_terms("String", pcs, &scalars, hint)),
            Val::Float(_, float) => {
                Val::Float(self.merge_terms(float.sort(), pcs, &scalars, hint), *float)
            }
            Val::Map {
                key,
                value,
                values: first_values,
                ..
            } => {
                let sort = key_sort(key).expect("a modeled dict has a modeled key");
                let mut lens = Vec::new();
                let mut key_sets = Vec::new();
                let mut stores = Vec::new();
                for each in values {
                    if let Val::Map {
                        len, keys, values, ..
                    } = each
                    {
                        lens.push(len.as_str());
                        key_sets.push(keys.as_str());
                        stores.extend(values.as_deref());
                    }
                }
                let len = self.merge_terms("Int", pcs, &lens, &format!("{hint}_len"));
                let keys = self.merge_terms(
                    &format!("(Array {sort} Bool)"),
                    pcs,
                    &key_sets,
                    &format!("{hint}_keys"),
                );
                let merged = match (first_values, element_sort(value)) {
                    (Some(_), Some(value_sort)) if stores.len() == values.len() => {
                        Some(self.merge_terms(
                            &format!("(Array {sort} {value_sort})"),
                            pcs,
                            &stores,
                            hint,
                        ))
                    }
                    _ => None,
                };
                Val::Map {
                    len,
                    keys,
                    values: merged,
                    key: key.clone(),
                    value: value.clone(),
                }
            }
            Val::Set { elem, .. } => {
                let sort = key_sort(elem).expect("a modeled set has a modeled element");
                let mut lens = Vec::new();
                let mut member_sets = Vec::new();
                for each in values {
                    if let Val::Set { len, members, .. } = each {
                        lens.push(len.as_str());
                        member_sets.push(members.as_str());
                    }
                }
                let len = self.merge_terms("Int", pcs, &lens, &format!("{hint}_len"));
                let members = self.merge_terms(
                    &format!("(Array {sort} Bool)"),
                    pcs,
                    &member_sets,
                    &format!("{hint}_members"),
                );
                Val::Set {
                    len,
                    members,
                    elem: elem.clone(),
                }
            }
            Val::List { elem, data, .. } => {
                let mut lens = Vec::new();
                let mut datas = Vec::new();
                for value in values {
                    if let Val::List { len, data, .. } = value {
                        lens.push(len.as_str());
                        datas.extend(data.as_ref());
                    }
                }
                let len = self.merge_terms("Int", pcs, &lens, &format!("{hint}_len"));
                // A store merges only when every path still models one.
                let data = if data.is_some() && datas.len() == values.len() {
                    self.merge_stores(pcs, &datas, hint)
                } else {
                    None
                };
                Val::List {
                    len,
                    data,
                    elem: elem.clone(),
                }
            }
            Val::Tuple(_) => Val::Tuple(self.merge_children(pcs, values, hint)),
            Val::Record(class, fields) => {
                let merged = self.merge_children(pcs, values, hint);
                Val::Record(
                    class.clone(),
                    fields
                        .iter()
                        .map(|(field, _)| field.clone())
                        .zip(merged)
                        .collect(),
                )
            }
            Val::Enum(name, _, payloads) => {
                let tags: Vec<&str> = values.iter().filter_map(|value| tag_term(value)).collect();
                let tag = self.merge_terms("Int", pcs, &tags, &format!("{hint}_tag"));
                let mut merged = self.merge_children(pcs, values, hint).into_iter();
                let payloads = payloads
                    .iter()
                    .map(|items| merged.by_ref().take(items.len()).collect())
                    .collect();
                Val::Enum(name.clone(), tag, payloads)
            }
            Val::Union(..) => {
                let tags: Vec<&str> = values.iter().filter_map(|value| tag_term(value)).collect();
                let tag = self.merge_terms("Int", pcs, &tags, &format!("{hint}_tag"));
                Val::Union(tag, self.merge_children(pcs, values, hint))
            }
            // Equal units and opaque values returned above.
            Val::Unit | Val::Opaque | Val::Func(_) | Val::Task(_) | Val::Contracted(_) => {
                Val::Opaque
            }
        }
    }

    /// Merges the components of same-shaped values position by position.
    fn merge_children(&mut self, pcs: &[String], values: &[&Val], hint: &str) -> Vec<Val> {
        let children: Vec<Vec<&Val>> = values.iter().map(|value| children(value)).collect();
        (0..children[0].len())
            .map(|index| {
                let column: Vec<&Val> = children.iter().map(|items| items[index]).collect();
                self.merge_vals(pcs, &column, &format!("{hint}_{index}"))
            })
            .collect()
    }

    // ----- Blocks ----------------------------------------------------------

    fn block(&mut self, index: usize, mut state: State) -> Vec<(usize, State)> {
        let block: &'a BasicBlock = &self.function.blocks[index];
        self.in_ghost = crate::mir::is_ghost_label(&block.label);
        for (position, instruction) in block.instructions.iter().enumerate() {
            self.position = (index, position);
            self.instruction(&mut state, instruction);
        }
        let target = |exec: &Self, label: &str| exec.labels[label];
        match &block.terminator {
            Terminator::Return(value) => {
                let result = self.operand(&mut state, value, None);
                self.prove_return_invariants(&state, &result);
                Vec::new()
            }
            Terminator::Goto(label) => vec![(target(self, label), state)],
            Terminator::Branch {
                condition,
                then_label,
                else_label,
            } => {
                let (pass, fail) = (target(self, then_label), target(self, else_label));
                if let Some(next) = self.clause_branch(&state, fail, pass) {
                    return next;
                }
                let condition = match self.operand(&mut state, condition, None) {
                    Val::Bool(term) => term,
                    _ => self.fresh_const("Bool", "branch"),
                };
                let then_pc = and(&state.pc, &condition);
                let else_pc = and(&state.pc, &not(&condition));
                let mut then_state = state.clone();
                then_state.pc = then_pc;
                state.pc = else_pc;
                vec![
                    (target(self, then_label), then_state),
                    (target(self, else_label), state),
                ]
            }
            Terminator::ForRange {
                binding,
                body_label,
                exit_label,
                ..
            } => {
                let more = self.fresh_const("Bool", "more");
                let mut body = state.clone();
                body.pc = and(&state.pc, &more);
                state.pc = and(&state.pc, &not(&more));
                self.havoc_root(&mut body, binding);
                vec![
                    (target(self, body_label), body),
                    (target(self, exit_label), state),
                ]
            }
            Terminator::Match {
                scrutinee,
                arms,
                otherwise,
            } => {
                let value = self.operand(&mut state, scrutinee, None);
                let mut out = Vec::new();
                let mut remaining = state.pc.clone();
                for arm in arms {
                    let condition = if arm.wildcard {
                        Some("true".to_string())
                    } else {
                        match (&value, &arm.variant_name) {
                            (Val::Enum(name, tag, _), Some(variant)) => self
                                .cx
                                .variant_index(name, variant)
                                .map(|index| format!("(= {tag} {index})")),
                            _ => None,
                        }
                    };
                    // An arm whose test the encoding does not model is
                    // taken on an unknown condition.
                    let condition = condition.unwrap_or_else(|| self.fresh_const("Bool", "arm"));
                    let mut arm_state = state.clone();
                    arm_state.pc = and(&remaining, &condition);
                    remaining = and(&remaining, &not(&condition));
                    out.push((target(self, &arm.label), arm_state));
                }
                state.pc = remaining;
                out.push((target(self, otherwise), state));
                out
            }
            Terminator::AssertFail {
                message,
                span,
                kind,
                ..
            } => {
                // A function's own `requires` holds on entry, since every
                // call proves it, and a loop's checks are proved apart.
                let obligation = match kind {
                    AssertKind::Requires => None,
                    AssertKind::Assert => Some(ObligationKind::Assert),
                    AssertKind::Ensures => Some(ObligationKind::Ensures),
                    AssertKind::Invariant => Some(ObligationKind::LoopInvariant),
                    AssertKind::Decreases => Some(ObligationKind::Decreases),
                };
                match obligation.filter(|_| !self.assumed_fails.contains(&index)) {
                    Some(obligation) => {
                        let message = check_message(*kind, message.as_ref());
                        self.oblige(
                            obligation,
                            *span,
                            message,
                            &state.pc,
                            Goal::Holds("false".into()),
                        );
                        // For a build, the check whose failure this is may
                        // go when no path reaches it.
                        if self.prove_sites {
                            self.copy_as_site((index, CHECK_FAILURE_SITE));
                        }
                    }
                    None => self.fact(not(&state.pc)),
                }
                Vec::new()
            }
            Terminator::Unreachable => {
                self.fact(not(&state.pc));
                Vec::new()
            }
        }
    }

    fn instruction(&mut self, state: &mut State, instruction: &Instruction) {
        match instruction {
            Instruction::SourceLocation { line, column } => {
                self.span = Span {
                    line: *line as usize,
                    column: *column as usize,
                };
            }
            Instruction::Safepoint
            | Instruction::SourceName { .. }
            | Instruction::Retain { .. }
            | Instruction::Eval { .. }
            | Instruction::EndLoan { .. }
            | Instruction::ReturnLoan { .. }
            | Instruction::PushCleanup { .. }
            // Lending and release instructions come from the ownership
            // pass, which runs after verification.
            | Instruction::Lend { .. }
            | Instruction::EndLend { .. }
            | Instruction::Release { .. } => {}
            Instruction::IndexInRange { list, index } => {
                let list = self.read_place(state, list);
                let index = self.read_place(state, index);
                if let (Val::List { len, .. }, Val::Int(index, _)) = (list, index) {
                    let pc = state.pc.clone();
                    self.assume(&pc, format!("(and (<= 0 {index}) (< {index} {len}))"));
                }
            }
            Instruction::BeginLoan { loan, source, .. } => {
                state
                    .loans
                    .insert(loan.clone(), Loan::Place(source.clone()));
            }
            Instruction::BeginElementLoan {
                loan,
                source,
                selector,
                projection,
                span,
                checked,
                ..
            } => {
                let collection = self.read_place(state, source);
                let selector = self.operand(state, selector, Some(IntTy::INT64));
                let value = match (&collection, &selector) {
                    (Val::List { len, .. }, Val::Int(index, _)) => {
                        if *checked {
                            self.oblige_index(state, *span, len, index);
                            self.oblige_site(state, *span, len, index);
                        }
                        let index = self.normalize_index(len, index);
                        Loan::Element {
                            list: source.clone(),
                            index,
                            projection: projection.clone(),
                        }
                    }
                    _ => Loan::Unknown(root_of(source).to_string()),
                };
                state.loans.insert(loan.clone(), value);
            }
            Instruction::BeginReturnedLoan { loan, origin, .. } => {
                state
                    .loans
                    .insert(loan.clone(), Loan::Unknown(root_of(origin).to_string()));
            }
            Instruction::Reborrow {
                loan,
                parent,
                projection,
                ..
            } => {
                let value = match state.loans.get(parent).cloned() {
                    Some(Loan::Place(place)) if projection.is_empty() => Loan::Place(place),
                    Some(Loan::Place(place)) => Loan::Place(format!("{place}.{projection}")),
                    Some(Loan::Element {
                        list,
                        index,
                        projection: inner,
                    }) => Loan::Element {
                        list,
                        index,
                        projection: join_path(&inner, projection),
                    },
                    Some(Loan::Unknown(root)) => Loan::Unknown(root),
                    None => Loan::Unknown(root_of(parent).to_string()),
                };
                state.loans.insert(loan.clone(), value);
            }
            Instruction::ReborrowElement {
                loan,
                parent,
                selector,
                projection,
                span,
                ..
            } => {
                let value = match state.loans.get(parent).cloned() {
                    Some(Loan::Place(place)) => {
                        let collection = self.read_place(state, &place);
                        let selector = self.operand(state, selector, Some(IntTy::INT64));
                        match (&collection, &selector) {
                            (Val::List { len, .. }, Val::Int(index, _)) => {
                                self.oblige_index(state, *span, len, index);
                                Loan::Element {
                                    list: place.clone(),
                                    index: self.normalize_index(len, index),
                                    projection: projection.clone(),
                                }
                            }
                            _ => Loan::Unknown(root_of(&place).to_string()),
                        }
                    }
                    Some(Loan::Element { list, .. }) | Some(Loan::Unknown(list)) => {
                        Loan::Unknown(root_of(&list).to_string())
                    }
                    None => Loan::Unknown(root_of(parent).to_string()),
                };
                state.loans.insert(loan.clone(), value);
            }
            Instruction::ReadLoan { target, loan } => {
                let value = match state.loans.get(loan).cloned() {
                    Some(Loan::Place(place)) => self.read_place(state, &place),
                    Some(Loan::Element {
                        list,
                        index,
                        projection,
                    }) => {
                        let list = self.read_place(state, &list);
                        let mut value = self.element(&list, &index, target);
                        for segment in projection.split('.').filter(|segment| !segment.is_empty()) {
                            value = project(&value, segment).unwrap_or(Val::Opaque);
                        }
                        if value == Val::Opaque {
                            self.fresh_local(target)
                        } else {
                            value
                        }
                    }
                    _ => self.fresh_local(target),
                };
                self.write_place(state, target, value);
            }
            Instruction::WriteLoan { loan, value } => {
                let hint = self.loan_int_ty(state, loan);
                let value = self.rvalue(state, None, value, hint);
                match state.loans.get(loan).cloned() {
                    Some(Loan::Place(place)) => self.write_place(state, &place, value),
                    Some(Loan::Element {
                        list,
                        index,
                        projection,
                    }) => {
                        let current = self.read_place(state, &list);
                        match current {
                            Val::List { len, data, elem } => {
                                let list_value = Val::List {
                                    len: len.clone(),
                                    data: data.clone(),
                                    elem: elem.clone(),
                                };
                                // A write through a field of the element
                                // rewrites the whole element.
                                let element = if projection.is_empty() {
                                    Some(value)
                                } else {
                                    let mut element = self.element(&list_value, &index, "element");
                                    let segments: Vec<&str> = projection.split('.').collect();
                                    set_in(&mut element, &segments, value).then_some(element)
                                };
                                let data = match (data, element) {
                                    (Some(data), Some(element)) => {
                                        self.put_or_fresh(&data, &elem, &index, &element, "store")
                                    }
                                    _ => self.fresh_store(&elem, "store"),
                                };
                                self.write_place(state, &list, Val::List { len, data, elem });
                            }
                            _ => self.havoc_place(state, &list),
                        }
                    }
                    Some(Loan::Unknown(root)) => self.havoc_root(state, &root),
                    None => self.havoc_root(state, root_of(loan)),
                }
            }
            Instruction::Assign { target, value } => {
                let hint = self.place_type(target).as_ref().and_then(IntTy::of);
                let value = self.rvalue(state, Some(target), value, hint);
                self.write_place(state, target, value);
            }
            Instruction::PopCleanup { place, run, .. } => {
                if *run {
                    self.havoc_root(state, root_of(place));
                }
            }
        }
    }

    fn loan_int_ty(&self, state: &State, loan: &str) -> Option<IntTy> {
        match state.loans.get(loan)? {
            Loan::Place(place) => self.place_type(place).as_ref().and_then(IntTy::of),
            Loan::Element {
                list, projection, ..
            } => match self.place_type(list)? {
                Type::Named(name, args) if name == "list" && args.len() == 1 => {
                    let mut ty = args[0].clone();
                    for segment in projection.split('.').filter(|segment| !segment.is_empty()) {
                        ty = self.cx.project_type(&ty, segment)?;
                    }
                    IntTy::of(&ty)
                }
                _ => None,
            },
            _ => None,
        }
    }

    /// A fresh value of the type of the place `target`.
    fn fresh_local(&mut self, target: &str) -> Val {
        self.place_type(target)
            .map_or(Val::Opaque, |ty| self.fresh(&ty, target))
    }

    /// A fresh store for elements of type `elem`, when the encoding models
    /// them: scalars, and tuples and classes of modeled values.
    pub(super) fn fresh_store(&mut self, elem: &Type, hint: &str) -> Option<Store> {
        self.fresh_store_at(elem, hint, 0)
    }

    fn fresh_store_at(&mut self, elem: &Type, hint: &str, depth: usize) -> Option<Store> {
        if let Some(sort) = element_sort(elem) {
            let array = self.fresh_const(&format!("(Array Int {sort})"), hint);
            return Some(Store::Leaf(array, sort));
        }
        if depth > MAX_DEPTH {
            return None;
        }
        let items = self.element_items(elem)?;
        items
            .iter()
            .enumerate()
            .map(|(index, (_, ty))| self.fresh_store_at(ty, &format!("{hint}_{index}"), depth + 1))
            .collect::<Option<Vec<_>>>()
            .map(Store::Items)
    }

    /// The items of a tuple or the fields of a class, in order.
    fn element_items(&self, elem: &Type) -> Option<Vec<(Option<String>, Type)>> {
        match elem {
            Type::Tuple(items) => Some(items.iter().map(|item| (None, item.clone())).collect()),
            Type::Named(name, args) => self.cx.class_fields(name, args).map(|fields| {
                fields
                    .into_iter()
                    .map(|(field, ty)| (Some(field), ty))
                    .collect()
            }),
            _ => None,
        }
    }

    /// The element a store holds at the term `index`, as `select` terms.
    /// The index may name a quantifier's bound variable.
    pub(super) fn store_select(&self, store: &Store, elem: &Type, index: &str) -> Val {
        match store {
            Store::Leaf(array, _) => scalar_val(format!("(select {array} {index})"), elem),
            Store::Items(stores) => {
                let Some(items) = self.element_items(elem) else {
                    return Val::Opaque;
                };
                let values = stores
                    .iter()
                    .zip(&items)
                    .map(|(store, (_, ty))| self.store_select(store, ty, index));
                match elem {
                    Type::Named(name, _) => Val::Record(
                        name.clone(),
                        items
                            .iter()
                            .map(|(field, _)| field.clone().unwrap_or_default())
                            .zip(values)
                            .collect(),
                    ),
                    _ => Val::Tuple(values.collect()),
                }
            }
        }
    }

    /// The store with `value` at `index`, when the store models each of
    /// its scalars.
    fn store_put(&mut self, store: &Store, index: &str, value: &Val) -> Option<Store> {
        match (store, value) {
            (Store::Leaf(array, sort), Val::Float(term, float)) => {
                let term = FloatTy::of_sort(sort)?.convert(term, *float);
                let array = self.define(
                    &format!("(Array Int {sort})"),
                    "store",
                    format!("(store {array} {index} {term})"),
                );
                Some(Store::Leaf(array, sort))
            }
            (Store::Leaf(array, sort), Val::Int(term, _) | Val::Bool(term) | Val::Str(term)) => {
                let array = self.define(
                    &format!("(Array Int {sort})"),
                    "store",
                    format!("(store {array} {index} {term})"),
                );
                Some(Store::Leaf(array, sort))
            }
            (Store::Items(stores), Val::Tuple(values)) if stores.len() == values.len() => stores
                .iter()
                .zip(values)
                .map(|(store, value)| self.store_put(store, index, value))
                .collect::<Option<Vec<_>>>()
                .map(Store::Items),
            (Store::Items(stores), Val::Record(_, fields)) if stores.len() == fields.len() => {
                stores
                    .iter()
                    .zip(fields)
                    .map(|(store, (_, value))| self.store_put(store, index, value))
                    .collect::<Option<Vec<_>>>()
                    .map(Store::Items)
            }
            _ => None,
        }
    }

    /// `store_put`, or a fresh store when the value is not modeled.
    fn put_or_fresh(
        &mut self,
        store: &Store,
        elem: &Type,
        index: &str,
        value: &Val,
        hint: &str,
    ) -> Option<Store> {
        match self.store_put(store, index, value) {
            Some(store) => Some(store),
            None => self.fresh_store(elem, hint),
        }
    }

    fn merge_stores(&mut self, pcs: &[String], stores: &[&Store], hint: &str) -> Option<Store> {
        match stores.first()? {
            Store::Leaf(_, sort) => {
                let sort = *sort;
                let mut arrays = Vec::new();
                for store in stores {
                    let Store::Leaf(array, _) = store else {
                        return None;
                    };
                    arrays.push(array.as_str());
                }
                let array = self.merge_terms(&format!("(Array Int {sort})"), pcs, &arrays, hint);
                Some(Store::Leaf(array, sort))
            }
            Store::Items(first) => {
                let mut merged = Vec::new();
                for position in 0..first.len() {
                    let mut children = Vec::new();
                    for store in stores {
                        let Store::Items(items) = store else {
                            return None;
                        };
                        children.push(items.get(position)?);
                    }
                    merged.push(self.merge_stores(
                        pcs,
                        &children,
                        &format!("{hint}_{position}"),
                    )?);
                }
                Some(Store::Items(merged))
            }
        }
    }

    /// Names each scalar of a value built from `select` terms, with the
    /// range of its integer type.
    pub(super) fn name_scalars(&mut self, value: Val, hint: &str) -> Val {
        match value {
            Val::Int(term, int) => {
                let term = self.define("Int", hint, term);
                self.fact(int.contains(&term));
                Val::Int(term, int)
            }
            Val::Bool(term) => Val::Bool(self.define("Bool", hint, term)),
            Val::Str(term) => Val::Str(self.define("String", hint, term)),
            Val::Float(term, float) => Val::Float(self.define(float.sort(), hint, term), float),
            Val::Tuple(items) => Val::Tuple(
                items
                    .into_iter()
                    .map(|item| self.name_scalars(item, hint))
                    .collect(),
            ),
            Val::Record(class, fields) => Val::Record(
                class,
                fields
                    .into_iter()
                    .map(|(field, value)| {
                        let value = self.name_scalars(value, hint);
                        (field, value)
                    })
                    .collect(),
            ),
            other => other,
        }
    }

    /// The element at the normalized index `index` of `list`.
    pub(super) fn element(&mut self, list: &Val, index: &str, hint: &str) -> Val {
        match list {
            Val::List {
                data: Some(data),
                elem,
                ..
            } => {
                let value = self.store_select(data, elem, index);
                self.name_scalars(value, hint)
            }
            Val::List { elem, .. } => {
                let elem = elem.clone();
                self.fresh(&elem, hint)
            }
            _ => Val::Opaque,
        }
    }

    /// Aura reads a negative index from the end of the list.
    pub(super) fn normalize_index(&mut self, len: &str, index: &str) -> String {
        if let Ok(value) = index.parse::<i128>() {
            if value >= 0 {
                return index.to_string();
            }
        }
        self.define(
            "Int",
            "index",
            format!("(ite (< {index} 0) (+ {index} {len}) {index})"),
        )
    }

    fn oblige_index(&mut self, state: &State, span: Span, len: &str, index: &str) {
        let goal = format!("(and (<= (- {len}) {index}) (< {index} {len}))");
        self.oblige(
            ObligationKind::IndexInRange,
            span,
            "list index in range",
            &state.pc,
            Goal::Holds(goal),
        );
    }

    /// For a build, the check that the element access at the current
    /// position may skip its bounds check: the index is in range and not
    /// negative, since an unchecked access does not count from the end.
    fn oblige_site(&mut self, state: &State, span: Span, len: &str, index: &str) {
        if !self.prove_sites {
            return;
        }
        self.obligations.push(Obligation {
            kind: ObligationKind::IndexInRange,
            span,
            message: "list index is not negative and in range".to_string(),
            pc: state.pc.clone(),
            goal: Goal::Holds(format!("(and (<= 0 {index}) (< {index} {len}))")),
            facts: self.facts.len(),
            site: Some(self.position),
            erased: false,
        });
    }

    // ----- Rvalues ---------------------------------------------------------

    fn rvalue(
        &mut self,
        state: &mut State,
        target: Option<&str>,
        value: &Rvalue,
        hint: Option<IntTy>,
    ) -> Val {
        let checked = match value {
            Rvalue::Binary { trap, .. } | Rvalue::Unary { trap, .. } => trap.provable(),
            _ => false,
        };
        if !(checked && self.prove_sites) {
            return self.rvalue_value(state, target, value, hint);
        }
        // For a build, the operation's checks may go when each is proved
        // on every path. A path where the encoding makes none keeps them.
        let site = self.position;
        let before = self.obligations.len();
        self.trap_site = true;
        let result = self.rvalue_value(state, target, value, hint);
        self.trap_site = false;
        if !self.obligations[before..]
            .iter()
            .any(|obligation| obligation.site == Some(site))
        {
            self.obligations.push(Obligation {
                kind: ObligationKind::Overflow,
                span: self.span,
                message: "an operation the encoding does not check".to_string(),
                pc: state.pc.clone(),
                goal: Goal::Unsupported("not modeled".into()),
                facts: self.facts.len(),
                site: Some(site),
                erased: false,
            });
        }
        result
    }

    fn rvalue_value(
        &mut self,
        state: &mut State,
        target: Option<&str>,
        value: &Rvalue,
        hint: Option<IntTy>,
    ) -> Val {
        let target_type = target.and_then(|target| self.place_type(target));
        let fresh_target = |exec: &mut Self| match &target_type {
            Some(ty) => exec.fresh(ty, target.unwrap_or("value")),
            None => Val::Opaque,
        };
        match value {
            Rvalue::Use(operand) => self.followed_operand(state, operand, hint),
            // An adapted function value still calls the function it names.
            Rvalue::CallableAdapt { value, .. } => {
                match self.followed_operand(state, value, None) {
                    value @ (Val::Func(_) | Val::Contracted(_)) => value,
                    _ => fresh_target(self),
                }
            }
            Rvalue::Binary {
                op,
                left,
                right,
                span,
                trap,
            } => self.binary(state, *op, left, right, *span, *trap, target_type.as_ref()),
            Rvalue::Unary {
                op,
                value,
                span,
                trap,
            } => {
                let operand = self.operand(state, value, hint);
                match (op, operand) {
                    (UnaryOp::Not, Val::Bool(term)) => Val::Bool(not(&term)),
                    (UnaryOp::Neg, Val::Float(term, float)) => Val::Float(
                        self.define(float.sort(), "neg", format!("(fp.neg {term})")),
                        float,
                    ),
                    (UnaryOp::Neg, Val::Int(term, ty)) => {
                        let ty = target_type.as_ref().and_then(IntTy::of).unwrap_or(ty);
                        let result = self.define("Int", "neg", format!("(- {term})"));
                        if trap.checks_overflow() {
                            self.oblige_range(state, *span, &result, ty);
                        }
                        Val::Int(result, ty)
                    }
                    _ => {
                        if *trap == ArithmeticTrap::Runtime {
                            self.oblige(
                                ObligationKind::Overflow,
                                *span,
                                "operation outside the verified subset",
                                &state.pc.clone(),
                                Goal::Unsupported("this operator is not modeled yet".into()),
                            );
                        }
                        fresh_target(self)
                    }
                }
            }
            Rvalue::Cast { value, ty, span } => {
                let operand = self.operand(state, value, None);
                if let Some(to) = FloatTy::of(ty) {
                    return match operand {
                        Val::Float(term, from) => {
                            Val::Float(self.define(to.sort(), "cast", to.convert(&term, from)), to)
                        }
                        Val::Int(term, _) => {
                            let result = self.define(to.sort(), "cast", to.round_int(&term));
                            // Every integer up to the significand's width
                            // converts exactly; a wider one must round to
                            // itself.
                            let exact = to.exact_ints();
                            self.oblige(
                                ObligationKind::Overflow,
                                *span,
                                "integer converts to a float exactly",
                                &state.pc.clone(),
                                Goal::Holds(format!(
                                    "(or (and (<= (- {exact}) {term}) (<= {term} {exact})) (= (fp.to_real {result}) (to_real {term})))"
                                )),
                            );
                            Val::Float(result, to)
                        }
                        _ => fresh_target(self),
                    };
                }
                if let (Val::Float(term, _), Some(int)) = (&operand, IntTy::of(ty)) {
                    // A cast truncates toward zero, and needs a finite value
                    // whose truncation fits: one above `lo - 1` and below
                    // `hi + 1`, which NaN is not.
                    let Val::Float(_, float) = operand else {
                        unreachable!("matched above")
                    };
                    let truncated = format!("(fp.to_real (fp.roundToIntegral RTZ {term}))");
                    self.oblige(
                        ObligationKind::Overflow,
                        *span,
                        "float converts to an integer in range",
                        &state.pc.clone(),
                        Goal::Holds(format!(
                            "(and {} {})",
                            float.above(term, int.lo - 1),
                            float.below(term, int.hi + 1)
                        )),
                    );
                    let result = self.define("Int", "cast", format!("(to_int {truncated})"));
                    return Val::Int(result, int);
                }
                match (operand, IntTy::of(ty)) {
                    (Val::Int(term, _), Some(int)) => {
                        self.oblige(
                            ObligationKind::Overflow,
                            *span,
                            "integer conversion in range",
                            &state.pc.clone(),
                            Goal::Holds(int.contains(&term)),
                        );
                        Val::Int(term, int)
                    }
                    (_, Some(_)) => {
                        self.oblige(
                            ObligationKind::Overflow,
                            *span,
                            "integer conversion in range",
                            &state.pc.clone(),
                            Goal::Unsupported("floating-point operands are not modeled yet".into()),
                        );
                        fresh_target(self)
                    }
                    _ => fresh_target(self),
                }
            }
            Rvalue::Call { callee, args, .. } => {
                let result = self.call(state, target, callee, args);
                for root in self.captured.clone() {
                    self.havoc_root(state, &root);
                }
                result
            }
            Rvalue::VecLiteral {
                elements,
                element_type,
            } => {
                let len = elements.len().to_string();
                let hint = IntTy::of(element_type);
                let mut data = self.fresh_store(element_type, "literal");
                for (index, element) in elements.iter().enumerate() {
                    let value = self.operand(state, element, hint);
                    if let Some(store) = &data {
                        data = self.put_or_fresh(
                            &store.clone(),
                            element_type,
                            &index.to_string(),
                            &value,
                            "literal",
                        );
                    }
                }
                Val::List {
                    len,
                    data,
                    elem: element_type.clone(),
                }
            }
            Rvalue::SetLiteral {
                elements,
                element_type,
            } => {
                let Some(sort) = key_sort(element_type) else {
                    return fresh_target(self);
                };
                let mut len = "0".to_string();
                let mut members = no_keys(sort);
                for element in elements {
                    let value = self.operand(state, element, IntTy::of(element_type));
                    let Some(k) = key_term(&value) else {
                        return fresh_target(self);
                    };
                    len = self.len_after_add(&len, &members, &k);
                    members = self.define(
                        &format!("(Array {sort} Bool)"),
                        "members",
                        format!("(store {members} {k} true)"),
                    );
                }
                Val::Set {
                    len,
                    members,
                    elem: element_type.clone(),
                }
            }
            Rvalue::MapLiteral {
                entries,
                key_type,
                value_type,
            } => {
                let Some(sort) = key_sort(key_type) else {
                    return fresh_target(self);
                };
                let mut len = "0".to_string();
                let mut keys = no_keys(sort);
                let value_sort = element_sort(value_type);
                let mut values = value_sort.map(|value_sort| {
                    self.fresh_const(&format!("(Array {sort} {value_sort})"), "values")
                });
                for entry in entries {
                    let k = self.operand(state, &entry.key, IntTy::of(key_type));
                    let Some(k) = key_term(&k) else {
                        return fresh_target(self);
                    };
                    let stored = self.operand(state, &entry.value, IntTy::of(value_type));
                    values = match (values, scalar_term(&stored), value_sort) {
                        (Some(array), Some(term), Some(value_sort)) => Some(self.define(
                            &format!("(Array {sort} {value_sort})"),
                            "values",
                            format!("(store {array} {k} {term})"),
                        )),
                        _ => None,
                    };
                    len = self.len_after_add(&len, &keys, &k);
                    keys = self.define(
                        &format!("(Array {sort} Bool)"),
                        "keys",
                        format!("(store {keys} {k} true)"),
                    );
                }
                Val::Map {
                    len,
                    keys,
                    values,
                    key: key_type.clone(),
                    value: value_type.clone(),
                }
            }
            Rvalue::TupleLiteral {
                elements,
                element_types,
            } => Val::Tuple(
                elements
                    .iter()
                    .zip(element_types)
                    .map(|(element, ty)| self.operand(state, element, IntTy::of(ty)))
                    .collect(),
            ),
            Rvalue::TupleElement { tuple, index, .. } => match self.operand(state, tuple, None) {
                Val::Tuple(items) if *index < items.len() => items[*index].clone(),
                _ => fresh_target(self),
            },
            Rvalue::TupleTakeElement { place, index, .. } => match self.read_place(state, place) {
                Val::Tuple(items) if *index < items.len() => items[*index].clone(),
                _ => fresh_target(self),
            },
            Rvalue::Construct { class_name, fields } => {
                let declared = match &target_type {
                    Some(Type::Named(name, args)) => self.cx.class_fields(name, args),
                    _ => None,
                };
                let Some(declared) = declared else {
                    return fresh_target(self);
                };
                let mut values = Vec::new();
                for (field, ty) in declared {
                    let value = match fields.iter().find(|init| init.name == field) {
                        Some(init) => self.operand(state, &init.value, IntTy::of(&ty)),
                        None => self.fresh(&ty, &field),
                    };
                    values.push((field, value));
                }
                Val::Record(class_name.clone(), values)
            }
            Rvalue::Member { object, field } => match self.operand(state, object, None) {
                Val::Record(_, fields) => fields
                    .iter()
                    .find(|(name, _)| name == field)
                    .map(|(_, value)| value.clone())
                    .unwrap_or(Val::Opaque),
                _ => fresh_target(self),
            },
            Rvalue::EnumVariant {
                enum_name,
                variant_name,
                payloads,
            } => {
                let Some(Type::Named(name, args)) = &target_type else {
                    return fresh_target(self);
                };
                let Some(variants) = self.cx.enum_variants(name, args) else {
                    return fresh_target(self);
                };
                let Some(index) = variants.iter().position(|(name, _)| name == variant_name) else {
                    return fresh_target(self);
                };
                let mut all = Vec::new();
                for (position, (variant, types)) in variants.iter().enumerate() {
                    if position == index {
                        all.push(
                            payloads
                                .iter()
                                .zip(types)
                                .map(|(payload, ty)| self.operand(state, payload, IntTy::of(ty)))
                                .collect(),
                        );
                    } else {
                        all.push(types.iter().map(|ty| self.fresh(ty, variant)).collect());
                    }
                }
                let _ = enum_name;
                Val::Enum(name.clone(), index.to_string(), all)
            }
            Rvalue::VariantPayload {
                scrutinee,
                variant_name,
                index,
            } => {
                let value = self.operand(state, scrutinee, None);
                match &value {
                    Val::Enum(name, _, payloads) => self
                        .cx
                        .variant_index(name, variant_name)
                        .and_then(|variant| payloads.get(variant))
                        .and_then(|items| items.get(*index))
                        .cloned()
                        .unwrap_or(Val::Opaque),
                    _ => fresh_target(self),
                }
            }
            Rvalue::UnionTagTest {
                place,
                member_index,
                ..
            } => match self.read_place(state, place) {
                Val::Union(tag, _) => Val::Bool(format!("(= {tag} {member_index})")),
                _ => fresh_target(self),
            },
            Rvalue::UnionTakePayload {
                place,
                member_index,
                ..
            } => match self.read_place(state, place) {
                Val::Union(_, members) if *member_index < members.len() => {
                    members[*member_index].clone()
                }
                _ => fresh_target(self),
            },
            Rvalue::UnionInject {
                value,
                union_type,
                member_index,
                member_type,
            } => {
                let Type::Union(union) = union_type else {
                    return fresh_target(self);
                };
                let injected = self.operand(state, value, IntTy::of(member_type));
                let members = union
                    .members
                    .iter()
                    .enumerate()
                    .map(|(index, member)| {
                        if index == *member_index {
                            injected.clone()
                        } else {
                            self.fresh(member, "member")
                        }
                    })
                    .collect();
                Val::Union(member_index.to_string(), members)
            }
            Rvalue::Try { value, .. } => {
                let operand = self.operand(state, value, None);
                match &operand {
                    Val::Enum(name, tag, payloads) => match self.cx.variant_index(name, "Ok") {
                        Some(ok) => {
                            state.pc = and(&state.pc, &format!("(= {tag} {ok})"));
                            payloads
                                .get(ok)
                                .and_then(|items| items.first())
                                .cloned()
                                .unwrap_or(Val::Opaque)
                        }
                        None => fresh_target(self),
                    },
                    _ => fresh_target(self),
                }
            }
            Rvalue::StartTask { function, args, .. } => {
                // A child runs its target on arguments moved or copied into
                // it, so the start proves the target's `requires` on them,
                // and its result has the target's `ensures`.
                let started = match self.followed_operand(state, function, None) {
                    Val::Func(name) if self.cx.function(&name).is_some() => Some(name),
                    _ => None,
                };
                let Some(name) = started else {
                    self.havoc_writebacks(state, args);
                    return fresh_target(self);
                };
                let mut actuals = Vec::new();
                for arg in args {
                    actuals.push((
                        arg.name.clone(),
                        self.followed_operand(state, &arg.value, None),
                        None,
                    ));
                }
                let result_type = match &target_type {
                    Some(Type::Named(task, items)) if task == "Task" && items.len() == 1 => {
                        Some(items[0].clone())
                    }
                    _ => None,
                };
                let result =
                    self.user_call(state, &name, None, actuals, result_type.as_ref(), target);
                Val::Task(Box::new(result))
            }
            _ => fresh_target(self),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn binary(
        &mut self,
        state: &mut State,
        op: BinaryOp,
        left: &Operand,
        right: &Operand,
        span: Span,
        trap: ArithmeticTrap,
        target_type: Option<&Type>,
    ) -> Val {
        let left_ty = self.operand_int_ty(left);
        let right_ty = self.operand_int_ty(right);
        let target_int = target_type.and_then(IntTy::of);
        let hint = left_ty.or(right_ty).or(target_int);
        let lhs = self.operand(state, left, hint);
        let rhs = self.operand(state, right, hint);
        let fresh = |exec: &mut Self| match target_type {
            Some(ty) => exec.fresh(ty, "binary"),
            None => Val::Opaque,
        };
        let pc = state.pc.clone();
        match (&lhs, &rhs) {
            (Val::Int(a, aty), Val::Int(b, _)) => {
                let ty = target_int.unwrap_or(*aty);
                let compare = |op: &str| Val::Bool(format!("({op} {a} {b})"));
                match op {
                    BinaryOp::Less => compare("<"),
                    BinaryOp::LessEq => compare("<="),
                    BinaryOp::Greater => compare(">"),
                    BinaryOp::GreaterEq => compare(">="),
                    BinaryOp::Eq => compare("="),
                    BinaryOp::NotEq => Val::Bool(format!("(not (= {a} {b}))")),
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul => {
                        let symbol = match op {
                            BinaryOp::Add => "+",
                            BinaryOp::Sub => "-",
                            _ => "*",
                        };
                        let result = self.define("Int", "arith", format!("({symbol} {a} {b})"));
                        if trap.checks_overflow() {
                            self.oblige_range_at(&pc, span, &result, ty);
                        }
                        Val::Int(result, ty)
                    }
                    BinaryOp::FloorDiv | BinaryOp::Mod => {
                        if trap.checks_divisor() {
                            self.oblige(
                                ObligationKind::DivisionByZero,
                                span,
                                "divisor is not zero",
                                &pc,
                                Goal::Holds(format!("(not (= {b} 0))")),
                            );
                        }
                        let term = super::spec::division_term(op, a, b);
                        // A zero divisor stops the program before the
                        // value exists.
                        let guarded = format!("(ite (= {b} 0) 0 {term})");
                        let result = self.define("Int", "div", guarded);
                        if trap.checks_overflow() {
                            let guard = and(&pc, &format!("(not (= {b} 0))"));
                            self.oblige_range_at(&guard, span, &result, ty);
                        }
                        Val::Int(result, ty)
                    }
                    BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor => fresh(self),
                    BinaryOp::Pow if b.parse::<u32>().is_ok_and(|power| power <= 4) => {
                        let power: u32 = b.parse().expect("matched above");
                        let term = match power {
                            0 => "1".to_string(),
                            1 => a.clone(),
                            _ => format!("(* {})", vec![a.as_str(); power as usize].join(" ")),
                        };
                        let result = self.define("Int", "pow", term);
                        self.oblige_range_at(&pc, span, &result, ty);
                        Val::Int(result, ty)
                    }
                    _ => {
                        if trap == ArithmeticTrap::Runtime {
                            self.oblige(
                                ObligationKind::Overflow,
                                span,
                                "operation outside the verified subset",
                                &pc,
                                Goal::Unsupported(format!("`{op:?}` is not modeled yet")),
                            );
                        }
                        fresh(self)
                    }
                }
            }
            (Val::Bool(a), Val::Bool(b)) => match op {
                BinaryOp::Eq => Val::Bool(format!("(= {a} {b})")),
                BinaryOp::NotEq => Val::Bool(format!("(not (= {a} {b}))")),
                _ => fresh(self),
            },
            (Val::Float(..), Val::Float(..)) => {
                let (Val::Float(a, aty), Val::Float(b, bty)) = (&lhs, &rhs) else {
                    unreachable!("matched above")
                };
                // A `float64` literal beside a `float32` rounds to it.
                let float = if *aty == FloatTy::F32 || *bty == FloatTy::F32 {
                    FloatTy::F32
                } else {
                    FloatTy::F64
                };
                let a = float.convert(a, *aty);
                let b = float.convert(b, *bty);
                let compare = |op: &str| Val::Bool(format!("({op} {a} {b})"));
                match op {
                    BinaryOp::Less => compare("fp.lt"),
                    BinaryOp::LessEq => compare("fp.leq"),
                    BinaryOp::Greater => compare("fp.gt"),
                    BinaryOp::GreaterEq => compare("fp.geq"),
                    BinaryOp::Eq => compare("fp.eq"),
                    BinaryOp::NotEq => Val::Bool(format!("(not (fp.eq {a} {b}))")),
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                        let symbol = match op {
                            BinaryOp::Add => "fp.add",
                            BinaryOp::Sub => "fp.sub",
                            BinaryOp::Mul => "fp.mul",
                            _ => "fp.div",
                        };
                        if op == BinaryOp::Div {
                            self.oblige_float_divisor(&pc, span, &b, trap);
                        }
                        let result =
                            self.define(float.sort(), "arith", format!("({symbol} RNE {a} {b})"));
                        Val::Float(result, float)
                    }
                    BinaryOp::FloorDiv | BinaryOp::Mod => {
                        self.oblige_float_divisor(&pc, span, &b, trap);
                        fresh(self)
                    }
                    _ => {
                        self.oblige(
                            ObligationKind::Overflow,
                            span,
                            "numeric result in range",
                            &pc,
                            Goal::Unsupported(format!(
                                "floating-point `{op:?}` is not modeled yet"
                            )),
                        );
                        fresh(self)
                    }
                }
            }
            (Val::Str(a), Val::Str(b)) => match op {
                BinaryOp::Add => {
                    Val::Str(self.define("String", "concat", format!("(str.++ {a} {b})")))
                }
                BinaryOp::Eq => Val::Bool(format!("(= {a} {b})")),
                BinaryOp::NotEq => Val::Bool(format!("(not (= {a} {b}))")),
                BinaryOp::Less => Val::Bool(format!("(str.< {a} {b})")),
                BinaryOp::LessEq => Val::Bool(format!("(str.<= {a} {b})")),
                BinaryOp::Greater => Val::Bool(format!("(str.< {b} {a})")),
                BinaryOp::GreaterEq => Val::Bool(format!("(str.<= {b} {a})")),
                _ => fresh(self),
            },
            _ => {
                if matches!(op, BinaryOp::Eq | BinaryOp::NotEq) {
                    if let Some(equal) = structural_eq(&lhs, &rhs) {
                        return Val::Bool(if op == BinaryOp::Eq {
                            equal
                        } else {
                            not(&equal)
                        });
                    }
                }
                let numeric = |ty: Option<Type>| {
                    ty.as_ref().is_some_and(|ty| {
                        IntTy::of(ty).is_some()
                            || matches!(ty, Type::Named(name, args) if args.is_empty() && (name == "float64" || name == "float32"))
                    })
                };
                let left_numeric = numeric(self.operand_type(left));
                let divides = matches!(op, BinaryOp::Div | BinaryOp::FloorDiv | BinaryOp::Mod);
                if left_numeric && (trap.checks_divisor() || trap == ArithmeticTrap::Runtime) {
                    let (kind, what) = if divides {
                        (ObligationKind::DivisionByZero, "divisor is not zero")
                    } else {
                        (ObligationKind::Overflow, "numeric result in range")
                    };
                    self.oblige(
                        kind,
                        span,
                        what,
                        &pc,
                        Goal::Unsupported("floating-point operands are not modeled yet".into()),
                    );
                }
                fresh(self)
            }
        }
    }

    fn oblige_range(&mut self, state: &State, span: Span, term: &str, ty: IntTy) {
        let pc = state.pc.clone();
        self.oblige_range_at(&pc, span, term, ty);
    }

    fn oblige_range_at(&mut self, pc: &str, span: Span, term: &str, ty: IntTy) {
        self.oblige(
            ObligationKind::Overflow,
            span,
            "integer result in range",
            pc,
            Goal::Holds(ty.contains(term)),
        );
    }

    // ----- Calls -----------------------------------------------------------

    fn havoc_writebacks(&mut self, state: &mut State, args: &[MirArg]) {
        for arg in args {
            if let Some(place) = &arg.writeback_place {
                self.havoc_place(state, place);
            }
        }
    }

    fn call(
        &mut self,
        state: &mut State,
        target: Option<&str>,
        callee: &CallTarget,
        args: &[MirArg],
    ) -> Val {
        let target_type = target.and_then(|target| self.place_type(target));
        let fresh_target = |exec: &mut Self| match &target_type {
            Some(ty) => exec.fresh(ty, target.unwrap_or("call")),
            None => Val::Opaque,
        };
        match callee {
            CallTarget::Runtime {
                op,
                receiver,
                receiver_place,
            } => {
                let receiver_value = receiver
                    .as_ref()
                    .map(|receiver| self.operand(state, receiver, None));
                let result = self.runtime_call(
                    state,
                    *op,
                    receiver_value,
                    receiver_place.as_deref(),
                    args,
                    target,
                );
                // An operation the encoding does not model may change its
                // receiver in any way.
                if result.is_none() && !runtime_op_is_pure(*op) {
                    if let Some(place) = receiver_place {
                        self.havoc_place(state, place);
                    }
                }
                self.havoc_writebacks(state, args);
                result.unwrap_or_else(|| fresh_target(self))
            }
            CallTarget::Name(name) if self.cx.spec_def(name).is_some() => {
                let mut values = Vec::new();
                for arg in args {
                    values.push(self.operand(state, &arg.value, None));
                }
                let spec = self.cx.spec_def(name).expect("checked above");
                self.spec_call_value(state, spec, values, name)
                    .unwrap_or_else(|| fresh_target(self))
            }
            CallTarget::Member {
                object,
                dispatch: Some(MethodDispatch::Static { function }),
                ..
            } if self.cx.spec_def(function).is_some() => {
                let mut values = vec![self.operand(state, object, None)];
                for arg in args {
                    values.push(self.operand(state, &arg.value, None));
                }
                let spec = self.cx.spec_def(function).expect("checked above");
                self.spec_call_value(state, spec, values, function)
                    .unwrap_or_else(|| fresh_target(self))
            }
            CallTarget::Name(name) if args.is_empty() && self.default_value(name).is_some() => {
                self.default_value(name).expect("checked above")
            }
            CallTarget::Name(name) => {
                if self.cx.function(name).is_some() {
                    let mut actuals = Vec::new();
                    for arg in args {
                        actuals.push((
                            arg.name.clone(),
                            self.followed_operand(state, &arg.value, None),
                            arg.writeback_place.clone(),
                        ));
                    }
                    self.user_call(state, name, None, actuals, target_type.as_ref(), target)
                } else {
                    self.havoc_writebacks(state, args);
                    fresh_target(self)
                }
            }
            CallTarget::Member {
                object,
                receiver_place,
                dispatch: Some(MethodDispatch::Static { function }),
                ..
            }
            | CallTarget::TraitMember {
                object,
                receiver_place,
                dispatch: Some(MethodDispatch::Static { function }),
                ..
            } if self.cx.function(function).is_some() => {
                let receiver = self.operand(state, object, None);
                let mut actuals = Vec::new();
                for arg in args {
                    actuals.push((
                        arg.name.clone(),
                        self.followed_operand(state, &arg.value, None),
                        arg.writeback_place.clone(),
                    ));
                }
                self.user_call(
                    state,
                    function,
                    Some((receiver, receiver_place.clone())),
                    actuals,
                    target_type.as_ref(),
                    target,
                )
            }
            // A call through a trait picks its implementation at run time.
            // Every implementation has the trait method's contract, so the
            // call relies on that.
            CallTarget::TraitMember {
                object,
                receiver_place,
                trait_name,
                field,
                dispatch: Some(MethodDispatch::Candidates { candidates, .. }),
            } if self
                .trait_call_target(candidates, trait_name, field)
                .is_some() =>
            {
                let (function, contract) = self
                    .trait_call_target(candidates, trait_name, field)
                    .expect("checked above");
                let receiver = self.operand(state, object, None);
                let mut actuals = Vec::new();
                for arg in args {
                    actuals.push((
                        arg.name.clone(),
                        self.followed_operand(state, &arg.value, None),
                        arg.writeback_place.clone(),
                    ));
                }
                self.contract_override = contract;
                let result = self.user_call(
                    state,
                    &function,
                    Some((receiver, receiver_place.clone())),
                    actuals,
                    target_type.as_ref(),
                    target,
                );
                self.contract_override = None;
                result
            }
            CallTarget::Member { receiver_place, .. }
            | CallTarget::TraitMember { receiver_place, .. } => {
                if let Some(place) = receiver_place {
                    self.havoc_place(state, place);
                }
                self.havoc_writebacks(state, args);
                fresh_target(self)
            }
            CallTarget::Value(callee) => match self.followed_operand(state, callee, None) {
                Val::Contracted(alias) => {
                    let info = self.cx.alias_contract(&alias).expect("a contracted alias");
                    let mut pre = Vec::new();
                    for (param, arg) in info.decl.params.iter().zip(args) {
                        let value = self.operand(state, &arg.value, None);
                        pre.push((param.name.clone(), value));
                    }
                    let span = self.span;
                    self.contract_override = Some(&info.decl);
                    self.call_requires(state, &alias, &pre, span);
                    let result = match target_type.as_ref().unwrap_or(&info.signature.return_type) {
                        Type::Unit => Val::Unit,
                        ty => self.fresh(ty, target.unwrap_or("result")),
                    };
                    self.call_ensures(state, &alias, &pre, &pre, &result);
                    self.contract_override = None;
                    result
                }
                Val::Func(name) if self.cx.function(&name).is_some() => {
                    let mut actuals = Vec::new();
                    for arg in args {
                        actuals.push((
                            arg.name.clone(),
                            self.followed_operand(state, &arg.value, None),
                            arg.writeback_place.clone(),
                        ));
                    }
                    self.user_call(state, &name, None, actuals, target_type.as_ref(), target)
                }
                _ => {
                    self.havoc_writebacks(state, args);
                    fresh_target(self)
                }
            },
            CallTarget::Extern(call) if self.cx.decl(&call.symbol).is_some() => {
                // A trusted contract: the call proves the `requires` and
                // the result and `mut` arguments have the `ensures`.
                let mut pre = Vec::new();
                let mut post = Vec::new();
                for (param, arg) in call.params.iter().zip(args) {
                    let value = fit(
                        self.operand(state, &arg.value, IntTy::of(&param.ty)),
                        &param.ty,
                    );
                    pre.push((param.name.clone(), value.clone()));
                    let after = match (&arg.writeback_place, param.passing) {
                        (Some(place), MirReceiverKind::BorrowMut) => {
                            let after = self.fresh(&param.ty, &param.name);
                            self.write_place(state, place, after.clone());
                            after
                        }
                        (Some(place), _) => {
                            self.havoc_place(state, place);
                            value
                        }
                        _ => value,
                    };
                    post.push((param.name.clone(), after));
                }
                let span = self.span;
                self.call_requires(state, &call.symbol, &pre, span);
                let result = match target_type.as_ref().unwrap_or(&call.return_type) {
                    Type::Unit => Val::Unit,
                    ty => self.fresh(ty, target.unwrap_or("result")),
                };
                self.call_ensures(state, &call.symbol, &pre, &post, &result);
                result
            }
            CallTarget::Extern(_) => {
                self.havoc_writebacks(state, args);
                fresh_target(self)
            }
        }
    }

    /// The function a call through a trait is modeled by, and the contract
    /// it relies on: the trait method's, which every implementation keeps,
    /// or else the one contract all the candidates share.
    fn trait_call_target(
        &self,
        candidates: &[crate::mir::MethodCandidate],
        trait_name: &str,
        field: &str,
    ) -> Option<(String, Option<&'a crate::ast::FunctionDecl>)> {
        if let Some(contract) = self.cx.trait_method(trait_name, field) {
            let first = candidates.first()?;
            if candidates
                .iter()
                .all(|candidate| self.cx.function(&candidate.function).is_some())
            {
                return Some((first.function.clone(), Some(contract)));
            }
        }
        Some((self.shared_contract(candidates)?, None))
    }

    /// The first of `candidates` when every one is a function with the same
    /// contract, as the implementations of one trait method are.
    fn shared_contract(&self, candidates: &[crate::mir::MethodCandidate]) -> Option<String> {
        let first = candidates.first()?;
        let contract = |name: &str| {
            self.cx.function(name)?;
            let decl = self.cx.decl(name)?;
            Some(
                decl.contracts
                    .iter()
                    .map(|clause| (clause.kind, clause.text.clone()))
                    .collect::<Vec<_>>(),
            )
        };
        let expected = contract(&first.function)?;
        candidates
            .iter()
            .all(|candidate| contract(&candidate.function).as_ref() == Some(&expected))
            .then_some(first.function.clone())
    }

    /// The value of the hidden function that computes a parameter's
    /// default, `f::__default_<index>_<param>`, read from the default as
    /// written when the contract model reads it.
    fn default_value(&mut self, name: &str) -> Option<Val> {
        let (function, rest) = name.split_once("::__default_")?;
        let (_, param) = rest.split_once('_')?;
        let decl = self.cx.decl(function)?;
        let default = decl
            .params
            .iter()
            .find(|p| p.name == param)?
            .default
            .as_ref()?;
        let mut wd = Vec::new();
        let value = self.spec(default, &Default::default(), &mut wd, 0).ok()?;
        wd.is_empty().then_some(value)
    }

    /// The value a call of a `spec def` returns, unfolded from its body,
    /// with the check that the call is well-defined there.
    fn spec_call_value(
        &mut self,
        state: &State,
        spec: &'a crate::ast::FunctionDecl,
        values: Vec<Val>,
        name: &str,
    ) -> Option<Val> {
        let mut wd = Vec::new();
        let value = self.spec_call(spec, values, &mut wd, 0).ok()?;
        let pc = state.pc.clone();
        let span = self.span;
        let wd = conjoin(&wd);
        if wd != "true" {
            self.oblige(
                ObligationKind::Assert,
                span,
                format!(
                    "`spec def {}` is well-defined",
                    crate::mir::public_function_name(name)
                ),
                &pc,
                Goal::Holds(wd),
            );
        }
        Some(value)
    }

    /// A call of a user function: prove its `requires`, then assume its
    /// `ensures` of a fresh result and fresh values for what it may
    /// change.
    fn user_call(
        &mut self,
        state: &mut State,
        name: &str,
        receiver: Option<(Val, Option<String>)>,
        actuals: Vec<(Option<String>, Val, Option<String>)>,
        result_type: Option<&Type>,
        hint: Option<&str>,
    ) -> Val {
        let callee = self.cx.function(name).expect("checked callee");
        let mut pre: Vec<(String, Val)> = Vec::new();
        let mut post_places: Vec<(String, String)> = Vec::new();
        if let Some((value, place)) = receiver {
            if let (Some(place), Some(MirReceiverKind::BorrowMut)) = (place, callee.receiver) {
                post_places.push(("self".to_string(), place));
            }
            pre.push(("self".to_string(), value));
        }
        let rest: Vec<_> = callee.params.iter().collect();
        let mut positional = 0usize;
        for (arg_name, value, place) in actuals {
            let param = match &arg_name {
                Some(arg_name) => rest.iter().find(|param| &param.name == arg_name),
                None => {
                    let param = rest.get(positional);
                    positional += 1;
                    param
                }
            };
            let Some(param) = param else {
                continue;
            };
            if let (Some(place), MirReceiverKind::BorrowMut) = (place, param.passing) {
                post_places.push((param.name.clone(), place));
            }
            self.pass_function_value(state, name, &param.name, &value);
            pre.push((param.name.clone(), fit(value, &param.ty)));
        }
        let span = self.span;
        self.erasing = self.cx.decl(name).is_some_and(|decl| decl.lemma);
        self.call_requires(state, name, &pre, span);
        self.erasing = false;
        let result = match result_type {
            Some(ty) => self.fresh(ty, hint.unwrap_or("result")),
            None => match &callee.return_type {
                Type::Unit => Val::Unit,
                ty => self.fresh(ty, "result"),
            },
        };
        let mut post = pre.clone();
        for (param, place) in &post_places {
            let ty = callee
                .params
                .iter()
                .find(|candidate| &candidate.name == param)
                .map(|candidate| candidate.ty.clone())
                .or_else(|| {
                    callee
                        .local_types
                        .iter()
                        .find(|local| &local.name == param)
                        .map(|local| local.ty.clone())
                });
            let value = match ty {
                Some(ty) => self.fresh(&ty, param),
                None => Val::Opaque,
            };
            if let Some(slot) = post.iter_mut().find(|(name, _)| name == param) {
                slot.1 = value.clone();
            }
            self.write_place(state, place, value);
        }
        self.call_ensures(state, name, &pre, &post, &result);
        result
    }

    fn runtime_call(
        &mut self,
        state: &mut State,
        op: RuntimeOp,
        receiver: Option<Val>,
        receiver_place: Option<&str>,
        args: &[MirArg],
        target: Option<&str>,
    ) -> Option<Val> {
        let arg = |exec: &mut Self, state: &mut State, index: usize, hint: Option<IntTy>| {
            args.get(index)
                .map(|arg| exec.operand(state, &arg.value, hint))
                .unwrap_or(Val::Opaque)
        };
        let span_of = |exec: &Self, first: usize| -> Span {
            let line = args.get(first).and_then(|arg| match arg.value {
                Operand::Int(value) => Some(value as usize),
                _ => None,
            });
            let column = args.get(first + 1).and_then(|arg| match arg.value {
                Operand::Int(value) => Some(value as usize),
                _ => None,
            });
            match (line, column) {
                (Some(line), Some(column)) => Span { line, column },
                _ => exec.span,
            }
        };
        let pc = state.pc.clone();
        match op {
            RuntimeOp::List(list_op) => {
                let Some(Val::List { len, data, elem }) = receiver else {
                    return None;
                };
                let elem_int = IntTy::of(&elem);
                match list_op {
                    ListOp::Len => Some(Val::Int(len, IntTy::INT64)),
                    ListOp::IsEmpty => Some(Val::Bool(format!("(= {len} 0)"))),
                    ListOp::Index
                    | ListOp::IndexScalar(_)
                    | ListOp::IndexUnchecked
                    | ListOp::IndexScalarUnchecked(_) => {
                        let Val::Int(index, _) = arg(self, state, 0, Some(IntTy::INT64)) else {
                            return None;
                        };
                        if matches!(list_op, ListOp::Index | ListOp::IndexScalar(_)) {
                            let span = span_of(self, 1);
                            self.oblige_index(state, span, &len, &index);
                            self.oblige_site(state, span, &len, &index);
                        }
                        let index = self.normalize_index(&len, &index);
                        let list = Val::List { len, data, elem };
                        Some(self.element(&list, &index, target.unwrap_or("element")))
                    }
                    ListOp::IndexOption | ListOp::TakeIndexOption => {
                        // `Found(element)` when the index is in range,
                        // else `Missing`; no index counts from the end.
                        let Val::Int(index, _) = arg(self, state, 0, Some(IntTy::INT64)) else {
                            return None;
                        };
                        let list = Val::List {
                            len: len.clone(),
                            data,
                            elem,
                        };
                        let element = self.element(&list, &index, target.unwrap_or("element"));
                        let tag = self.define(
                            "Int",
                            "lookup",
                            format!("(ite (and (<= 0 {index}) (< {index} {len})) 0 1)"),
                        );
                        if list_op == ListOp::TakeIndexOption {
                            if let Some(place) = receiver_place {
                                self.havoc_place(state, place);
                            }
                        }
                        Some(Val::Enum(
                            super::LOOKUP.to_string(),
                            tag,
                            vec![vec![element], Vec::new()],
                        ))
                    }
                    ListOp::SetIndex | ListOp::SetIndexScalar(_) => {
                        let Val::Int(index, _) = arg(self, state, 0, Some(IntTy::INT64)) else {
                            return None;
                        };
                        let value = arg(self, state, 1, elem_int);
                        let span = span_of(self, 2);
                        self.oblige_index(state, span, &len, &index);
                        let index = self.normalize_index(&len, &index);
                        let data = match data {
                            Some(data) => self.put_or_fresh(&data, &elem, &index, &value, "store"),
                            None => None,
                        };
                        if let Some(place) = receiver_place {
                            self.write_place(state, place, Val::List { len, data, elem });
                        }
                        Some(Val::Unit)
                    }
                    ListOp::Push | ListOp::PushScalar(_) => {
                        let value = arg(self, state, 0, elem_int);
                        let new_len = self.define("Int", "len", format!("(+ {len} 1)"));
                        let data = match data {
                            Some(data) => self.put_or_fresh(&data, &elem, &len, &value, "push"),
                            None => None,
                        };
                        // A list longer than `int64` can count stops the
                        // program before the push returns.
                        self.assume(&pc, format!("(<= {new_len} {})", i64::MAX));
                        if let Some(place) = receiver_place {
                            self.write_place(
                                state,
                                place,
                                Val::List {
                                    len: new_len,
                                    data,
                                    elem,
                                },
                            );
                        }
                        Some(Val::Unit)
                    }
                    ListOp::Pop => {
                        self.oblige(
                            ObligationKind::IndexInRange,
                            self.span,
                            "pop from a list that is not empty",
                            &pc,
                            Goal::Holds(format!("(> {len} 0)")),
                        );
                        self.assume(&pc, format!("(> {len} 0)"));
                        let new_len = self.define("Int", "len", format!("(- {len} 1)"));
                        let list = Val::List {
                            len: len.clone(),
                            data: data.clone(),
                            elem: elem.clone(),
                        };
                        let value = self.element(&list, &new_len, "popped");
                        if let Some(place) = receiver_place {
                            self.write_place(
                                state,
                                place,
                                Val::List {
                                    len: new_len,
                                    data,
                                    elem,
                                },
                            );
                        }
                        // `pop` returns the element itself.
                        Some(value)
                    }
                    ListOp::Clear => {
                        if let Some(place) = receiver_place {
                            self.write_place(
                                state,
                                place,
                                Val::List {
                                    len: "0".to_string(),
                                    data,
                                    elem,
                                },
                            );
                        }
                        Some(Val::Unit)
                    }
                    ListOp::Reverse | ListOp::Sort | ListOp::Swap => {
                        let data = self.fresh_store(&elem, "permuted");
                        if let Some(place) = receiver_place {
                            self.write_place(state, place, Val::List { len, data, elem });
                        }
                        Some(Val::Unit)
                    }
                    _ => None,
                }
            }
            RuntimeOp::Str(str_op) => {
                let Some(Val::Str(text)) = receiver else {
                    return None;
                };
                let other = |exec: &mut Self, state: &mut State| match arg(exec, state, 0, None) {
                    Val::Str(other) => Some(other),
                    _ => None,
                };
                match str_op {
                    StrOp::Len => {
                        let len = self.define("Int", "len", format!("(str.len {text})"));
                        self.fact(format!("(<= {len} {})", i64::MAX));
                        Some(Val::Int(len, IntTy::INT64))
                    }
                    StrOp::Contains => {
                        let other = other(self, state)?;
                        Some(Val::Bool(format!("(str.contains {text} {other})")))
                    }
                    StrOp::StartsWith => {
                        let other = other(self, state)?;
                        Some(Val::Bool(format!("(str.prefixof {other} {text})")))
                    }
                    StrOp::EndsWith => {
                        let other = other(self, state)?;
                        Some(Val::Bool(format!("(str.suffixof {other} {text})")))
                    }
                    _ => None,
                }
            }
            RuntimeOp::Dict(dict_op) => {
                let Some(Val::Map {
                    len,
                    keys,
                    values,
                    key,
                    value,
                }) = receiver
                else {
                    return None;
                };
                let sort = key_sort(&key).expect("a modeled dict has a modeled key");
                let key_arg = |exec: &mut Self, state: &mut State| {
                    key_term(&arg(exec, state, 0, IntTy::of(&key)))
                };
                match dict_op {
                    DictOp::Len => Some(Val::Int(len, IntTy::INT64)),
                    DictOp::IsEmpty => Some(Val::Bool(format!("(= {len} 0)"))),
                    DictOp::ContainsKey | DictOp::ContainsKeyInt64 => {
                        let k = key_arg(self, state)?;
                        Some(Val::Bool(format!("(select {keys} {k})")))
                    }
                    DictOp::Index => {
                        let k = key_arg(self, state)?;
                        let span = span_of(self, 1);
                        let present = format!("(select {keys} {k})");
                        self.oblige(
                            ObligationKind::KeyPresent,
                            span,
                            "key is in the dict",
                            &pc,
                            Goal::Holds(present.clone()),
                        );
                        self.assume(&pc, present);
                        let hint = target.unwrap_or("value");
                        Some(match &values {
                            Some(values) => {
                                let read = scalar_val(format!("(select {values} {k})"), &value);
                                self.name_scalars(read, hint)
                            }
                            None => self.fresh(&value, hint),
                        })
                    }
                    DictOp::SetIndex | DictOp::Set => {
                        let k = key_arg(self, state)?;
                        let stored = arg(self, state, 1, IntTy::of(&value));
                        let new_len = self.len_after_add(&len, &keys, &k);
                        let new_keys = self.define(
                            &format!("(Array {sort} Bool)"),
                            "keys",
                            format!("(store {keys} {k} true)"),
                        );
                        let new_values = match (&values, scalar_term(&stored), element_sort(&value))
                        {
                            (Some(values), Some(term), Some(value_sort)) => Some(self.define(
                                &format!("(Array {sort} {value_sort})"),
                                "values",
                                format!("(store {values} {k} {term})"),
                            )),
                            _ => None,
                        };
                        if let Some(place) = receiver_place {
                            self.write_place(
                                state,
                                place,
                                Val::Map {
                                    len: new_len,
                                    keys: new_keys,
                                    values: new_values,
                                    key,
                                    value,
                                },
                            );
                        }
                        Some(Val::Unit)
                    }
                    DictOp::Keys | DictOp::Values | DictOp::Items => {
                        // A list of `len` entries, each holding a key of the
                        // dict and, for values and items, its value there.
                        let entries = self.fresh_const(&format!("(Array Int {sort})"), "keys");
                        let i = self.bound_name();
                        let at = format!("(select {entries} {i})");
                        let mut holds = vec![format!("(select {keys} {at})")];
                        let value_store = match (&values, element_sort(&value)) {
                            (Some(values), Some(value_sort)) => {
                                let store = self
                                    .fresh_const(&format!("(Array Int {value_sort})"), "values");
                                holds.push(format!(
                                    "(= (select {store} {i}) (select {values} {at}))"
                                ));
                                Some(Store::Leaf(store, value_sort))
                            }
                            _ => None,
                        };
                        self.fact(format!(
                            "(forall (({i} Int)) (=> (and (<= 0 {i}) (< {i} {len})) {}))",
                            conjoin(&holds)
                        ));
                        let key_store = Store::Leaf(entries, sort);
                        let (data, elem) = match dict_op {
                            DictOp::Keys => (Some(key_store), key),
                            DictOp::Values => (value_store, value),
                            _ => (
                                value_store.map(|values| Store::Items(vec![key_store, values])),
                                Type::Tuple(vec![key, value]),
                            ),
                        };
                        Some(Val::List { len, data, elem })
                    }
                    DictOp::Clear => {
                        if let Some(place) = receiver_place {
                            self.write_place(
                                state,
                                place,
                                Val::Map {
                                    len: "0".to_string(),
                                    keys: no_keys(sort),
                                    values,
                                    key,
                                    value,
                                },
                            );
                        }
                        Some(Val::Unit)
                    }
                    _ => None,
                }
            }
            RuntimeOp::Set(set_op) => {
                let Some(Val::Set { len, members, elem }) = receiver else {
                    return None;
                };
                let sort = key_sort(&elem).expect("a modeled set has a modeled element");
                let member = |exec: &mut Self, state: &mut State| {
                    key_term(&arg(exec, state, 0, IntTy::of(&elem)))
                };
                let array = format!("(Array {sort} Bool)");
                match set_op {
                    SetOp::Len => Some(Val::Int(len, IntTy::INT64)),
                    SetOp::IsEmpty => Some(Val::Bool(format!("(= {len} 0)"))),
                    SetOp::Contains | SetOp::ContainsInt64 => {
                        let k = member(self, state)?;
                        Some(Val::Bool(format!("(select {members} {k})")))
                    }
                    SetOp::Add | SetOp::AddInt64 | SetOp::Remove | SetOp::Discard => {
                        let k = member(self, state)?;
                        let present = format!("(select {members} {k})");
                        let (len, members) = if matches!(set_op, SetOp::Add | SetOp::AddInt64) {
                            let len = self.len_after_add(&len, &members, &k);
                            let members = self.define(
                                &array,
                                "members",
                                format!("(store {members} {k} true)"),
                            );
                            (len, members)
                        } else {
                            if set_op == SetOp::Remove {
                                let span = self.span;
                                self.oblige(
                                    ObligationKind::KeyPresent,
                                    span,
                                    "element is in the set",
                                    &pc,
                                    Goal::Holds(present.clone()),
                                );
                                self.assume(&pc, present.clone());
                            }
                            let len = self.define(
                                "Int",
                                "len",
                                format!("(ite {present} (- {len} 1) {len})"),
                            );
                            let members = self.define(
                                &array,
                                "members",
                                format!("(store {members} {k} false)"),
                            );
                            (len, members)
                        };
                        if let Some(place) = receiver_place {
                            self.write_place(state, place, Val::Set { len, members, elem });
                        }
                        Some(Val::Unit)
                    }
                    SetOp::IndexOption | SetOp::TakeIndexOption => {
                        // Iteration reads the elements by position: each
                        // position below `len` holds one of them.
                        let Val::Int(index, _) = arg(self, state, 0, Some(IntTy::INT64)) else {
                            return None;
                        };
                        let element = self.fresh(&elem, target.unwrap_or("element"));
                        let in_range = format!("(and (<= 0 {index}) (< {index} {len}))");
                        if let Some(k) = key_term(&element) {
                            self.fact(format!("(=> {in_range} (select {members} {k}))"));
                        }
                        let tag = self.define("Int", "lookup", format!("(ite {in_range} 0 1)"));
                        if set_op == SetOp::TakeIndexOption {
                            if let Some(place) = receiver_place {
                                self.havoc_place(state, place);
                            }
                        }
                        Some(Val::Enum(
                            super::LOOKUP.to_string(),
                            tag,
                            vec![vec![element], Vec::new()],
                        ))
                    }
                    SetOp::Clear => {
                        if let Some(place) = receiver_place {
                            self.write_place(
                                state,
                                place,
                                Val::Set {
                                    len: "0".to_string(),
                                    members: no_keys(sort),
                                    elem,
                                },
                            );
                        }
                        Some(Val::Unit)
                    }
                    _ => None,
                }
            }
            RuntimeOp::Function(
                function @ (FunctionOp::Abs | FunctionOp::Min | FunctionOp::Max),
            ) => {
                let target_int = target
                    .and_then(|target| self.place_type(target))
                    .as_ref()
                    .and_then(IntTy::of);
                let values: Vec<Val> = (0..args.len())
                    .map(|index| arg(self, state, index, target_int))
                    .collect();
                match (function, values.as_slice()) {
                    (FunctionOp::Abs, [Val::Float(term, float)]) => Some(Val::Float(
                        self.define(float.sort(), "abs", format!("(fp.abs {term})")),
                        *float,
                    )),
                    (FunctionOp::Abs, [Val::Int(term, ty)]) => {
                        let ty = target_int.unwrap_or(*ty);
                        let result = self.define(
                            "Int",
                            "abs",
                            format!("(ite (< {term} 0) (- {term}) {term})"),
                        );
                        let span = self.span;
                        self.oblige_range_at(&pc, span, &result, ty);
                        Some(Val::Int(result, ty))
                    }
                    (FunctionOp::Min | FunctionOp::Max, [Val::Int(a, ty), Val::Int(b, _)]) => {
                        let op = if function == FunctionOp::Min {
                            "<="
                        } else {
                            ">="
                        };
                        let result =
                            self.define("Int", "minmax", format!("(ite ({op} {a} {b}) {a} {b})"));
                        Some(Val::Int(result, target_int.unwrap_or(*ty)))
                    }
                    _ => None,
                }
            }
            RuntimeOp::Task(task_op) => {
                let Some(Val::Task(value)) = receiver else {
                    return None;
                };
                let result_type = target.and_then(|target| self.place_type(target))?;
                match task_op {
                    TaskOp::Result | TaskOp::ResultTimeout | TaskOp::Poll | TaskOp::PollTimeout => {
                        // A `Ready` outcome carries the task's value.
                        let Val::Enum(name, tag, mut payloads) =
                            self.fresh(&result_type, target.unwrap_or("outcome"))
                        else {
                            return None;
                        };
                        let Type::Named(_, type_args) = &result_type else {
                            return None;
                        };
                        let ready = self
                            .cx
                            .enum_variants(&name, type_args)?
                            .iter()
                            .position(|(variant, _)| variant == "Ready")?;
                        payloads[ready] = vec![*value];
                        Some(Val::Enum(name, tag, payloads))
                    }
                    TaskOp::ResultOr | TaskOp::ResultOrTimeout => {
                        let default =
                            fit(arg(self, state, 0, IntTy::of(&result_type)), &result_type);
                        let ready = self.fresh_const("Bool", "ready");
                        let pcs = [ready.clone(), not(&ready)];
                        Some(self.merge_vals(&pcs, &[&value, &default], "outcome"))
                    }
                }
            }
            RuntimeOp::Scalar(ScalarOp::ToFloat) => match receiver {
                Some(Val::Int(term, _)) => {
                    let float = FloatTy::F64;
                    Some(Val::Float(
                        self.define(float.sort(), "float", float.round_int(&term)),
                        float,
                    ))
                }
                _ => None,
            },
            RuntimeOp::Scalar(ScalarOp::Sqrt) => match receiver {
                Some(Val::Float(term, float)) => Some(Val::Float(
                    self.define(float.sort(), "sqrt", format!("(fp.sqrt RNE {term})")),
                    float,
                )),
                _ => None,
            },
            RuntimeOp::Value(ValueOp::Clone | ValueOp::Copy) => receiver,
            RuntimeOp::Print(_) => Some(Val::Unit),
            _ => None,
        }
    }
}

/// Whether a runtime op leaves its receiver as it was.
fn runtime_op_is_pure(op: RuntimeOp) -> bool {
    match op {
        RuntimeOp::List(op) => list_op_is_pure(op),
        RuntimeOp::Str(_) | RuntimeOp::Scalar(_) | RuntimeOp::Value(_) => true,
        RuntimeOp::Dict(op) => matches!(
            op,
            DictOp::Len
                | DictOp::IsEmpty
                | DictOp::Get
                | DictOp::GetStr
                | DictOp::GetInt64
                | DictOp::ContainsKey
                | DictOp::ContainsKeyInt64
                | DictOp::Keys
                | DictOp::Values
                | DictOp::Items
                | DictOp::Index
        ),
        RuntimeOp::Set(op) => matches!(
            op,
            SetOp::Len
                | SetOp::IsEmpty
                | SetOp::Contains
                | SetOp::ContainsInt64
                | SetOp::IndexOption
        ),
        _ => false,
    }
}

/// The position that names a check failure block's own check as a build
/// site: the branch into it.
pub(crate) const CHECK_FAILURE_SITE: usize = usize::MAX;

/// What a failure block's check states, for the report: the clause as
/// written, or the `assert` and its message.
pub(super) fn check_message(kind: AssertKind, message: Option<&Operand>) -> String {
    let text = match message {
        Some(Operand::String(text)) => text.as_str(),
        _ => return "`assert` condition".to_string(),
    };
    if kind == AssertKind::Assert {
        return format!("`assert` condition ({text})");
    }
    for keyword in ["requires", "ensures", "invariant", "decreases"] {
        let prefix = format!("`{keyword}` clause failed: ");
        let Some(rest) = text.strip_prefix(&prefix) else {
            continue;
        };
        if let Some(measure) = rest.strip_suffix(" went below 0") {
            return format!("`decreases` measure {measure} stays at least 0");
        }
        if let Some(measure) = rest.strip_suffix(" did not get smaller in the last iteration") {
            return format!("`decreases` measure {measure} gets smaller in every iteration");
        }
        return format!("`{keyword}` clause {rest}");
    }
    text.to_string()
}

fn list_op_is_pure(op: ListOp) -> bool {
    matches!(
        op,
        ListOp::Len
            | ListOp::IsEmpty
            | ListOp::Get
            | ListOp::IndexOf
            | ListOp::Count
            | ListOp::Contains
            | ListOp::Index
            | ListOp::IndexScalar(_)
            | ListOp::IndexUnchecked
            | ListOp::IndexScalarUnchecked(_)
            | ListOp::Slice
            | ListOp::SliceStep
            | ListOp::IndexOption
    )
}

pub(super) fn structural_eq(left: &Val, right: &Val) -> Option<String> {
    match (left, right) {
        (Val::Int(a, _), Val::Int(b, _))
        | (Val::Bool(a), Val::Bool(b))
        | (Val::Str(a), Val::Str(b))
        | (Val::Set { members: a, .. }, Val::Set { members: b, .. }) => {
            Some(format!("(= {a} {b})"))
        }
        (Val::Float(a, _), Val::Float(b, _)) => Some(format!("(fp.eq {a} {b})")),
        (
            Val::Map {
                keys: a,
                values: Some(x),
                key,
                ..
            },
            Val::Map {
                keys: b,
                values: Some(y),
                ..
            },
        ) => {
            let sort = key_sort(key)?;
            Some(format!(
                "(and (= {a} {b}) (forall ((k {sort})) (=> (select {a} k) (= (select {x} k) (select {y} k)))))"
            ))
        }
        (Val::Unit, Val::Unit) => Some("true".to_string()),
        (Val::Tuple(a), Val::Tuple(b)) if a.len() == b.len() => {
            let parts: Option<Vec<String>> =
                a.iter().zip(b).map(|(a, b)| structural_eq(a, b)).collect();
            Some(conjoin(&parts?))
        }
        (Val::Record(ca, a), Val::Record(cb, b)) if ca == cb && a.len() == b.len() => {
            let parts: Option<Vec<String>> = a
                .iter()
                .zip(b)
                .map(|((_, a), (_, b))| structural_eq(a, b))
                .collect();
            Some(conjoin(&parts?))
        }
        _ => None,
    }
}

pub(super) fn conjoin(parts: &[String]) -> String {
    let parts: Vec<&String> = parts.iter().filter(|part| *part != "true").collect();
    match parts.len() {
        0 => "true".to_string(),
        1 => parts[0].clone(),
        _ => format!(
            "(and {})",
            parts
                .iter()
                .map(|part| part.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        ),
    }
}

/// The member index a `__union_payload_N` place segment names.
pub(super) fn union_payload_index(segment: &str) -> Option<usize> {
    segment
        .strip_prefix(crate::mir::UNION_PAYLOAD_PROJECTION_PREFIX)?
        .parse()
        .ok()
}

fn project(value: &Val, segment: &str) -> Option<Val> {
    match value {
        Val::Union(_, members) => members.get(union_payload_index(segment)?).cloned(),
        Val::Record(_, fields) => fields
            .iter()
            .find(|(name, _)| name == segment)
            .map(|(_, value)| value.clone()),
        Val::Tuple(items) => segment
            .parse::<usize>()
            .ok()
            .and_then(|index| items.get(index).cloned()),
        _ => None,
    }
}

fn set_in(value: &mut Val, segments: &[&str], new: Val) -> bool {
    let Some((first, rest)) = segments.split_first() else {
        *value = new;
        return true;
    };
    let slot = match value {
        Val::Union(_, members) => {
            union_payload_index(first).and_then(|index| members.get_mut(index))
        }
        Val::Record(_, fields) => fields
            .iter_mut()
            .find(|(name, _)| name == first)
            .map(|(_, value)| value),
        Val::Tuple(items) => first
            .parse::<usize>()
            .ok()
            .and_then(|index| items.get_mut(index)),
        _ => None,
    };
    match slot {
        Some(slot) => set_in(slot, rest, new),
        None => false,
    }
}

fn join_path(left: &str, right: &str) -> String {
    match (left.is_empty(), right.is_empty()) {
        (true, _) => right.to_string(),
        (_, true) => left.to_string(),
        _ => format!("{left}.{right}"),
    }
}

pub(super) fn root_of(place: &str) -> &str {
    place.split('.').next().unwrap_or(place)
}

fn is_atom(term: &str) -> bool {
    !term.contains(' ')
}

pub(super) fn and(left: &str, right: &str) -> String {
    match (left, right) {
        ("true", _) => right.to_string(),
        (_, "true") => left.to_string(),
        ("false", _) | (_, "false") => "false".to_string(),
        _ => format!("(and {left} {right})"),
    }
}

pub(super) fn not(term: &str) -> String {
    match term {
        "true" => "false".to_string(),
        "false" => "true".to_string(),
        _ => match term
            .strip_prefix("(not ")
            .and_then(|rest| rest.strip_suffix(')'))
        {
            Some(inner) if balanced(inner) => inner.to_string(),
            _ => format!("(not {term})"),
        },
    }
}

fn balanced(text: &str) -> bool {
    let mut depth = 0i64;
    for ch in text.chars() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

fn terminator_targets(terminator: &Terminator) -> Vec<&str> {
    match terminator {
        Terminator::Goto(label) => vec![label],
        Terminator::Branch {
            then_label,
            else_label,
            ..
        } => vec![then_label, else_label],
        Terminator::ForRange {
            body_label,
            exit_label,
            ..
        } => vec![body_label, exit_label],
        Terminator::Match {
            arms, otherwise, ..
        } => arms
            .iter()
            .map(|arm| arm.label.as_str())
            .chain(std::iter::once(otherwise.as_str()))
            .collect(),
        Terminator::Return(_) | Terminator::AssertFail { .. } | Terminator::Unreachable => {
            Vec::new()
        }
    }
}

/// The locals a closure made in `function` captures by reference.
fn closure_captures(function: &MirFunction) -> BTreeSet<String> {
    let mut captured = BTreeSet::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            let (Instruction::Assign { value, .. } | Instruction::WriteLoan { value, .. }) =
                instruction
            else {
                continue;
            };
            let Rvalue::Closure { captures, .. } = value else {
                continue;
            };
            for capture in captures {
                if capture.passing != MirReceiverKind::BorrowMut && !capture.mutated {
                    continue;
                }
                if let Some(place) = &capture.source_place {
                    captured.insert(root_of(place).to_string());
                }
                if let Operand::Place(place) | Operand::MovePlace(place) = &capture.value {
                    captured.insert(root_of(place).to_string());
                }
            }
        }
    }
    captured
}

/// The root local each loan of `blocks` reaches, following reborrows.
fn loan_roots(blocks: &[BasicBlock]) -> BTreeMap<String, String> {
    let mut parents: BTreeMap<&str, &str> = BTreeMap::new();
    for block in blocks {
        for instruction in &block.instructions {
            match instruction {
                Instruction::BeginLoan { loan, source, .. }
                | Instruction::BeginElementLoan { loan, source, .. } => {
                    parents.insert(loan, root_of(source));
                }
                Instruction::BeginReturnedLoan { loan, origin, .. } => {
                    parents.insert(loan, root_of(origin));
                }
                Instruction::Reborrow { loan, parent, .. }
                | Instruction::ReborrowElement { loan, parent, .. } => {
                    parents.insert(loan, root_of(parent));
                }
                _ => {}
            }
        }
    }
    let mut roots = BTreeMap::new();
    for &loan in parents.keys() {
        let mut current = loan;
        let mut steps = 0;
        while let Some(parent) = parents.get(current) {
            current = parent;
            steps += 1;
            if steps > parents.len() {
                break;
            }
        }
        roots.insert(loan.to_string(), current.to_string());
    }
    roots
}

/// The direction a write `target = value` steps `target`, when `value`
/// is a value `target` had in the loop plus or minus a constant. `defs`
/// holds every assignment the loop makes, by target, so a chain of
/// temporaries such as `t = i; k = 1; s = t + k; i = s` counts.
fn counter_step(
    target: &str,
    value: &Rvalue,
    defs: &BTreeMap<&str, Vec<&Rvalue>>,
    depth: usize,
) -> Option<Step> {
    if depth > 8 {
        return None;
    }
    match value {
        Rvalue::Binary {
            op: op @ (BinaryOp::Add | BinaryOp::Sub),
            left,
            right,
            ..
        } if reads_target(target, left, defs, 0) && is_constant(right, defs, 0) => {
            Some(if *op == BinaryOp::Add {
                Step::Up
            } else {
                Step::Down
            })
        }
        Rvalue::Use(Operand::Place(temp) | Operand::MovePlace(temp)) if temp != target => {
            let values = defs.get(temp.as_str())?;
            let mut step = None;
            for value in values {
                let next = counter_step(target, value, defs, depth + 1)?;
                if step.is_some() && step != Some(next) {
                    return None;
                }
                step = Some(next);
            }
            step
        }
        _ => None,
    }
}

/// Whether `operand` holds a value `target` had: `target` itself, or a
/// temporary every assignment of which copies one.
fn reads_target(
    target: &str,
    operand: &Operand,
    defs: &BTreeMap<&str, Vec<&Rvalue>>,
    depth: usize,
) -> bool {
    let (Operand::Place(place) | Operand::MovePlace(place)) = operand else {
        return false;
    };
    if place == target {
        return true;
    }
    if depth > 8 {
        return false;
    }
    match defs.get(place.as_str()) {
        Some(values) if !values.is_empty() => values.iter().all(|value| match value {
            Rvalue::Use(operand) => reads_target(target, operand, defs, depth + 1),
            _ => false,
        }),
        _ => false,
    }
}

/// Whether `operand` is a non-negative constant, directly or through
/// temporaries the loop sets only to constants.
fn is_constant(operand: &Operand, defs: &BTreeMap<&str, Vec<&Rvalue>>, depth: usize) -> bool {
    match operand {
        Operand::Int(_) => true,
        Operand::Place(place) | Operand::MovePlace(place) if depth <= 8 => {
            match defs.get(place.as_str()) {
                Some(values) if !values.is_empty() => values.iter().all(|value| match value {
                    Rvalue::Use(operand) => is_constant(operand, defs, depth + 1),
                    _ => false,
                }),
                _ => false,
            }
        }
        _ => false,
    }
}

/// Every root local `block` may write. With `root_assigns` false, a plain
/// `Assign` to a root local does not count.
fn collect_writes(
    block: &BasicBlock,
    loan_roots: &BTreeMap<String, String>,
    root_assigns: bool,
    writes: &mut BTreeSet<String>,
) {
    let mut place = |place: &str| {
        let root = root_of(place);
        writes.insert(root.to_string());
        if let Some(source) = loan_roots.get(root) {
            writes.insert(source.clone());
        }
    };
    for instruction in &block.instructions {
        match instruction {
            Instruction::Assign { target, value } => {
                if root_assigns || target.contains('.') {
                    place(target);
                }
                rvalue_writes(value, &mut place);
            }
            Instruction::WriteLoan { loan, value } => {
                place(loan);
                rvalue_writes(value, &mut place);
            }
            Instruction::ReadLoan { target, .. } => place(target),
            Instruction::PopCleanup { place: p, .. } => place(p),
            _ => {}
        }
    }
    if let Terminator::ForRange { binding, .. } = &block.terminator {
        place(binding);
    }
}

fn rvalue_writes(value: &Rvalue, place: &mut impl FnMut(&str)) {
    let args = match value {
        Rvalue::Call { callee, args, .. } => {
            match callee {
                CallTarget::Runtime {
                    op,
                    receiver_place: Some(receiver),
                    ..
                } if !runtime_op_is_pure(*op) => place(receiver),
                CallTarget::Member {
                    receiver_place: Some(receiver),
                    ..
                }
                | CallTarget::TraitMember {
                    receiver_place: Some(receiver),
                    ..
                } => place(receiver),
                _ => {}
            }
            args
        }
        Rvalue::StartTask { args, .. } => args,
        Rvalue::TupleTakeElement { place: p, .. } | Rvalue::UnionTakePayload { place: p, .. } => {
            place(p);
            return;
        }
        _ => return,
    };
    for arg in args {
        if let Some(writeback) = &arg.writeback_place {
            place(writeback);
        }
    }
}

/// Whether two values have the same structure, so that they merge
/// component by component.
fn same_shape(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::Int(_, x), Val::Int(_, y)) => x == y,
        (Val::Bool(_), Val::Bool(_)) | (Val::Str(_), Val::Str(_)) | (Val::Unit, Val::Unit) => true,
        (Val::Float(_, x), Val::Float(_, y)) => x == y,
        (
            Val::Map {
                key: a, value: x, ..
            },
            Val::Map {
                key: b, value: y, ..
            },
        ) => a == b && x == y,
        (Val::Set { elem: x, .. }, Val::Set { elem: y, .. }) => x == y,
        (Val::List { elem: x, .. }, Val::List { elem: y, .. }) => x == y,
        (Val::Tuple(x), Val::Tuple(y)) => x.len() == y.len(),
        (Val::Record(x, xs), Val::Record(y, ys)) => {
            x == y
                && xs
                    .iter()
                    .map(|(name, _)| name)
                    .eq(ys.iter().map(|(name, _)| name))
        }
        (Val::Enum(x, _, xs), Val::Enum(y, _, ys)) => {
            x == y && xs.iter().map(Vec::len).eq(ys.iter().map(Vec::len))
        }
        (Val::Union(_, xs), Val::Union(_, ys)) => xs.len() == ys.len(),
        _ => false,
    }
}

fn scalar_term(value: &Val) -> Option<&str> {
    match value {
        Val::Int(term, _) | Val::Bool(term) | Val::Str(term) | Val::Float(term, _) => Some(term),
        _ => None,
    }
}

/// The term of a `dict` key or `set` element the encoding models.
fn key_term(value: &Val) -> Option<String> {
    match value {
        Val::Int(term, _) | Val::Str(term) => Some(term.clone()),
        _ => None,
    }
}

fn tag_term(value: &Val) -> Option<&str> {
    match value {
        Val::Enum(_, tag, _) | Val::Union(tag, _) => Some(tag),
        _ => None,
    }
}

/// The components of a compound value, in a fixed order.
fn children(value: &Val) -> Vec<&Val> {
    match value {
        Val::Tuple(items) | Val::Union(_, items) => items.iter().collect(),
        Val::Record(_, fields) => fields.iter().map(|(_, value)| value).collect(),
        Val::Enum(_, _, payloads) => payloads.iter().flatten().collect(),
        _ => Vec::new(),
    }
}
