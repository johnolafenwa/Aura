use crate::diag::Span;

#[derive(Clone, Debug)]
pub struct Module {
    pub items: Vec<Item>,
}

#[derive(Clone, Debug)]
pub enum Item {
    Class(ClassDecl),
    Function(FunctionDecl),
}

impl Item {
    pub fn name(&self) -> &str {
        match self {
            Item::Class(class_decl) => &class_decl.name,
            Item::Function(function_decl) => &function_decl.name,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ClassDecl {
    pub name: String,
    pub fields: Vec<FieldDecl>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct FieldDecl {
    pub public: bool,
    pub name: String,
    pub ty: TypeRef,
    pub default: Option<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct FunctionDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: TypeRef,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub ty: TypeRef,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Assign(AssignStmt),
    Return(ReturnStmt),
    Expr(ExprStmt),
}

#[derive(Clone, Debug)]
pub struct AssignStmt {
    pub mutable: bool,
    pub name: String,
    pub annotation: Option<TypeRef>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ReturnStmt {
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ExprStmt {
    pub expr: Expr,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Name(String),
    Int(i64),
    Float(f64),
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Argument>,
    },
    Member {
        object: Box<Expr>,
        field: String,
    },
    Group(Box<Expr>),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Clone, Debug)]
pub struct Argument {
    pub name: Option<String>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeRef {
    pub name: String,
    pub args: Vec<TypeRef>,
    pub span: Span,
}
