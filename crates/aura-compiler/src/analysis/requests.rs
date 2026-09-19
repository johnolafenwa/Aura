use super::*;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AnalysisSignatureHelp {
    pub label: String,
    pub parameters: Vec<String>,
    pub active_parameter: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AnalysisRename {
    pub range: AnalysisRange,
    pub edits: Vec<AnalysisDiagnosticEdit>,
}

fn byte_character(source: &str, line: usize, character: usize) -> Option<usize> {
    let text = source.lines().nth(line)?;
    let mut units = 0;
    for (offset, ch) in text.char_indices() {
        if units == character {
            return Some(offset);
        }
        units += ch.len_utf16();
        if units > character {
            return None;
        }
    }
    (units == character).then_some(text.len())
}

fn editor_range(source: &str, mut range: AnalysisRange) -> AnalysisRange {
    let imported = range
        .file_path
        .as_ref()
        .and_then(|file| std::fs::read_to_string(file).ok());
    let text = imported
        .as_deref()
        .unwrap_or(source)
        .lines()
        .nth(range.line)
        .unwrap_or("");
    range.start_character = text
        .get(..range.start_character)
        .map(|prefix| prefix.encode_utf16().count())
        .unwrap_or(range.start_character);
    range.end_character = text
        .get(..range.end_character)
        .map(|prefix| prefix.encode_utf16().count())
        .unwrap_or(range.end_character);
    range
}

fn request_analysis(path: &Path, source: &str) -> AnalysisOutput {
    let mut output = analyze_path_source(path, source);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let canonical = crate::canonicalize_if_exists(&absolute).unwrap_or(absolute);
    let mut declarations = Vec::new();
    fn collect(
        symbols: &mut [AnalysisSymbol],
        source: &str,
        declarations: &mut Vec<(AnalysisRange, AnalysisRange)>,
    ) {
        for symbol in symbols {
            let old = AnalysisRange {
                file_path: None,
                line: symbol.line,
                start_character: symbol.start_character,
                end_character: symbol.end_character,
            };
            if let Some((start, end)) = source
                .lines()
                .nth(symbol.line)
                .and_then(|line| find_identifier_in_line(line, &symbol.name))
            {
                let new = AnalysisRange {
                    start_character: start,
                    end_character: end,
                    ..old.clone()
                };
                declarations.push((old, new));
                symbol.start_character = start;
                symbol.end_character = end;
            }
            collect(&mut symbol.children, source, declarations);
        }
    }
    collect(&mut output.symbols, source, &mut declarations);
    let mut imported_declarations = BTreeMap::new();
    for item in &mut output.occurrences {
        if let Some(definition) = &mut item.definition {
            if definition
                .file_path
                .as_deref()
                .is_some_and(|file| Path::new(file) == path || Path::new(file) == canonical)
            {
                definition.file_path = None;
            }
            let mapping = if let Some(file) = &definition.file_path {
                imported_declarations
                    .entry(file.clone())
                    .or_insert_with(|| {
                        let mut ranges = Vec::new();
                        if let Ok(text) = std::fs::read_to_string(file) {
                            if let Ok(module) = crate::parse_source(&text) {
                                collect(&mut symbols_from_module(&module), &text, &mut ranges);
                                for (old, new) in &mut ranges {
                                    old.file_path = Some(file.clone());
                                    new.file_path = Some(file.clone());
                                }
                            }
                        }
                        ranges
                    })
            } else {
                &declarations
            };
            if let Some((_, replacement)) = mapping.iter().find(|(old, _)| old == definition) {
                *definition = replacement.clone();
            }
        }
    }
    output
}

fn contains(range: &AnalysisRange, line: usize, character: usize) -> bool {
    range.line == line && range.start_character <= character && character < range.end_character
}

fn occurrence_range(item: &AnalysisOccurrence) -> AnalysisRange {
    AnalysisRange {
        file_path: None,
        line: item.line,
        start_character: item.start_character,
        end_character: item.end_character,
    }
}

fn target_at(analysis: &AnalysisOutput, line: usize, character: usize) -> Option<AnalysisRange> {
    if let Some(item) = analysis
        .occurrences
        .iter()
        .find(|item| contains(&occurrence_range(item), line, character))
    {
        return item.definition.clone();
    }
    if let Some(definition) = analysis
        .occurrences
        .iter()
        .filter_map(|item| item.definition.as_ref())
        .find(|range| range.file_path.is_none() && contains(range, line, character))
    {
        return Some(definition.clone());
    }
    fn symbol_at(
        symbols: &[AnalysisSymbol],
        line: usize,
        character: usize,
    ) -> Option<AnalysisRange> {
        for symbol in symbols {
            let range = AnalysisRange {
                file_path: None,
                line: symbol.line,
                start_character: symbol.start_character,
                end_character: symbol.end_character,
            };
            if contains(&range, line, character) {
                return Some(range);
            }
            if let Some(range) = symbol_at(&symbol.children, line, character) {
                return Some(range);
            }
        }
        None
    }
    symbol_at(&analysis.symbols, line, character)
}

fn reference_ranges(
    analysis: &AnalysisOutput,
    target: &AnalysisRange,
    include_declaration: bool,
) -> Vec<AnalysisRange> {
    let mut ranges = analysis
        .occurrences
        .iter()
        .filter(|item| item.definition.as_ref() == Some(target))
        .map(occurrence_range)
        .filter(|range| include_declaration || range != target)
        .collect::<Vec<_>>();
    if include_declaration {
        ranges.push(target.clone());
    }
    ranges.sort_by(|a, b| {
        (&a.file_path, a.line, a.start_character, a.end_character).cmp(&(
            &b.file_path,
            b.line,
            b.start_character,
            b.end_character,
        ))
    });
    ranges.dedup();
    ranges
}

pub fn references_path_source(
    path: &Path,
    source: &str,
    line: usize,
    character: usize,
    include_declaration: bool,
) -> Vec<AnalysisRange> {
    let Some(character) = byte_character(source, line, character) else {
        return Vec::new();
    };
    let analysis = request_analysis(path, source);
    if !analysis.diagnostics.is_empty() {
        return Vec::new();
    }
    target_at(&analysis, line, character)
        .map(|target| {
            reference_ranges(&analysis, &target, include_declaration)
                .into_iter()
                .map(|range| editor_range(source, range))
                .collect()
        })
        .unwrap_or_default()
}

pub fn prepare_rename_path_source(
    path: &Path,
    source: &str,
    line: usize,
    character: usize,
) -> Option<AnalysisRange> {
    let character = byte_character(source, line, character)?;
    let analysis = request_analysis(path, source);
    if !analysis.diagnostics.is_empty() {
        return None;
    }
    let target = target_at(&analysis, line, character)?;
    if target.file_path.is_some() {
        return None;
    }
    reference_ranges(&analysis, &target, true)
        .into_iter()
        .find(|range| contains(range, line, character))
        .map(|range| editor_range(source, range))
}

pub fn rename_path_source(
    path: &Path,
    source: &str,
    line: usize,
    character: usize,
    new_name: &str,
) -> Option<AnalysisRename> {
    use crate::lexer::TokenKind;
    let character = byte_character(source, line, character)?;
    let tokens = crate::lexer::lex(new_name).ok()?;
    let identifiers = tokens
        .iter()
        .filter(|token| !matches!(token.kind, TokenKind::Newline | TokenKind::Eof))
        .collect::<Vec<_>>();
    if !matches!(identifiers.as_slice(), [token] if matches!(&token.kind, TokenKind::Identifier(name) if name == new_name))
        || matches!(new_name, "type" | "is")
    {
        return None;
    }
    let before = request_analysis(path, source);
    if !before.diagnostics.is_empty() {
        return None;
    }
    let target = target_at(&before, line, character)?;
    // Imported-package definitions belong to their own edit transaction.
    if target.file_path.is_some() {
        return None;
    }
    let ranges = reference_ranges(&before, &target, true);
    let range = ranges
        .iter()
        .find(|range| contains(range, line, character))?
        .clone();
    let mut lines = source.split('\n').map(str::to_owned).collect::<Vec<_>>();
    for edit in ranges.iter().rev() {
        let text = lines.get_mut(edit.line)?;
        if !text.is_char_boundary(edit.start_character)
            || !text.is_char_boundary(edit.end_character)
        {
            return None;
        }
        text.replace_range(edit.start_character..edit.end_character, new_name);
    }
    let after = request_analysis(path, &lines.join("\n"));
    if !after.diagnostics.is_empty() {
        return None;
    }
    let relocated = |original: &AnalysisRange| {
        if original.file_path.is_some() {
            return original.clone();
        }
        let shift = ranges
            .iter()
            .filter(|edit| {
                edit.line == original.line && edit.end_character <= original.start_character
            })
            .map(|edit| {
                new_name.len() as isize - (edit.end_character - edit.start_character) as isize
            })
            .sum::<isize>();
        let start = original
            .start_character
            .checked_add_signed(shift)
            .unwrap_or(usize::MAX);
        let len = if ranges.contains(original) {
            new_name.len()
        } else {
            original.end_character - original.start_character
        };
        AnalysisRange {
            file_path: None,
            line: original.line,
            start_character: start,
            end_character: start.saturating_add(len),
        }
    };
    // A successful type check alone cannot detect capture by an existing
    // same-typed binding. Compare every previously resolved occurrence.
    for item in &before.occurrences {
        let position = relocated(&occurrence_range(item));
        let expected = item.definition.as_ref().map(&relocated);
        let actual = after
            .occurrences
            .iter()
            .find(|item| occurrence_range(item) == position)?;
        if actual.definition != expected {
            return None;
        }
    }
    Some(AnalysisRename {
        range: editor_range(source, range),
        edits: ranges
            .into_iter()
            .map(|range| editor_range(source, range))
            .map(|range| AnalysisDiagnosticEdit {
                line: range.line,
                start_character: range.start_character,
                end_character: range.end_character,
                replacement: new_name.to_owned(),
                applicability: "machine-applicable".to_owned(),
            })
            .collect(),
    })
}

// Locate an unfinished call while respecting strings, comments, and nested
// collection/call delimiters. This is compiler-owned editor recovery; the LSP
// never parses or guesses a contract.
fn active_call(source: &str, cursor: usize) -> Option<(usize, usize, usize)> {
    let mut stack: Vec<(char, usize, usize, usize)> = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    let mut comment = false;
    for (offset, ch) in source.get(..cursor)?.char_indices() {
        if comment {
            if ch == '\n' {
                comment = false;
            }
            continue;
        }
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == delimiter {
                quote = None;
            }
            continue;
        }
        match ch {
            '#' => comment = true,
            '\'' | '"' => quote = Some(ch),
            '(' | '[' | '{' => stack.push((ch, offset, 0, offset + 1)),
            ')' | ']' | '}' => {
                stack.pop();
            }
            ',' => {
                if let Some((_, _, count, start)) = stack.last_mut() {
                    *count += 1;
                    *start = offset + 1;
                }
            }
            _ => {}
        }
    }
    if comment {
        return None;
    }
    stack
        .into_iter()
        .rev()
        .find(|(ch, _, _, _)| *ch == '(')
        .map(|(_, open, count, start)| (open, count, start))
}

fn call_site_contract(
    builder: &AnalysisBuilder<'_>,
    callee: &Expr,
    scope: &BTreeMap<String, BindingInfo>,
) -> Option<Type> {
    let specialization = match &callee.kind {
        ExprKind::Specialize { expr, type_args } => Some((
            expr.as_ref(),
            type_args
                .iter()
                .map(|ty| builder.lower_analysis_type_ref(ty))
                .collect::<Vec<_>>(),
        )),
        ExprKind::Index { object, index } => {
            let arguments = match &index.kind {
                ExprKind::Tuple(elements) => elements.as_slice(),
                _ => std::slice::from_ref(index.as_ref()),
            };
            arguments
                .iter()
                .map(|argument| builder.analysis_type_arg_expr(argument))
                .collect::<Option<Vec<_>>>()
                .map(|arguments| (object.as_ref(), arguments))
        }
        _ => None,
    };
    if let Some((base, arguments)) = specialization {
        if let Some((declaration, _)) = builder.returned_view_callee_decl(base, scope) {
            if !declaration.type_params.is_empty()
                && declaration.type_params.len() == arguments.len()
            {
                let contract = call_site_contract(builder, base, scope)?;
                let substitutions = crate::sema::substitutions_from_decl_type_args(
                    &declaration.type_params,
                    &arguments,
                );
                return Some(crate::sema::substitute_type(&contract, &substitutions));
            }
        }
    }
    if let Some(ty @ (Type::Function { .. } | Type::Closure { .. } | Type::Callable(_))) =
        builder.infer_expr_type(callee, scope)
    {
        return Some(ty);
    }
    if let ExprKind::Group(inner) = &callee.kind {
        return call_site_contract(builder, inner, scope);
    }
    let ExprKind::Member { object, field } = &callee.kind else {
        return None;
    };
    let receiver = builder.infer_expr_type(object, scope)?;
    let inherent = if let Type::Named(name, args) = &receiver {
        builder.class_info_for_type_name(name).and_then(|class| {
            class.methods.get(field).map(|method| {
                (
                    &method.decl,
                    &method.signature,
                    crate::sema::substitutions_from_decl_type_args(&class.decl.type_params, args),
                )
            })
        })
    } else {
        None
    };
    let (declaration, signature, substitutions) = inherent
        .or_else(|| {
            let (_, method, substitutions) = builder.trait_method_for_receiver(&receiver, field)?;
            Some((&method.decl, &method.signature, substitutions))
        })
        .or_else(|| {
            let (declaration, method, bound) =
                builder.unambiguous_trait_bound_method(&receiver, field, scope)?;
            Some((
                &method.decl,
                &method.signature,
                crate::sema::self_type_substitutions(
                    &declaration.decl,
                    &bound.trait_args,
                    receiver.clone(),
                ),
            ))
        })?;
    Some(Type::Function {
        params: declaration
            .params
            .iter()
            .zip(&signature.params)
            .zip(&signature.param_passings)
            .map(|((param, ty), passing)| FunctionParamContract {
                name: param.name.clone(),
                keyword_only: param.keyword_only,
                has_default: param.default.is_some(),
                passing: *passing,
                ty: crate::sema::substitute_type(ty, &substitutions),
            })
            .collect(),
        return_type: Box::new(crate::sema::substitute_type(
            &signature.return_type,
            &substitutions,
        )),
    })
}

pub fn signature_help_path_source(
    path: &Path,
    source: &str,
    line: usize,
    character: usize,
) -> Option<AnalysisSignatureHelp> {
    let character = byte_character(source, line, character)?;
    let line_start = source
        .split_inclusive('\n')
        .take(line)
        .map(str::len)
        .sum::<usize>();
    let line_text = source.lines().nth(line)?;
    if character > line_text.len() {
        return None;
    }
    let cursor = line_start.checked_add(character)?;
    let (open, ordinal, argument_start) = active_call(source, cursor)?;
    let prefix = &source[..open];
    // Try expression suffixes at lexical boundaries, longest first. Parsing
    // accepts qualified members, bound methods, specialization and call results.
    let statement_start = prefix.rfind('\n').map_or(0, |offset| offset + 1);
    let mut candidates = vec![statement_start];
    for (offset, ch) in prefix[statement_start..].char_indices() {
        if ch.is_whitespace() || matches!(ch, '=' | ',' | '(' | ':') {
            candidates.push(statement_start + offset + ch.len_utf8());
        }
    }
    let callee = candidates
        .into_iter()
        .find_map(|start| parser::parse_expression(prefix[start..].trim()).ok())?;
    let mut checker =
        |candidate: &str| crate::check_path_with_source_without_lockfile(path, candidate);
    let program = checker(source).ok().or_else(|| {
        recover_checked_program_after_position(source, line, character, &mut checker)
    })?;
    let builder = AnalysisBuilder::new(source, &program, Vec::new());
    let scope = builder.scope_for_position(line, character);
    let ty = call_site_contract(&builder, &callee, &scope)?;
    let (params, result) = match &ty {
        Type::Function {
            params,
            return_type,
        } => (params.as_slice(), return_type.as_ref()),
        Type::Closure {
            params,
            return_type,
            ..
        } => (params.as_slice(), return_type.as_ref()),
        Type::Callable(callable) => (callable.params.as_slice(), &callable.return_type),
        _ => return None,
    };
    let previous = source[open + 1..argument_start]
        .trim_end()
        .trim_end_matches(',');
    let mut used = BTreeSet::new();
    if let Ok(Expr {
        kind: ExprKind::Call { args, .. },
        ..
    }) = parser::parse_expression(&format!("callback({previous})"))
    {
        for argument in args {
            let slot = if let Some(name) = argument.name {
                params.iter().position(|param| param.name == name)
            } else {
                params
                    .iter()
                    .enumerate()
                    .position(|(index, param)| !param.keyword_only && !used.contains(&index))
            };
            if let Some(slot) = slot {
                used.insert(slot);
            }
        }
    }
    let next = (0..params.len())
        .find(|index| !used.contains(index))
        .unwrap_or(ordinal);
    let current_argument = source[argument_start..cursor].trim_start();
    let active_parameter = current_argument
        .split_once('=')
        .and_then(|(name, _)| params.iter().position(|param| param.name == name.trim()))
        .unwrap_or(next)
        .min(params.len().saturating_sub(1));
    let parameters = params
        .iter()
        .map(|param| {
            let signature = Type::Function {
                params: vec![param.clone()],
                return_type: Box::new(Type::Unit),
            }
            .to_string();
            signature
                .strip_prefix("def(")
                .and_then(|text| text.strip_suffix(") -> None"))
                .unwrap_or(&signature)
                .to_owned()
        })
        .collect();
    Some(AnalysisSignatureHelp {
        label: Type::Function {
            params: params.to_vec(),
            return_type: Box::new(result.clone()),
        }
        .to_string(),
        parameters,
        active_parameter,
    })
}

#[cfg(test)]
#[path = "requests_tests.rs"]
mod tests;
