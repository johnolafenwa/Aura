use crate::diag::Span;
use crate::integer::IntegerValue;
use serde::{Deserialize, Serialize};
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
    pub payload: Option<TypeRef>,
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
    pub return_type: TypeRef,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraitDecl {
    pub public: bool,
    pub name: String,
    pub type_params: Vec<String>,
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

#[derive(Clone, Debug, Serialize)]
pub struct Param {
    pub name: String,
    pub passing: ReceiverKind,
    pub ty: TypeRef,
    pub default: Option<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub enum Stmt {
    Assign(AssignStmt),
    Pass(PassStmt),
    Return(ReturnStmt),
    If(IfStmt),
    Match(MatchStmt),
    For(ForStmt),
    With(WithStmt),
    Select(SelectStmt),
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
pub struct AssignStmt {
    pub mutable: bool,
    pub target: AssignTarget,
    pub annotation: Option<TypeRef>,
    pub op: Option<BinaryOp>,
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
pub enum Pattern {
    Variant(VariantPattern),
    Literal(LiteralPattern),
    Wildcard(Span),
}

#[derive(Clone, Debug, Serialize)]
pub struct VariantPattern {
    pub enum_name: Option<String>,
    pub variant_name: String,
    pub binding: Option<String>,
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
    Bool(bool),
    String(String),
}

#[derive(Clone, Debug, Serialize)]
pub struct ForStmt {
    pub binding: String,
    pub iterable: Expr,
    pub borrow_mode: Option<ReceiverKind>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct WithStmt {
    pub binding: String,
    pub value: Expr,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct SelectStmt {
    pub arms: Vec<SelectArm>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct SelectArm {
    pub binding: Option<String>,
    pub expr: Expr,
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
    DurationMillis(i128),
    Float(f64),
    Bool(bool),
    String(String),
    FString(Vec<FormatPart>),
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
    Spawn {
        detached: bool,
        value: Box<Expr>,
    },
    Try(Box<Expr>),
    Group(Box<Expr>),
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
pub struct TypeRef {
    pub name: String,
    pub args: Vec<TypeRef>,
    pub indirect: bool,
    pub span: Span,
}

#[cfg(test)]
mod tests {
    use super::{
        ClassDecl, EnumDecl, FieldDecl, FunctionDecl, ImplDecl, Item, ReceiverKind, TraitDecl,
        TypeRef,
    };
    use crate::diag::Span;
    use std::collections::BTreeMap;

    fn dummy_type(name: &str) -> TypeRef {
        TypeRef {
            name: name.to_string(),
            args: vec![],
            indirect: false,
            span: Span::new(1, 1),
        }
    }

    fn dummy_function(name: &str) -> FunctionDecl {
        FunctionDecl {
            public: false,
            name: name.to_string(),
            type_params: vec![],
            type_param_bounds: BTreeMap::new(),
            receiver: Some(ReceiverKind::Borrow),
            params: vec![],
            return_type: dummy_type("None"),
            body: vec![],
            span: Span::new(1, 1),
        }
    }

    #[test]
    fn item_name_matches_decl_name() {
        let class_item = Item::Class(ClassDecl {
            public: true,
            copy: false,
            name: "Point".to_string(),
            type_params: vec![],
            type_param_bounds: BTreeMap::new(),
            fields: vec![FieldDecl {
                public: true,
                name: "x".to_string(),
                ty: dummy_type("int32"),
                default: None,
                span: Span::new(1, 1),
            }],
            methods: vec![],
            span: Span::new(1, 1),
        });
        let enum_item = Item::Enum(EnumDecl {
            public: true,
            name: "Status".to_string(),
            type_params: vec![],
            type_param_bounds: BTreeMap::new(),
            variants: vec![],
            span: Span::new(1, 1),
        });
        let function_item = Item::Function(dummy_function("main"));
        let trait_item = Item::Trait(TraitDecl {
            public: true,
            name: "Display".to_string(),
            type_params: vec![],
            methods: vec![dummy_function("show")],
            span: Span::new(1, 1),
        });
        let impl_item = Item::Impl(ImplDecl {
            type_params: vec![],
            type_param_bounds: BTreeMap::new(),
            trait_name: "Display".to_string(),
            trait_args: vec![dummy_type("String")],
            for_type: dummy_type("Point"),
            methods: vec![dummy_function("show")],
            span: Span::new(1, 1),
        });

        assert_eq!(class_item.name(), "Point");
        assert_eq!(enum_item.name(), "Status");
        assert_eq!(function_item.name(), "main");
        assert_eq!(trait_item.name(), "Display");
        assert_eq!(impl_item.name(), "Display");
    }
}
