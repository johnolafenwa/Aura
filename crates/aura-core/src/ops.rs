//! Operator and passing enums shared by the AST, MIR, and the runtime.

use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ReceiverKind {
    Value,
    Borrow,
    BorrowMut,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
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
    Pow,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    NotEq,
    Less,
    LessEq,
    Greater,
    GreaterEq,
}
