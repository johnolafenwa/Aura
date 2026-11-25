//! A memo of `Lowerer::infer_expr_type` for one outermost inference.
//!
//! Inferring the type of a call re-infers its receiver in several helpers,
//! so a chain such as `x.f().f().f()` costs a power of its length without a
//! memo. The memo lives only while one outermost inference runs, for one
//! lowerer, whose state cannot change during it. An entry is keyed by the
//! expression's whole contents, its source position included, because the
//! inference also builds expressions of its own, each a new value.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::ast::Expr;
use crate::sema::Type;

struct Memo {
    lowerer: usize,
    entries: HashMap<String, Option<Type>>,
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
    if !own {
        // Another lowerer, such as one made for a nested function, has
        // its own state: its results are not reused.
        return infer();
    }
    let Some(key) = contents(expr) else {
        return infer();
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
