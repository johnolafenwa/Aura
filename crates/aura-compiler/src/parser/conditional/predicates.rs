use super::*;
use aura_runtime::frozen_value::FrozenValue;

pub(super) fn imports_name(import: &ImportDecl, name: &str) -> bool {
    match &import.kind {
        ImportKind::Module { path, alias } => alias
            .as_ref()
            .or(path.first())
            .is_some_and(|bound| bound == name),
        ImportKind::From { names, .. } => names
            .iter()
            .any(|imported| imported.alias.as_ref().unwrap_or(&imported.name) == name),
    }
}

pub(super) fn build_import(import: &ImportDecl) -> bool {
    let path = match &import.kind {
        ImportKind::Module { path, .. } => path,
        ImportKind::From { module_path, .. } => module_path,
    };
    path == &["build"]
}

pub(super) fn evaluate(
    resolver: &mut Resolver,
    module: &Module,
    tokens: &[Token],
    expression: &Expr,
    locals: &BTreeSet<String>,
    inactive: &[Range<usize>],
    declaration: bool,
) -> Result<Option<bool>> {
    let mut names = BTreeSet::new();
    if !predicate_names(expression, &mut names) {
        return Ok(None);
    }
    if names.is_empty() && !declaration {
        return Ok(None);
    }
    let mut roots = BTreeSet::new();
    for reference in &names {
        let path = reference.split('.').collect::<Vec<_>>();
        let root = path[0];
        if locals.contains(root) {
            return Ok(None);
        }
        roots.insert(root.to_owned());
        if module.constants.iter().any(|binding| {
            binding.name == root
                && binding.required
                && is_active(binding.span, inactive)
                && (!declaration || binding.span.line < expression.span.line)
        }) {
            continue;
        }
        let mut marked = false;
        for import in module
            .imports
            .iter()
            .filter(|import| is_active(import.span, inactive))
        {
            if resolver.marked(import, &path)? {
                marked = true;
                break;
            }
        }
        if !marked {
            return Ok(None);
        }
    }
    let mut names = roots;
    let mut probe = Module {
        inactive_lines: Vec::new(),
        doc: None,
        imports: Vec::new(),
        constants: Vec::new(),
        items: Vec::new(),
        top_level_stmts: Vec::new(),
    };
    // Find the declaration closure. A dependency can name another function
    // defined later, and signatures can depend on classes, enums or aliases.
    let mut inspected = BTreeSet::new();
    loop {
        let Some(name) = names
            .iter()
            .find(|name| !inspected.contains(*name))
            .cloned()
        else {
            break;
        };
        inspected.insert(name.clone());
        for constant in module
            .constants
            .iter()
            .filter(|c| c.name == name && is_active(c.span, inactive))
        {
            collect_decl_names(tokens, constant.span, &mut names);
        }
        for item in module
            .items
            .iter()
            .filter(|item| item.name() == name && is_active(item_span(item), inactive))
        {
            collect_decl_names(tokens, item_span(item), &mut names);
        }
    }
    probe.constants.extend(
        module
            .constants
            .iter()
            .filter(|c| names.contains(&c.name) && is_active(c.span, inactive))
            .cloned(),
    );
    probe.items.extend(
        module
            .items
            .iter()
            .filter(|item| names.contains(item.name()) && is_active(item_span(item), inactive))
            .cloned(),
    );
    for import in module
        .imports
        .iter()
        .filter(|import| is_active(import.span, inactive))
    {
        if names.iter().any(|name| imports_name(import, name)) {
            let mut import = import.clone();
            if let ImportKind::From {
                names: selected, ..
            } = &mut import.kind
            {
                selected
                    .retain(|entry| names.contains(entry.alias.as_ref().unwrap_or(&entry.name)));
            }
            probe.imports.push(import);
        }
    }
    // A space makes the compiler's private spelling inaccessible in source.
    const RESULT: &str = "conditional predicate";
    probe.constants.push(ConstantDecl {
        public: false,
        required: true,
        name: RESULT.to_string(),
        annotation: None,
        value: expression.clone(),
        span: expression.span,
    });
    resolver.check_dependencies(&mut probe, tokens, expression.span)?;
    let program = resolver.compile(probe)?;
    match program
        .frozen_constants
        .get(&format!("{}::{RESULT}", program.module_name))
    {
        Some(FrozenValue::Bool(value)) => Ok(Some(*value)),
        _ => Err(Diagnostic::coded_at(
            "AU2071",
            expression.span,
            "a compile-time condition must have type bool",
        )),
    }
}

/// Gather names of operands while restricting guards to pure operators.
fn predicate_names(expr: &Expr, names: &mut BTreeSet<String>) -> bool {
    match &expr.kind {
        ExprKind::Name(name) => {
            names.insert(name.clone());
            true
        }
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::String(_)
        | ExprKind::Bool(_)
        | ExprKind::DurationNanos(_) => true,
        ExprKind::Member { object, field } => {
            let mut parts = vec![field.as_str()];
            let mut base = object.as_ref();
            while let ExprKind::Member { object, field } = &base.kind {
                parts.push(field);
                base = object;
            }
            if let ExprKind::Name(name) = &base.kind {
                parts.push(name);
                parts.reverse();
                names.insert(parts.join("."));
                true
            } else {
                predicate_names(base, names)
            }
        }
        ExprKind::Group(value)
        | ExprKind::Unary { expr: value, .. }
        | ExprKind::Cast { expr: value, .. }
        | ExprKind::IsNone { value, .. } => predicate_names(value, names),
        ExprKind::Binary { left, right, .. }
        | ExprKind::Index {
            object: left,
            index: right,
        }
        | ExprKind::Membership {
            value: left,
            container: right,
            ..
        } => predicate_names(left, names) && predicate_names(right, names),
        ExprKind::CompareChain { first, links } => {
            predicate_names(first, names)
                && links
                    .iter()
                    .all(|link| predicate_names(&link.operand, names))
        }
        ExprKind::Conditional {
            then_expr,
            condition,
            else_expr,
        } => [then_expr, condition, else_expr]
            .into_iter()
            .all(|value| predicate_names(value, names)),
        ExprKind::List(values) | ExprKind::Set(values) | ExprKind::Tuple(values) => {
            values.iter().all(|value| predicate_names(value, names))
        }
        ExprKind::Map(entries) => entries.iter().all(|entry| {
            predicate_names(&entry.key, names) && predicate_names(&entry.value, names)
        }),
        _ => false,
    }
}

fn collect_decl_names(tokens: &[Token], span: Span, names: &mut BTreeSet<String>) {
    let start = tokens.partition_point(|token| token.span.line < span.line);
    let mut iter = tokens[start..].iter().peekable();
    let mut depth = 0usize;
    while let Some(token) = iter.next() {
        match &token.kind {
            TokenKind::Identifier(name) => {
                names.insert(name.clone());
            }
            TokenKind::Indent => depth += 1,
            TokenKind::Dedent if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            TokenKind::Newline
                if depth == 0
                    && !iter
                        .peek()
                        .is_some_and(|next| next.kind == TokenKind::Indent) =>
            {
                break
            }
            _ => {}
        }
    }
}
