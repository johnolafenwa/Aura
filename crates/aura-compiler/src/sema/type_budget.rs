//! Bounded accounting for transparent semantic type expansion.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use crate::diag::{Diagnostic, Result, Span};

use super::Type;

const DEFAULT_RESULT_NODE_LIMIT: usize = 65_536;
const DEFAULT_RECURSION_DEPTH_LIMIT: usize = 128;
const DEFAULT_AGGREGATE_NODE_LIMIT: usize = 1_048_576;
const DEFAULT_KEY_NODE_LIMIT: usize = 65_536;
const DEFAULT_KEY_DEPTH_LIMIT: usize = 128;
const DEFAULT_KEY_BYTE_LIMIT: usize = 1_048_576;

#[derive(Clone, Copy, Debug)]
struct ExpansionLimits {
    result_nodes: usize,
    recursion_depth: usize,
    aggregate_nodes: usize,
    key_nodes: usize,
    key_depth: usize,
    key_bytes: usize,
}

#[derive(Debug, Default)]
struct ExpansionState {
    depth: usize,
    aggregate_nodes: usize,
}

/// Shared, local accounting for alias-template expansion and substitution.
#[derive(Clone, Debug)]
pub(crate) struct ExpansionBudget {
    limits: ExpansionLimits,
    state: Rc<RefCell<ExpansionState>>,
    charge_aggregate: bool,
}

impl Default for ExpansionBudget {
    fn default() -> Self {
        Self {
            limits: ExpansionLimits {
                result_nodes: DEFAULT_RESULT_NODE_LIMIT,
                recursion_depth: DEFAULT_RECURSION_DEPTH_LIMIT,
                aggregate_nodes: DEFAULT_AGGREGATE_NODE_LIMIT,
                key_nodes: DEFAULT_KEY_NODE_LIMIT,
                key_depth: DEFAULT_KEY_DEPTH_LIMIT,
                key_bytes: DEFAULT_KEY_BYTE_LIMIT,
            },
            state: Rc::new(RefCell::new(ExpansionState::default())),
            charge_aggregate: true,
        }
    }
}

impl ExpansionBudget {
    /// Enters one alias-expansion frame. Dropping the guard restores the depth.
    pub(super) fn enter(&self, span: Span) -> Result<DepthGuard> {
        let mut state = self.state.borrow_mut();
        let next = state.depth.checked_add(1).ok_or_else(|| {
            capacity_error(
                span,
                format!(
                    "type expansion exceeds recursion depth limit of {}",
                    self.limits.recursion_depth
                ),
            )
        })?;
        if next > self.limits.recursion_depth {
            return Err(capacity_error(
                span,
                format!(
                    "type expansion exceeds recursion depth limit of {}",
                    self.limits.recursion_depth
                ),
            ));
        }
        state.depth = next;
        drop(state);
        Ok(DepthGuard {
            state: Rc::clone(&self.state),
        })
    }

    /// Validates the exact tree that `substitute_type` will clone.
    ///
    /// A substituted type parameter contributes its replacement tree at the
    /// parameter's position. The replacement is not substituted again, which
    /// mirrors `substitute_type` and avoids both cloning and Rust recursion.
    pub(super) fn check_substitution(
        &self,
        template: &Type,
        substitutions: &HashMap<String, Type>,
        span: Span,
    ) -> Result<()> {
        let nodes = prospective_nodes(
            template,
            substitutions,
            self.limits.result_nodes,
            self.limits.recursion_depth,
            span,
        )?;

        if !self.charge_aggregate {
            return Ok(());
        }

        let mut state = self.state.borrow_mut();
        let aggregate = state
            .aggregate_nodes
            .checked_add(nodes)
            .ok_or_else(|| aggregate_error(span, self.limits.aggregate_nodes))?;
        if aggregate > self.limits.aggregate_nodes {
            return Err(aggregate_error(span, self.limits.aggregate_nodes));
        }
        state.aggregate_nodes = aggregate;
        Ok(())
    }

    /// Checks the canonical key of the prospective substituted tree before
    /// either the tree or its key is allocated.
    pub(super) fn check_substitution_key(
        &self,
        template: &Type,
        substitutions: &HashMap<String, Type>,
        module_name: &str,
        canonical_names: &BTreeMap<String, String>,
        span: Span,
    ) -> Result<()> {
        check_key_shape(
            template,
            module_name,
            canonical_names,
            Some(substitutions),
            self.limits,
            span,
        )
    }

    /// Checks the structural canonical-key encoding before it is allocated.
    pub(super) fn check_canonical_key(
        &self,
        ty: &Type,
        module_name: &str,
        canonical_names: &BTreeMap<String, String>,
        span: Span,
    ) -> Result<()> {
        check_key_shape(ty, module_name, canonical_names, None, self.limits, span)
    }

    /// Retains per-expansion limits for MIR lowering after semantic checking,
    /// without charging the semantic compilation aggregate a second time.
    pub(crate) fn for_checked_lowering(&self) -> Self {
        Self {
            limits: self.limits,
            state: Rc::new(RefCell::new(ExpansionState::default())),
            charge_aggregate: false,
        }
    }

    #[cfg(test)]
    pub(super) fn with_limits(
        result_nodes: usize,
        recursion_depth: usize,
        aggregate_nodes: usize,
    ) -> Self {
        Self {
            limits: ExpansionLimits {
                result_nodes,
                recursion_depth,
                aggregate_nodes,
                key_nodes: DEFAULT_KEY_NODE_LIMIT,
                key_depth: DEFAULT_KEY_DEPTH_LIMIT,
                key_bytes: DEFAULT_KEY_BYTE_LIMIT,
            },
            state: Rc::new(RefCell::new(ExpansionState::default())),
            charge_aggregate: true,
        }
    }

    #[cfg(test)]
    pub(super) fn aggregate_nodes(&self) -> usize {
        self.state.borrow().aggregate_nodes
    }

    #[cfg(test)]
    pub(super) fn with_key_limits(key_nodes: usize, key_depth: usize, key_bytes: usize) -> Self {
        Self {
            limits: ExpansionLimits {
                result_nodes: DEFAULT_RESULT_NODE_LIMIT,
                recursion_depth: DEFAULT_RECURSION_DEPTH_LIMIT,
                aggregate_nodes: DEFAULT_AGGREGATE_NODE_LIMIT,
                key_nodes,
                key_depth,
                key_bytes,
            },
            state: Rc::new(RefCell::new(ExpansionState::default())),
            charge_aggregate: true,
        }
    }
}

/// RAII token for one active alias-expansion frame.
#[must_use = "the depth guard must remain alive for the expansion frame"]
pub(super) struct DepthGuard {
    state: Rc<RefCell<ExpansionState>>,
}

impl Drop for DepthGuard {
    fn drop(&mut self) {
        let mut state = self.state.borrow_mut();
        debug_assert!(state.depth > 0);
        state.depth = state.depth.saturating_sub(1);
    }
}

fn prospective_nodes(
    template: &Type,
    substitutions: &HashMap<String, Type>,
    result_node_limit: usize,
    depth_limit: usize,
    span: Span,
) -> Result<usize> {
    let mut stack = vec![(template, 1usize, true)];
    let mut nodes = 0usize;

    while let Some((ty, depth, substitute)) = stack.pop() {
        if substitute {
            if let Type::TypeParam(name) = ty {
                if let Some(replacement) = substitutions.get(name) {
                    stack.push((replacement, depth, false));
                    continue;
                }
            }
        }

        if depth > depth_limit {
            return Err(capacity_error(
                span,
                format!("type expansion exceeds recursion depth limit of {depth_limit}"),
            ));
        }
        nodes = nodes
            .checked_add(1)
            .ok_or_else(|| result_nodes_error(span, result_node_limit))?;
        if nodes > result_node_limit {
            return Err(result_nodes_error(span, result_node_limit));
        }

        let child_depth = depth
            .checked_add(1)
            .ok_or_else(|| capacity_error(span, "type expansion depth overflow"))?;
        match ty {
            Type::Named(_, args) | Type::Tuple(args) => {
                reserve_stack(&mut stack, args.len(), span)?;
                stack.extend(
                    args.iter()
                        .rev()
                        .map(|child| (child, child_depth, substitute)),
                );
            }
            Type::Union(union) => {
                reserve_stack(&mut stack, union.members.len(), span)?;
                stack.extend(
                    union
                        .members
                        .iter()
                        .rev()
                        .map(|member| (member, child_depth, substitute)),
                );
            }
            Type::Function {
                params,
                return_type,
            } => {
                let children = params
                    .len()
                    .checked_add(1)
                    .ok_or_else(|| capacity_error(span, "type expansion child-count overflow"))?;
                reserve_stack(&mut stack, children, span)?;
                stack.push((return_type, child_depth, substitute));
                stack.extend(
                    params
                        .iter()
                        .rev()
                        .map(|param| (&param.ty, child_depth, substitute)),
                );
            }
            Type::ReturnedView(view) => {
                reserve_stack(&mut stack, 1, span)?;
                stack.push((&view.pointee, child_depth, substitute));
            }
            Type::Callable(callable) => {
                let children =
                    callable.params.len().checked_add(1).ok_or_else(|| {
                        capacity_error(span, "type expansion child-count overflow")
                    })?;
                reserve_stack(&mut stack, children, span)?;
                stack.push((&callable.return_type, child_depth, substitute));
                stack.extend(
                    callable
                        .params
                        .iter()
                        .rev()
                        .map(|param| (&param.ty, child_depth, substitute)),
                );
            }
            Type::Closure {
                params,
                return_type,
                captures,
                ..
            } => {
                let children = params
                    .len()
                    .checked_add(captures.len())
                    .and_then(|count| count.checked_add(1))
                    .ok_or_else(|| capacity_error(span, "type expansion child-count overflow"))?;
                reserve_stack(&mut stack, children, span)?;
                stack.push((return_type, child_depth, substitute));
                stack.extend(
                    captures
                        .iter()
                        .rev()
                        .map(|capture| (&capture.ty, child_depth, substitute)),
                );
                stack.extend(
                    params
                        .iter()
                        .rev()
                        .map(|param| (&param.ty, child_depth, substitute)),
                );
            }
            Type::TypeParam(_) | Type::Module(_) | Type::Unit => {}
        }
    }

    Ok(nodes)
}

fn check_key_shape(
    ty: &Type,
    module_name: &str,
    canonical_names: &BTreeMap<String, String>,
    substitutions: Option<&HashMap<String, Type>>,
    limits: ExpansionLimits,
    span: Span,
) -> Result<()> {
    let (node_limit, depth_limit, byte_limit) =
        (limits.key_nodes, limits.key_depth, limits.key_bytes);
    let mut stack = vec![(ty, 1usize, module_name, true, true)];
    let mut nodes = 0usize;
    let mut bytes = "aura-type-key-v1:".len();

    while let Some((ty, depth, module, resolve_names, substitute)) = stack.pop() {
        if substitute {
            if let Type::TypeParam(name) = ty {
                if let Some(replacement) = substitutions.and_then(|values| values.get(name)) {
                    // Alias substitution inserts the caller's argument as-is.
                    // If it is later normalized as part of a template union,
                    // that union is rebuilt in the caller's context. Imported
                    // template leaves have already been qualified.
                    stack.push((replacement, depth, module_name, true, false));
                    continue;
                }
            }
        }
        if depth > depth_limit {
            return Err(key_limit_error(span, "depth", depth_limit));
        }
        nodes = nodes
            .checked_add(1)
            .ok_or_else(|| key_limit_error(span, "node", node_limit))?;
        if nodes > node_limit {
            return Err(key_limit_error(span, "node", node_limit));
        }
        let child_depth = depth
            .checked_add(1)
            .ok_or_else(|| capacity_error(span, "canonical type-key depth overflow"))?;

        match ty {
            Type::Unit => add_key_bytes(&mut bytes, 9, byte_limit, span)?,
            Type::ReturnedView(view) => {
                // `["returned_view",` followed by the mutability boolean,
                // the unquoted origin ordinal, and the pointee, each of the
                // last two preceded by `,`, then `]`: exactly the shape
                // `Type::canonical_key` renders for the node.
                add_key_bytes(&mut bytes, 20, byte_limit, span)?;
                add_key_bytes(
                    &mut bytes,
                    bool_json_len(view.mutable) + decimal_digit_count(view.origin),
                    byte_limit,
                    span,
                )?;
                reserve_key_stack(&mut stack, 1, span)?;
                stack.push((
                    &view.pointee,
                    child_depth,
                    module,
                    resolve_names,
                    substitute,
                ));
            }
            Type::Module(name) => {
                add_key_bytes(&mut bytes, 11, byte_limit, span)?;
                add_json_string(&mut bytes, name, byte_limit, span)?;
            }
            Type::TypeParam(name) => {
                add_key_bytes(&mut bytes, 14, byte_limit, span)?;
                add_json_string(&mut bytes, name, byte_limit, span)?;
            }
            Type::Named(name, args) => {
                add_key_bytes(
                    &mut bytes,
                    13usize.saturating_add(args.len().saturating_sub(1)),
                    byte_limit,
                    span,
                )?;
                let resolved = if resolve_names {
                    canonical_names
                        .get(name)
                        .map(String::as_str)
                        .unwrap_or(name)
                } else {
                    name
                };
                if super::is_builtin_type(resolved) || resolved.contains('.') || module.is_empty() {
                    add_json_string(&mut bytes, resolved, byte_limit, span)?;
                } else {
                    let content = escaped_json_content_len(module)
                        .checked_add(1)
                        .and_then(|length| length.checked_add(escaped_json_content_len(resolved)))
                        .and_then(|length| length.checked_add(2))
                        .ok_or_else(|| key_limit_error(span, "byte", byte_limit))?;
                    add_key_bytes(&mut bytes, content, byte_limit, span)?;
                }
                reserve_key_stack(&mut stack, args.len(), span)?;
                stack.extend(
                    args.iter()
                        .rev()
                        .map(|argument| (argument, child_depth, module, resolve_names, substitute)),
                );
            }
            Type::Tuple(elements) => {
                add_key_collection_overhead(&mut bytes, 12, elements.len(), byte_limit, span)?;
                reserve_key_stack(&mut stack, elements.len(), span)?;
                stack.extend(
                    elements
                        .iter()
                        .rev()
                        .map(|element| (element, child_depth, module, resolve_names, substitute)),
                );
            }
            Type::Union(union) => {
                add_key_collection_overhead(&mut bytes, 12, union.members.len(), byte_limit, span)?;
                reserve_key_stack(&mut stack, union.members.len(), span)?;
                stack.extend(union.members.iter().rev().map(|member| {
                    (
                        member,
                        child_depth,
                        union.module_name.as_str(),
                        false,
                        substitute,
                    )
                }));
            }
            Type::Function {
                params,
                return_type,
            } => {
                add_key_collection_overhead(&mut bytes, 16, params.len(), byte_limit, span)?;
                for param in params {
                    add_key_bytes(&mut bytes, 7, byte_limit, span)?;
                    add_json_string(&mut bytes, &param.name, byte_limit, span)?;
                    add_key_bytes(
                        &mut bytes,
                        receiver_kind_json_len(param.passing)
                            + bool_json_len(param.has_default)
                            + bool_json_len(param.keyword_only),
                        byte_limit,
                        span,
                    )?;
                }
                let children = params.len().checked_add(1).ok_or_else(|| {
                    capacity_error(span, "canonical type-key child-count overflow")
                })?;
                reserve_key_stack(&mut stack, children, span)?;
                stack.push((return_type, child_depth, module, resolve_names, substitute));
                stack.extend(
                    params
                        .iter()
                        .rev()
                        .map(|param| (&param.ty, child_depth, module, resolve_names, substitute)),
                );
            }
            Type::Callable(callable) => {
                // `["callable",` plus the separators around the task flag,
                // call kind, parameter list, and result.
                add_key_bytes(&mut bytes, 16, byte_limit, span)?;
                add_key_bytes(
                    &mut bytes,
                    bool_json_len(callable.task) + closure_call_kind_json_len(callable.call_kind),
                    byte_limit,
                    span,
                )?;
                add_key_collection_overhead(
                    &mut bytes,
                    2,
                    callable.params.len(),
                    byte_limit,
                    span,
                )?;
                for param in &callable.params {
                    add_key_bytes(&mut bytes, 7, byte_limit, span)?;
                    add_json_string(&mut bytes, &param.name, byte_limit, span)?;
                    add_key_bytes(
                        &mut bytes,
                        receiver_kind_json_len(param.passing)
                            + bool_json_len(param.has_default)
                            + bool_json_len(param.keyword_only),
                        byte_limit,
                        span,
                    )?;
                }
                let children = callable.params.len().checked_add(1).ok_or_else(|| {
                    capacity_error(span, "canonical type-key child-count overflow")
                })?;
                reserve_key_stack(&mut stack, children, span)?;
                stack.push((
                    &callable.return_type,
                    child_depth,
                    module,
                    resolve_names,
                    substitute,
                ));
                stack.extend(
                    callable
                        .params
                        .iter()
                        .rev()
                        .map(|param| (&param.ty, child_depth, module, resolve_names, substitute)),
                );
            }
            Type::Closure {
                params,
                return_type,
                captures,
                call_kind,
            } => {
                add_key_bytes(&mut bytes, 19, byte_limit, span)?;
                add_key_bytes(
                    &mut bytes,
                    closure_call_kind_json_len(*call_kind),
                    byte_limit,
                    span,
                )?;
                add_key_bytes(
                    &mut bytes,
                    params.len().saturating_sub(1) + captures.len().saturating_sub(1),
                    byte_limit,
                    span,
                )?;
                for param in params.iter() {
                    add_key_bytes(&mut bytes, 7, byte_limit, span)?;
                    add_json_string(&mut bytes, &param.name, byte_limit, span)?;
                    add_key_bytes(
                        &mut bytes,
                        receiver_kind_json_len(param.passing)
                            + bool_json_len(param.has_default)
                            + bool_json_len(param.keyword_only),
                        byte_limit,
                        span,
                    )?;
                }
                for capture in captures.iter() {
                    add_key_bytes(&mut bytes, 4, byte_limit, span)?;
                    add_json_string(&mut bytes, &capture.name, byte_limit, span)?;
                    add_key_bytes(
                        &mut bytes,
                        capture_mode_json_len(capture.mode),
                        byte_limit,
                        span,
                    )?;
                }
                let children = params
                    .len()
                    .checked_add(captures.len())
                    .and_then(|count| count.checked_add(1))
                    .ok_or_else(|| {
                        capacity_error(span, "canonical type-key child-count overflow")
                    })?;
                reserve_key_stack(&mut stack, children, span)?;
                stack.push((return_type, child_depth, module, resolve_names, substitute));
                stack.extend(
                    captures.iter().rev().map(|capture| {
                        (&capture.ty, child_depth, module, resolve_names, substitute)
                    }),
                );
                stack.extend(
                    params
                        .iter()
                        .rev()
                        .map(|param| (&param.ty, child_depth, module, resolve_names, substitute)),
                );
            }
        }
    }

    Ok(())
}

fn add_key_collection_overhead(
    bytes: &mut usize,
    base: usize,
    children: usize,
    limit: usize,
    span: Span,
) -> Result<()> {
    add_key_bytes(
        bytes,
        base.saturating_add(children.saturating_sub(1)),
        limit,
        span,
    )
}

fn add_json_string(bytes: &mut usize, value: &str, limit: usize, span: Span) -> Result<()> {
    let encoded = escaped_json_content_len(value)
        .checked_add(2)
        .ok_or_else(|| key_limit_error(span, "byte", limit))?;
    add_key_bytes(bytes, encoded, limit, span)
}

fn escaped_json_content_len(value: &str) -> usize {
    value
        .chars()
        .map(|character| match character {
            '"' | '\\' | '\u{0008}' | '\u{0009}' | '\n' | '\u{000c}' | '\r' => 2,
            character if character <= '\u{001f}' => 6,
            character => character.len_utf8(),
        })
        .fold(0usize, usize::saturating_add)
}

fn add_key_bytes(bytes: &mut usize, additional: usize, limit: usize, span: Span) -> Result<()> {
    *bytes = bytes
        .checked_add(additional)
        .ok_or_else(|| key_limit_error(span, "byte", limit))?;
    if *bytes > limit {
        return Err(key_limit_error(span, "byte", limit));
    }
    Ok(())
}

fn reserve_key_stack<'a>(
    stack: &mut Vec<(&'a Type, usize, &'a str, bool, bool)>,
    additional: usize,
    span: Span,
) -> Result<()> {
    stack.try_reserve(additional).map_err(|_| {
        capacity_error(
            span,
            "cannot reserve canonical type-key validation capacity",
        )
    })
}

fn receiver_kind_json_len(kind: crate::ast::ReceiverKind) -> usize {
    match kind {
        crate::ast::ReceiverKind::Value => 7,
        crate::ast::ReceiverKind::Borrow => 8,
        crate::ast::ReceiverKind::BorrowMut => 11,
    }
}

fn closure_call_kind_json_len(kind: super::ClosureCallKind) -> usize {
    match kind {
        super::ClosureCallKind::Repeatable => 12,
        super::ClosureCallKind::MutableRepeatable => 19,
        super::ClosureCallKind::Consuming => 11,
    }
}

fn capture_mode_json_len(mode: super::ClosureCaptureMode) -> usize {
    match mode {
        super::ClosureCaptureMode::Copy => 6,
        super::ClosureCaptureMode::Move => 6,
        super::ClosureCaptureMode::SharedView => 12,
        super::ClosureCaptureMode::MutableView => 13,
    }
}

fn bool_json_len(value: bool) -> usize {
    if value {
        4
    } else {
        5
    }
}

/// The rendered width of an unquoted JSON integer such as a returned-view
/// origin ordinal.
fn decimal_digit_count(value: usize) -> usize {
    let mut digits = 1;
    let mut remaining = value / 10;
    while remaining > 0 {
        digits += 1;
        remaining /= 10;
    }
    digits
}

fn key_limit_error(span: Span, kind: &str, limit: usize) -> Diagnostic {
    capacity_error(
        span,
        format!("canonical type key exceeds {kind} limit of {limit}"),
    )
}

fn reserve_stack(
    stack: &mut Vec<(&Type, usize, bool)>,
    additional: usize,
    span: Span,
) -> Result<()> {
    stack
        .try_reserve(additional)
        .map_err(|_| capacity_error(span, "cannot reserve type-expansion validation capacity"))
}

fn result_nodes_error(span: Span, limit: usize) -> Diagnostic {
    capacity_error(
        span,
        format!("type expansion exceeds per-result node limit of {limit}"),
    )
}

fn aggregate_error(span: Span, limit: usize) -> Diagnostic {
    capacity_error(
        span,
        format!("type expansion exceeds aggregate node limit of {limit}"),
    )
}

fn capacity_error(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::capacity_at(span, message)
}
