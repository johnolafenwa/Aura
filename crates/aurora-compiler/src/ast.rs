use crate::diag::Span;
use crate::integer::IntegerValue;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize)]
pub struct Module {
    pub imports: Vec<ImportDecl>,
    pub items: Vec<Item>,
    pub top_level_stmts: Vec<Stmt>,
}

#[derive(Clone, Debug, Serialize)]
pub enum Item {
    Class(ClassDecl),
    Enum(EnumDecl),
    Function(FunctionDecl),
    Trait(TraitDecl),
    Impl(ImplDecl),
}

impl Item {
    pub fn name(&self) -> &str {
        match self {
            Item::Class(class_decl) => &class_decl.name,
            Item::Enum(enum_decl) => &enum_decl.name,
            Item::Function(function_decl) => &function_decl.name,
            Item::Trait(trait_decl) => &trait_decl.name,
            Item::Impl(impl_decl) => &impl_decl.trait_name,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ImportDecl {
    pub kind: ImportKind,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub enum ImportKind {
    Module {
        path: Vec<String>,
    },
    From {
        module_path: Vec<String>,
        names: Vec<String>,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct ClassDecl {
    pub public: bool,
    pub copy: bool,
    pub name: String,
    pub type_params: Vec<String>,
    pub type_param_bounds: BTreeMap<String, Vec<TypeRef>>,
    pub fields: Vec<FieldDecl>,
    pub methods: Vec<FunctionDecl>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct FieldDecl {
    pub public: bool,
    pub name: String,
    pub ty: TypeRef,
    pub default: Option<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct EnumDecl {
    pub public: bool,
    pub name: String,
    pub type_params: Vec<String>,
    pub type_param_bounds: BTreeMap<String, Vec<TypeRef>>,
    pub variants: Vec<EnumVariantDecl>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct EnumVariantDecl {
    pub name: String,
    pub payloads: Vec<EnumPayloadFieldDecl>,
    pub named_payloads: bool,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct EnumPayloadFieldDecl {
    pub name: Option<String>,
    pub ty: TypeRef,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct FunctionDecl {
    pub public: bool,
    pub name: String,
    pub type_params: Vec<String>,
    pub type_param_bounds: BTreeMap<String, Vec<TypeRef>>,
    pub receiver: Option<ReceiverKind>,
    pub params: Vec<Param>,
    pub return_passing: ReceiverKind,
    pub return_borrow_source: Option<String>,
    pub return_type: TypeRef,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraitDecl {
    pub public: bool,
    pub name: String,
    pub type_params: Vec<String>,
    pub supertraits: Vec<TypeRef>,
    pub methods: Vec<FunctionDecl>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct ImplDecl {
    pub type_params: Vec<String>,
    pub type_param_bounds: BTreeMap<String, Vec<TypeRef>>,
    pub trait_name: String,
    pub trait_args: Vec<TypeRef>,
    pub for_type: TypeRef,
    pub methods: Vec<FunctionDecl>,
    pub span: Span,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize)]
pub enum ReceiverKind {
    Value,
    Borrow,
    BorrowMut,
}

/// Source-level ownership spelling for an ordinary parameter.
///
/// `Default` is resolved exactly once from the declared type: copy types use
/// value passing, while non-copy and unresolved generic types use a shared
/// borrow. Keeping this source intent separate prevents generic
/// specialization from changing the function ABI.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize)]
pub enum ParamMode {
    Default,
    Own,
    Borrow,
    BorrowMut,
}

#[derive(Clone, Debug, Serialize)]
pub struct Param {
    pub name: String,
    pub mode: ParamMode,
    pub borrow_label: Option<String>,
    pub ty: TypeRef,
    pub default: Option<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub enum Stmt {
    Assign(AssignStmt),
    Destructure(DestructureStmt),
    Pass(PassStmt),
    Assert(AssertStmt),
    Return(ReturnStmt),
    If(IfStmt),
    Match(MatchStmt),
    For(ForStmt),
    With(WithStmt),
    While(WhileStmt),
    Break(BreakStmt),
    Continue(ContinueStmt),
    Expr(ExprStmt),
}

#[derive(Clone, Debug, Serialize)]
pub struct PassStmt {
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct AssertStmt {
    pub condition: Expr,
    pub message: Option<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct AssignStmt {
    pub mutable: bool,
    pub target: AssignTarget,
    pub annotation: Option<TypeRef>,
    pub op: Option<BinaryOp>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct DestructureStmt {
    pub target: BindingTarget,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub enum AssignTarget {
    Name(String),
    Member { object: Box<Expr>, field: String },
    Index { object: Box<Expr>, index: Box<Expr> },
}

#[derive(Clone, Debug, Serialize)]
pub enum BindingTarget {
    Name {
        name: String,
        span: Span,
    },
    Tuple {
        elements: Vec<BindingTarget>,
        span: Span,
    },
}

impl BindingTarget {
    pub fn span(&self) -> Span {
        match self {
            Self::Name { span, .. } | Self::Tuple { span, .. } => *span,
        }
    }

    pub fn name(&self) -> Option<&str> {
        match self {
            Self::Name { name, .. } => Some(name),
            Self::Tuple { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ReturnStmt {
    pub value: Option<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct IfStmt {
    pub branches: Vec<IfBranch>,
    pub else_body: Option<Vec<Stmt>>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct IfBranch {
    pub condition: Expr,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct MatchStmt {
    pub scrutinee: Expr,
    pub borrow_mode: Option<ReceiverKind>,
    pub arms: Vec<MatchArm>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct MatchExprArm {
    pub pattern: Pattern,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub enum Pattern {
    Variant(VariantPattern),
    Tuple(TuplePattern),
    Binding(BindingPattern),
    Literal(LiteralPattern),
    Wildcard(Span),
}

#[derive(Clone, Debug, Serialize)]
pub struct TuplePattern {
    pub elements: Vec<Pattern>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct VariantPattern {
    pub enum_name: Option<String>,
    pub variant_name: String,
    pub subpatterns: Vec<Pattern>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct BindingPattern {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct LiteralPattern {
    pub kind: LiteralPatternKind,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub enum LiteralPatternKind {
    Int(IntegerValue),
    Float(f64),
    Bool(bool),
    String(String),
}

#[derive(Clone, Debug)]
pub struct ForStmt {
    pub target: BindingTarget,
    pub iterable: Expr,
    pub borrow_mode: Option<ReceiverKind>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

impl Serialize for ForStmt {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("ForStmt", 5)?;
        match &self.target {
            BindingTarget::Name { name, .. } => state.serialize_field("binding", name)?,
            BindingTarget::Tuple { .. } => state.serialize_field("target", &self.target)?,
        }
        state.serialize_field("iterable", &self.iterable)?;
        state.serialize_field("borrow_mode", &self.borrow_mode)?;
        state.serialize_field("body", &self.body)?;
        state.serialize_field("span", &self.span)?;
        state.end()
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct WithStmt {
    pub binding: String,
    pub value: Expr,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct WhileStmt {
    pub condition: Expr,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct BreakStmt {
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct ContinueStmt {
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExprStmt {
    pub expr: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub enum ExprKind {
    Name(String),
    Int(u128),
    DurationNanos(i128),
    /// Compiler-generated marker for an omitted builtin default. Source syntax
    /// never constructs this expression.
    BuiltinOmitted,
    Float(f64),
    Bool(bool),
    String(String),
    FString(Vec<FormatPart>),
    Tuple(Vec<Expr>),
    List(Vec<Expr>),
    Set(Vec<Expr>),
    Map(Vec<MapEntryExpr>),
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Cast {
        expr: Box<Expr>,
        ty: TypeRef,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Conditional {
        then_expr: Box<Expr>,
        condition: Box<Expr>,
        else_expr: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Argument>,
    },
    Specialize {
        expr: Box<Expr>,
        type_args: Vec<TypeRef>,
    },
    Member {
        object: Box<Expr>,
        field: String,
    },
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
    },
    Try(Box<Expr>),
    Group(Box<Expr>),
    Match {
        scrutinee: Box<Expr>,
        borrow_mode: Option<ReceiverKind>,
        arms: Vec<MatchExprArm>,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct MapEntryExpr {
    pub key: Expr,
    pub value: Expr,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum BinaryOp {
    And,
    Or,
    Add,
    Sub,
    Mul,
    Div,
    FloorDiv,
    Mod,
    Eq,
    NotEq,
    Less,
    LessEq,
    Greater,
    GreaterEq,
}

#[derive(Clone, Debug, Serialize)]
pub struct Argument {
    pub name: Option<String>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub enum FormatPart {
    Literal(String),
    Expr(Expr),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub enum TypeRefKind {
    Named { name: String, args: Vec<TypeRef> },
    Tuple(Vec<TypeRef>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeRef {
    pub kind: TypeRefKind,
    pub indirect: bool,
    pub span: Span,
}

impl Serialize for TypeRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match &self.kind {
            TypeRefKind::Named { name, args } => {
                let mut state = serializer.serialize_struct("TypeRef", 4)?;
                state.serialize_field("name", name)?;
                state.serialize_field("args", args)?;
                state.serialize_field("indirect", &self.indirect)?;
                state.serialize_field("span", &self.span)?;
                state.end()
            }
            TypeRefKind::Tuple(elements) => {
                let mut state = serializer.serialize_struct("TupleTypeRef", 3)?;
                state.serialize_field("elements", elements)?;
                state.serialize_field("indirect", &self.indirect)?;
                state.serialize_field("span", &self.span)?;
                state.end()
            }
        }
    }
}

impl TypeRef {
    pub fn named(name: impl Into<String>, args: Vec<TypeRef>, indirect: bool, span: Span) -> Self {
        Self {
            kind: TypeRefKind::Named {
                name: name.into(),
                args,
            },
            indirect,
            span,
        }
    }

    pub fn tuple(elements: Vec<TypeRef>, indirect: bool, span: Span) -> Self {
        Self {
            kind: TypeRefKind::Tuple(elements),
            indirect,
            span,
        }
    }

    pub fn named_parts(&self) -> Option<(&str, &[TypeRef])> {
        match &self.kind {
            TypeRefKind::Named { name, args } => Some((name, args)),
            TypeRefKind::Tuple(_) => None,
        }
    }

    pub fn elements(&self) -> Option<&[TypeRef]> {
        match &self.kind {
            TypeRefKind::Tuple(elements) => Some(elements),
            TypeRefKind::Named { .. } => None,
        }
    }
}

#[cfg(test)]
#[path = "ast_tests.rs"]
mod tests;
