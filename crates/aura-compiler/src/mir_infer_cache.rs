//! A memo of `Lowerer::infer_expr_type` for one outermost inference.
//!
//! Inferring the type of a call re-infers its receiver in several helpers,
//! so a chain such as `x.f().f().f()` costs a power of its length without a
//! memo. The memo lives only while one outermost inference runs, for one
//! lowerer, whose state cannot change during it. An entry is keyed by the
//! expression's whole contents, its source position included, because the
//! inference also builds expressions of its own, each a new value.
//!
//! Building that key costs the size of the expression, so a chain would
//! still cost a power of its length. The calls and member reads that the
//! outermost expression holds through receivers and arguments stay alive
//! and unchanged at their addresses for the whole outermost inference, so
//! they are keyed by address instead. Only expressions the inference built
//! itself are keyed by contents.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use crate::ast::{Expr, ExprKind};
use crate::sema::Type;

struct Memo {
    lowerer: usize,
    /// Addresses of the calls and member reads inside the outermost
    /// expression, reached through callees, arguments and member objects.
    stable: HashSet<usize>,
    entries: HashMap<Key, Option<Type>>,
}

#[derive(PartialEq, Eq, Hash)]
enum Key {
    Stable(usize),
    Contents(String),
}

fn address(expr: &Expr) -> usize {
    expr as *const Expr as usize
}

/// The calls and member reads `outermost` holds, other than itself.
fn stable_parts(outermost: &Expr) -> HashSet<usize> {
    let mut stable = HashSet::new();
    let mut pending = vec![outermost];
    while let Some(expr) = pending.pop() {
        match &expr.kind {
            ExprKind::Call { callee, args } => {
                pending.push(callee);
                pending.extend(args.iter().map(|arg| &arg.value));
            }
            ExprKind::Member { object, .. } => pending.push(object),
            _ => continue,
        }
        if !std::ptr::eq(expr, outermost) {
            stable.insert(address(expr));
        }
    }
    stable
}

thread_local! {
    static MEMO: RefCell<Option<Memo>> = const { RefCell::new(None) };
}

/// Clears the memo when the outermost inference ends, even by a panic.
struct Outermost;

impl Drop for Outermost {
    fn drop(&mut self) {
        MEMO.with(|memo| *memo.borrow_mut() = None);
    }
}

/// The expression's contents, as the key of its entry.
fn contents(expr: &Expr) -> Option<String> {
    serde_json::to_string(expr).ok()
}

/// Runs `infer` for `expr` on the lowerer at address `lowerer`, reusing the
/// result of an earlier inference of the same expression in the same
/// outermost inference.
pub(super) fn memoized(
    lowerer: usize,
    expr: &Expr,
    infer: impl FnOnce() -> Option<Type>,
) -> Option<Type> {
    let started = MEMO.with(|memo| {
        let mut memo = memo.borrow_mut();
        if memo.is_some() {
            return false;
        }
        *memo = Some(Memo {
            lowerer,
            stable: stable_parts(expr),
            entries: HashMap::new(),
        });
        true
    });
    // Built only when this call started the memo: dropping one clears it.
    let _outermost = if started { Some(Outermost) } else { None };
    let own = MEMO.with(|memo| {
        memo.borrow()
            .as_ref()
            .is_some_and(|memo| memo.lowerer == lowerer)
    });
    // Only calls and member reads re-infer their parts, and the outermost
    // expression is not asked for again inside its own inference.
    if started || !matches!(expr.kind, ExprKind::Call { .. } | ExprKind::Member { .. }) {
        return infer();
    }
    if !own {
        // Another lowerer, such as one made for a nested function, has
        // its own state: its results are not reused.
        return infer();
    }
    let stable = MEMO.with(|memo| {
        memo.borrow()
            .as_ref()
            .is_some_and(|memo| memo.stable.contains(&address(expr)))
    });
    let key = if stable {
        Key::Stable(address(expr))
    } else {
        let Some(contents) = contents(expr) else {
            return infer();
        };
        Key::Contents(contents)
    };
    let hit = MEMO.with(|memo| {
        memo.borrow()
            .as_ref()
            .and_then(|memo| memo.entries.get(&key).cloned())
    });
    if let Some(ty) = hit {
        return ty;
    }
    let ty = infer();
    MEMO.with(|memo| {
        if let Some(memo) = memo.borrow_mut().as_mut() {
            memo.entries.insert(key, ty.clone());
        }
    });
    ty
}
