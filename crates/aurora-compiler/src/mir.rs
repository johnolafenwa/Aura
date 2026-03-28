use crate::ast::{
    Argument, AssignStmt, AssignTarget, BinaryOp, Expr, ExprKind, IfStmt, MatchStmt, Pattern,
    ReceiverKind, Stmt, UnaryOp, WhileStmt,
};
use crate::call::{bind_call_arguments, callable_params_from_decl, CallConvention};
use crate::diag::Span;
use crate::integer::minimal_signed_type_for_negative_literal;
use crate::sema::{substitute_type, ModuleNamespace, Program, Type};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

fn is_known_enum_name(program: &Program, name: &str) -> bool {
    program.enums.contains_key(name) || matches!(name, "Result" | "Option" | "SendError")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MirModule {
    pub functions: Vec<MirFunction>,
    pub classes: Vec<MirClass>,
    pub trait_impls: Vec<MirTraitImpl>,
    pub top_level: Option<MirFunction>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MirFunction {
    pub name: String,
    pub module_name: String,
    pub span: crate::diag::Span,
    pub receiver: Option<MirReceiverKind>,
    pub params: Vec<MirParam>,
    pub local_types: Vec<MirLocalType>,
    pub return_type: Type,
    pub entry: String,
    pub blocks: Vec<BasicBlock>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MirLocalType {
    pub name: String,
    pub ty: Type,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MirClass {
    pub name: String,
    pub type_params: Vec<String>,
    pub fields: Vec<MirClassField>,
    pub methods: Vec<MirMethod>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MirClassField {
    pub name: String,
    pub ty: Type,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MirMethod {
    pub name: String,
    pub function_name: String,
    pub receiver: Option<MirReceiverKind>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MirTraitImpl {
    pub trait_name: String,
    pub for_type: Type,
    pub methods: Vec<MirMethod>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum MirReceiverKind {
    Value,
    Borrow,
    BorrowMut,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MirParam {
    pub name: String,
    pub passing: MirReceiverKind,
    pub ty: Type,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BasicBlock {
    pub label: String,
    pub instructions: Vec<Instruction>,
    pub terminator: Terminator,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Instruction {
    Assign {
        target: String,
        value: Rvalue,
    },
    Eval {
        value: Operand,
    },
    PushCleanup {
        place: String,
    },
    PopCleanup {
        place: String,
        cancel_before_cleanup: bool,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Rvalue {
    Use(Operand),
    FormatString {
        parts: Vec<MirFormatPart>,
    },
    Unary {
        op: UnaryOp,
        value: Operand,
        span: crate::diag::Span,
    },
    Cast {
        value: Operand,
        ty: Type,
        span: crate::diag::Span,
    },
    Try {
        value: Operand,
    },
    Spawn {
        detached: bool,
        task_group: Option<Operand>,
        function: String,
        args: Vec<MirArg>,
    },
    Binary {
        op: BinaryOp,
        left: Operand,
        right: Operand,
        span: crate::diag::Span,
    },
    Call {
        callee: CallTarget,
        args: Vec<MirArg>,
    },
    Construct {
        class_name: String,
        fields: Vec<MirFieldInit>,
    },
    EnumVariant {
        enum_name: String,
        variant_name: String,
        payload: Option<Operand>,
    },
    VariantPayload {
        scrutinee: Operand,
    },
    Member {
        object: Operand,
        field: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CallTarget {
    Name(String),
    Member {
        object: Operand,
        field: String,
        receiver_place: Option<String>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MirArg {
    pub name: Option<String>,
    pub value: Operand,
    pub writeback_place: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MirFieldInit {
    pub name: String,
    pub value: Operand,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MirFormatPart {
    Literal(String),
    Value(Operand),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MirMatchArm {
    pub enum_name: Option<String>,
    pub variant_name: Option<String>,
    pub wildcard: bool,
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MirSelectArm {
    pub binding: Option<String>,
    pub kind: MirSelectKind,
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MirSelectKind {
    Recv { channel: Operand },
    Send { channel: Operand, value: Operand },
    After { duration: Operand },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Operand {
    Place(String),
    Int(u128),
    Duration(i128),
    Float(f64),
    Bool(bool),
    String(String),
    Unit,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Terminator {
    Return(Operand),
    Goto(String),
    Branch {
        condition: Operand,
        then_label: String,
        else_label: String,
    },
    ForRange {
        binding: String,
        iterable: Operand,
        body_label: String,
        exit_label: String,
    },
    Match {
        scrutinee: Operand,
        arms: Vec<MirMatchArm>,
        otherwise: String,
    },
    Select {
        arms: Vec<MirSelectArm>,
        otherwise: String,
    },
    Unreachable,
}

pub fn lower(program: &Program) -> MirModule {
    let mut functions = program
        .functions
        .values()
        .map(|function| {
            lower_function(
                program,
                &function.decl.name,
                &function.module_name,
                function.decl.receiver,
                None,
                &function.decl,
                &function.signature.params,
                &function.signature.return_type,
            )
        })
        .collect::<Vec<_>>();
    let mut seen_function_names = functions
        .iter()
        .map(|function| function.name.clone())
        .collect::<BTreeSet<_>>();

    push_imported_module_functions(program, &mut functions, &mut seen_function_names);

    let mut classes = Vec::new();
    for class in program.classes.values() {
        let fields = class
            .decl
            .fields
            .iter()
            .map(|field| MirClassField {
                name: field.name.clone(),
                ty: class
                    .fields
                    .get(&field.name)
                    .expect("class field type should be available during MIR lowering")
                    .ty
                    .clone(),
            })
            .collect::<Vec<_>>();
        let mut methods = Vec::new();
        for method in class.methods.values() {
            let qualified_name = format!("{}.{}", class.decl.name, method.decl.name);
            functions.push(lower_function(
                program,
                &qualified_name,
                &class.module_name,
                method.decl.receiver,
                Some(Type::Named(
                    class.decl.name.clone(),
                    class
                        .decl
                        .type_params
                        .iter()
                        .cloned()
                        .map(Type::TypeParam)
                        .collect(),
                )),
                &method.decl,
                &method.signature.params,
                &method.signature.return_type,
            ));
            methods.push(MirMethod {
                name: method.decl.name.clone(),
                function_name: qualified_name,
                receiver: method.decl.receiver.map(lower_receiver_kind),
            });
        }
        classes.push(MirClass {
            name: class.decl.name.clone(),
            type_params: class.decl.type_params.clone(),
            fields,
            methods,
        });
    }
    let mut seen_class_names = classes
        .iter()
        .map(|class| class.name.clone())
        .collect::<BTreeSet<_>>();
    push_imported_module_classes(
        program,
        &mut classes,
        &mut functions,
        &mut seen_function_names,
        &mut seen_class_names,
    );

    let mut trait_impls = Vec::new();
    let mut seen_trait_impls = BTreeSet::new();
    for trait_impl in &program.trait_impls {
        seen_trait_impls.insert(format!(
            "{}{} for {}",
            trait_impl.trait_name,
            format_trait_args(&trait_impl.trait_args),
            trait_impl.for_type
        ));
        trait_impls.push(lower_trait_impl(
            program,
            &program.module_name,
            trait_impl,
            &mut functions,
            &mut seen_function_names,
        ));
    }
    push_imported_module_trait_impls(
        program,
        &mut functions,
        &mut trait_impls,
        &mut seen_function_names,
        &mut seen_trait_impls,
    );

    let top_level = if program.top_level_stmts.is_empty() {
        None
    } else {
        Some(lower_top_level(program))
    };

    MirModule {
        functions,
        classes,
        trait_impls,
        top_level,
    }
}

fn push_imported_module_functions(
    program: &Program,
    functions: &mut Vec<MirFunction>,
    seen: &mut BTreeSet<String>,
) {
    for namespace in program.module_registry.values() {
        push_imported_module_functions_from_namespace(program, namespace, functions, seen);
    }
}

fn push_imported_module_classes(
    program: &Program,
    classes: &mut Vec<MirClass>,
    functions: &mut Vec<MirFunction>,
    seen_function_names: &mut BTreeSet<String>,
    seen_class_names: &mut BTreeSet<String>,
) {
    for namespace in program.module_registry.values() {
        push_imported_module_classes_from_namespace(
            program,
            namespace,
            classes,
            functions,
            seen_function_names,
            seen_class_names,
        );
    }
}

fn push_imported_module_classes_from_namespace(
    program: &Program,
    namespace: &ModuleNamespace,
    classes: &mut Vec<MirClass>,
    functions: &mut Vec<MirFunction>,
    seen_function_names: &mut BTreeSet<String>,
    seen_class_names: &mut BTreeSet<String>,
) {
    for class in namespace.classes.values() {
        if !seen_class_names.insert(class.decl.name.clone()) {
            continue;
        }
        let fields = class
            .decl
            .fields
            .iter()
            .map(|field| MirClassField {
                name: field.name.clone(),
                ty: class
                    .fields
                    .get(&field.name)
                    .expect("imported class field type should exist during MIR lowering")
                    .ty
                    .clone(),
            })
            .collect::<Vec<_>>();
        let mut methods = Vec::new();
        for method in class.methods.values() {
            let qualified_name = format!(
                "{}::{}.{}",
                namespace.path, class.decl.name, method.decl.name
            );
            if seen_function_names.insert(qualified_name.clone()) {
                functions.push(lower_function(
                    program,
                    &qualified_name,
                    &class.module_name,
                    method.decl.receiver,
                    Some(Type::Named(
                        class.decl.name.clone(),
                        class
                            .decl
                            .type_params
                            .iter()
                            .cloned()
                            .map(Type::TypeParam)
                            .collect(),
                    )),
                    &method.decl,
                    &method.signature.params,
                    &method.signature.return_type,
                ));
            }
            methods.push(MirMethod {
                name: method.decl.name.clone(),
                function_name: qualified_name,
                receiver: method.decl.receiver.map(lower_receiver_kind),
            });
        }
        classes.push(MirClass {
            name: class.decl.name.clone(),
            type_params: class.decl.type_params.clone(),
            fields,
            methods,
        });
    }
    for child in namespace.modules.values() {
        push_imported_module_classes_from_namespace(
            program,
            child,
            classes,
            functions,
            seen_function_names,
            seen_class_names,
        );
    }
}

fn push_imported_module_trait_impls(
    program: &Program,
    functions: &mut Vec<MirFunction>,
    trait_impls: &mut Vec<MirTraitImpl>,
    seen_function_names: &mut BTreeSet<String>,
    seen_trait_impls: &mut BTreeSet<String>,
) {
    for namespace in program.module_registry.values() {
        for trait_impl in &namespace.trait_impls {
            let impl_key = format!(
                "{}{} for {}",
                trait_impl.trait_name,
                format_trait_args(&trait_impl.trait_args),
                trait_impl.for_type
            );
            if !seen_trait_impls.insert(impl_key) {
                continue;
            }
            trait_impls.push(lower_trait_impl(
                program,
                &namespace.path,
                trait_impl,
                functions,
                seen_function_names,
            ));
        }
    }
}

fn lower_trait_impl(
    program: &Program,
    module_name: &str,
    trait_impl: &crate::sema::TraitImplInfo,
    functions: &mut Vec<MirFunction>,
    seen_function_names: &mut BTreeSet<String>,
) -> MirTraitImpl {
    let mut methods = Vec::new();
    for method in trait_impl.methods.values() {
        let qualified_name = format!(
            "{}{} for {}.{}",
            trait_impl.trait_name,
            format_trait_args(&trait_impl.trait_args),
            trait_impl.for_type,
            method.decl.name
        );
        if seen_function_names.insert(qualified_name.clone()) {
            functions.push(lower_function(
                program,
                &qualified_name,
                module_name,
                method.decl.receiver,
                Some(trait_impl.for_type.clone()),
                &method.decl,
                &method.signature.params,
                &method.signature.return_type,
            ));
        }
        methods.push(MirMethod {
            name: method.decl.name.clone(),
            function_name: qualified_name,
            receiver: method.decl.receiver.map(lower_receiver_kind),
        });
    }
    MirTraitImpl {
        trait_name: trait_impl.trait_name.clone(),
        for_type: trait_impl.for_type.clone(),
        methods,
    }
}

fn format_trait_args(args: &[Type]) -> String {
    if args.is_empty() {
        String::new()
    } else {
        format!(
            "[{}]",
            args.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn push_imported_module_functions_from_namespace(
    program: &Program,
    namespace: &ModuleNamespace,
    functions: &mut Vec<MirFunction>,
    seen: &mut BTreeSet<String>,
) {
    for (name, function) in &namespace.functions {
        let qualified_name = imported_module_function_name(&namespace.path, name);
        if seen.insert(qualified_name.clone()) {
            functions.push(lower_function(
                program,
                &qualified_name,
                &function.module_name,
                function.decl.receiver,
                None,
                &function.decl,
                &function.signature.params,
                &function.signature.return_type,
            ));
        }
    }
    for child in namespace.modules.values() {
        push_imported_module_functions_from_namespace(program, child, functions, seen);
    }
}

fn imported_module_function_name(module_path: &str, name: &str) -> String {
    format!("{}::{}", module_path, name)
}

fn lower_receiver_kind(receiver: ReceiverKind) -> MirReceiverKind {
    match receiver {
        ReceiverKind::Value => MirReceiverKind::Value,
        ReceiverKind::Borrow => MirReceiverKind::Borrow,
        ReceiverKind::BorrowMut => MirReceiverKind::BorrowMut,
    }
}

fn lower_function(
    program: &Program,
    name: &str,
    module_name: &str,
    receiver: Option<ReceiverKind>,
    receiver_type: Option<Type>,
    function: &crate::ast::FunctionDecl,
    param_types: &[Type],
    return_type: &Type,
) -> MirFunction {
    let params = function
        .params
        .iter()
        .zip(param_types.iter())
        .map(|(param, ty)| MirParam {
            name: param.name.clone(),
            passing: lower_receiver_kind(param.passing),
            ty: ty.clone(),
        })
        .collect::<Vec<_>>();

    let mut lowerer = Lowerer::new(program, name, module_name);
    if let Some(receiver_type) = receiver_type {
        lowerer
            .local_types
            .insert("self".to_string(), receiver_type);
    }
    for (param, ty) in function.params.iter().zip(param_types.iter()) {
        lowerer.local_types.insert(param.name.clone(), ty.clone());
    }
    lowerer.lower_stmts(&function.body);
    lowerer.finish(MirFunctionSpec {
        name: name.to_string(),
        span: function.span,
        receiver: receiver.map(lower_receiver_kind),
        params,
        return_type: return_type.clone(),
        default_return: Operand::Unit,
    })
}

fn lower_top_level(program: &Program) -> MirFunction {
    let mut lowerer = Lowerer::new(program, "__script", &program.module_name);
    lowerer.lower_stmts(&program.top_level_stmts);
    lowerer.finish(MirFunctionSpec {
        name: "__script".to_string(),
        span: crate::diag::Span::new(1, 1),
        receiver: None,
        params: Vec::new(),
        return_type: Type::named("int32"),
        default_return: Operand::Int(0),
    })
}

struct MirFunctionSpec {
    name: String,
    span: Span,
    receiver: Option<MirReceiverKind>,
    params: Vec<MirParam>,
    return_type: Type,
    default_return: Operand,
}

struct Lowerer<'a> {
    program: &'a Program,
    function_name: &'a str,
    module_name: &'a str,
    blocks: Vec<BasicBlockBuilder>,
    current_block: usize,
    temp_counter: usize,
    block_counter: usize,
    loop_stack: Vec<LoopLabels>,
    with_stack: Vec<String>,
    local_types: std::collections::BTreeMap<String, Type>,
    scoped_names: Vec<std::collections::HashMap<String, String>>,
}

struct LoopLabels {
    break_label: String,
    continue_label: String,
    cleanup_depth: usize,
}

struct BasicBlockBuilder {
    label: String,
    instructions: Vec<Instruction>,
    terminator: Option<Terminator>,
}

impl<'a> Lowerer<'a> {
    fn find_namespace_in_modules<'b>(
        modules: &'b BTreeMap<String, ModuleNamespace>,
        path: &str,
    ) -> Option<&'b ModuleNamespace> {
        for namespace in modules.values() {
            if namespace.path == path {
                return Some(namespace);
            }
            if let Some(found) = Self::find_namespace_in_modules(&namespace.modules, path) {
                return Some(found);
            }
            if let Some(found) = Self::find_namespace_in_modules(&namespace.imported_modules, path)
            {
                return Some(found);
            }
        }
        None
    }

    fn new(program: &'a Program, function_name: &'a str, module_name: &'a str) -> Self {
        Self {
            program,
            function_name,
            module_name,
            blocks: vec![BasicBlockBuilder {
                label: "entry".to_string(),
                instructions: Vec::new(),
                terminator: None,
            }],
            current_block: 0,
            temp_counter: 0,
            block_counter: 0,
            loop_stack: Vec::new(),
            with_stack: Vec::new(),
            local_types: std::collections::BTreeMap::new(),
            scoped_names: Vec::new(),
        }
    }

    fn module_namespace(&self, path: &str) -> Option<&ModuleNamespace> {
        if let Some(namespace) = self.program.module_registry.get(path) {
            return Some(namespace);
        }
        self.current_module_namespace()
            .and_then(|current| Self::find_namespace_in_modules(&current.imported_modules, path))
            .or_else(|| Self::find_namespace_in_modules(&self.program.imported_modules, path))
    }

    fn current_module_namespace(&self) -> Option<&ModuleNamespace> {
        if self.module_name == self.program.module_name {
            None
        } else {
            self.program.module_registry.get(self.module_name)
        }
    }

    fn resolve_function_info(&self, name: &str) -> Option<&crate::sema::FunctionInfo> {
        self.current_module_namespace()
            .and_then(|namespace| namespace.all_functions.get(name))
            .or_else(|| self.program.functions.get(name))
    }

    fn resolve_class_info(&self, name: &str) -> Option<&crate::sema::ClassInfo> {
        if let Some((module_path, item_name)) = name.rsplit_once('.') {
            if let Some(namespace) = self.module_namespace(module_path) {
                if let Some(class_info) = namespace
                    .classes
                    .get(item_name)
                    .or_else(|| namespace.all_classes.get(item_name))
                {
                    return Some(class_info);
                }
            }
        }
        self.current_module_namespace()
            .and_then(|namespace| namespace.all_classes.get(name))
            .or_else(|| self.program.classes.get(name))
            .or_else(|| self.find_imported_class_info(name))
    }

    fn resolve_enum_info(&self, name: &str) -> Option<&crate::sema::EnumInfo> {
        if let Some((module_path, item_name)) = name.rsplit_once('.') {
            if let Some(namespace) = self.module_namespace(module_path) {
                if let Some(enum_info) = namespace
                    .enums
                    .get(item_name)
                    .or_else(|| namespace.all_enums.get(item_name))
                {
                    return Some(enum_info);
                }
            }
        }
        self.current_module_namespace()
            .and_then(|namespace| namespace.all_enums.get(name))
            .or_else(|| self.program.enums.get(name))
            .or_else(|| self.find_imported_enum_info(name))
    }

    fn infer_module_path(&self, expr: &Expr) -> Option<String> {
        match &expr.kind {
            ExprKind::Name(name) => self
                .current_module_namespace()
                .and_then(|namespace| namespace.imported_modules.get(name))
                .or_else(|| self.program.imported_modules.get(name))
                .map(|namespace| namespace.path.clone()),
            ExprKind::Specialize { expr, .. } => self.infer_module_path(expr),
            ExprKind::Group(inner) => self.infer_module_path(inner),
            ExprKind::Member { object, field } => {
                let parent = self.infer_module_path(object)?;
                let namespace = self.module_namespace(&parent)?;
                namespace.modules.get(field).map(|child| child.path.clone())
            }
            _ => None,
        }
    }

    fn qualified_module_item(&self, expr: &Expr) -> Option<(String, String)> {
        match &expr.kind {
            ExprKind::Specialize { expr, .. } => self.qualified_module_item(expr),
            ExprKind::Member { object, field } => self
                .infer_module_path(object)
                .map(|path| (path, field.clone())),
            ExprKind::Group(inner) => self.qualified_module_item(inner),
            _ => None,
        }
    }

    fn find_class_in_modules<'b>(
        modules: &'b BTreeMap<String, ModuleNamespace>,
        name: &str,
        found: &mut Option<&'b crate::sema::ClassInfo>,
        ambiguous: &mut bool,
    ) {
        for namespace in modules.values() {
            if let Some(class_info) = namespace
                .classes
                .get(name)
                .or_else(|| namespace.all_classes.get(name))
            {
                if found.is_some() {
                    *ambiguous = true;
                } else {
                    *found = Some(class_info);
                }
            }
            Self::find_class_in_modules(&namespace.modules, name, found, ambiguous);
        }
    }

    fn find_enum_in_modules<'b>(
        modules: &'b BTreeMap<String, ModuleNamespace>,
        name: &str,
        found: &mut Option<&'b crate::sema::EnumInfo>,
        ambiguous: &mut bool,
    ) {
        for namespace in modules.values() {
            if let Some(enum_info) = namespace
                .enums
                .get(name)
                .or_else(|| namespace.all_enums.get(name))
            {
                if found.is_some() {
                    *ambiguous = true;
                } else {
                    *found = Some(enum_info);
                }
            }
            Self::find_enum_in_modules(&namespace.modules, name, found, ambiguous);
        }
    }

    fn find_imported_class_info(&self, name: &str) -> Option<&crate::sema::ClassInfo> {
        let modules = self
            .current_module_namespace()
            .map(|namespace| &namespace.imported_modules)
            .unwrap_or(&self.program.imported_modules);
        let mut found = None;
        let mut ambiguous = false;
        Self::find_class_in_modules(modules, name, &mut found, &mut ambiguous);
        if ambiguous {
            None
        } else {
            found
        }
    }

    fn find_imported_enum_info(&self, name: &str) -> Option<&crate::sema::EnumInfo> {
        let modules = self
            .current_module_namespace()
            .map(|namespace| &namespace.imported_modules)
            .unwrap_or(&self.program.imported_modules);
        let mut found = None;
        let mut ambiguous = false;
        Self::find_enum_in_modules(modules, name, &mut found, &mut ambiguous);
        if ambiguous {
            None
        } else {
            found
        }
    }

    fn finish(mut self, spec: MirFunctionSpec) -> MirFunction {
        if self.blocks[self.current_block].terminator.is_none() {
            self.blocks[self.current_block].terminator =
                Some(Terminator::Return(spec.default_return));
        }

        MirFunction {
            name: spec.name,
            module_name: self.module_name.to_string(),
            span: spec.span,
            receiver: spec.receiver,
            params: spec.params,
            local_types: self
                .local_types
                .into_iter()
                .map(|(name, ty)| MirLocalType { name, ty })
                .collect(),
            return_type: spec.return_type,
            entry: self.blocks[0].label.clone(),
            blocks: self
                .blocks
                .into_iter()
                .map(|block| BasicBlock {
                    label: block.label,
                    instructions: block.instructions,
                    terminator: block.terminator.unwrap_or(Terminator::Unreachable),
                })
                .collect(),
        }
    }

    fn lower_stmts(&mut self, statements: &[Stmt]) {
        for stmt in statements {
            if !self.lower_stmt(stmt) {
                break;
            }
        }
    }

    fn lower_stmt(&mut self, stmt: &Stmt) -> bool {
        match stmt {
            Stmt::Assign(assign) => {
                self.lower_assign(assign);
                true
            }
            Stmt::Pass(_) => true,
            Stmt::Expr(expr_stmt) => {
                let value = self.lower_expr(&expr_stmt.expr);
                self.emit(Instruction::Eval { value });
                true
            }
            Stmt::Return(return_stmt) => {
                let mut value = if let Some(value) = &return_stmt.value {
                    self.lower_expr(value)
                } else {
                    Operand::Unit
                };
                if !self.with_stack.is_empty() {
                    let temp = self.new_temp();
                    self.emit(Instruction::Assign {
                        target: temp.clone(),
                        value: Rvalue::Use(value),
                    });
                    value = Operand::Place(temp);
                }
                self.emit_cleanup_range(0, true);
                self.terminate(Terminator::Return(value));
                false
            }
            Stmt::If(if_stmt) => {
                self.lower_if(if_stmt);
                true
            }
            Stmt::Match(match_stmt) => {
                self.lower_match(match_stmt);
                true
            }
            Stmt::For(for_stmt) => {
                self.lower_for(for_stmt);
                true
            }
            Stmt::With(with_stmt) => {
                let value = self.lower_expr(&with_stmt.value);
                self.emit(Instruction::Assign {
                    target: with_stmt.binding.clone(),
                    value: Rvalue::Use(value),
                });
                self.emit(Instruction::PushCleanup {
                    place: with_stmt.binding.clone(),
                });
                self.with_stack.push(with_stmt.binding.clone());
                self.lower_stmts(&with_stmt.body);
                if !self.current_terminated() {
                    self.emit(Instruction::PopCleanup {
                        place: with_stmt.binding.clone(),
                        cancel_before_cleanup: false,
                    });
                }
                self.with_stack.pop();
                !self.current_terminated()
            }
            Stmt::Select(select_stmt) => {
                self.lower_select(select_stmt);
                true
            }
            Stmt::While(while_stmt) => {
                self.lower_while(while_stmt);
                true
            }
            Stmt::Break(_) => {
                let loop_labels = self.loop_stack.last().expect("checked loop context");
                let cleanup_depth = loop_labels.cleanup_depth;
                let break_label = loop_labels.break_label.clone();
                self.emit_cleanup_range(cleanup_depth, true);
                self.terminate(Terminator::Goto(break_label));
                false
            }
            Stmt::Continue(_) => {
                let loop_labels = self.loop_stack.last().expect("checked loop context");
                let cleanup_depth = loop_labels.cleanup_depth;
                let continue_label = loop_labels.continue_label.clone();
                self.emit_cleanup_range(cleanup_depth, true);
                self.terminate(Terminator::Goto(continue_label));
                false
            }
        }
    }

    fn lower_assign(&mut self, assign: &AssignStmt) {
        let target = self.render_assign_target(&assign.target);
        if let (AssignTarget::Name(name), Some(annotation)) = (&assign.target, &assign.annotation) {
            self.local_types
                .entry(name.clone())
                .or_insert_with(|| lower_type_ref(annotation));
        } else if let AssignTarget::Name(name) = &assign.target {
            if let Some(inferred) = self.infer_expr_type(&assign.value) {
                self.local_types.entry(name.clone()).or_insert(inferred);
            }
        }
        if let Some(op) = assign.op {
            let value = self.lower_expr(&assign.value);
            self.emit(Instruction::Assign {
                target: target.clone(),
                value: Rvalue::Binary {
                    op,
                    left: Operand::Place(target),
                    right: value,
                    span: assign.span,
                },
            });
            return;
        }

        let value = self.lower_expr(&assign.value);
        self.emit(Instruction::Assign {
            target,
            value: Rvalue::Use(value),
        });
    }

    fn render_assign_target(&self, target: &AssignTarget) -> String {
        match target {
            AssignTarget::Name(name) => self.render_local_name(name),
            AssignTarget::Member { object, field } => {
                format!("{}.{}", self.render_expr_place(object), field)
            }
        }
    }

    fn render_expr_place(&self, expr: &Expr) -> String {
        match &expr.kind {
            ExprKind::Name(name) => self.render_local_name(name),
            ExprKind::Group(inner) => self.render_expr_place(inner),
            ExprKind::Member { object, field } => {
                format!("{}.{}", self.render_expr_place(object), field)
            }
            _ => "<expr>".to_string(),
        }
    }

    fn scoped_local_name(&self, name: &str) -> Option<&str> {
        self.scoped_names
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).map(String::as_str))
    }

    fn render_local_name(&self, name: &str) -> String {
        self.scoped_local_name(name).unwrap_or(name).to_string()
    }

    fn lower_if(&mut self, if_stmt: &IfStmt) {
        let after_block = self.new_block("if_end");
        let mut next_condition_block = self.current_block;
        let mut else_block_to_lower = None;

        for (index, branch) in if_stmt.branches.iter().enumerate() {
            self.switch_to(next_condition_block);
            let condition = self.lower_expr(&branch.condition);
            let then_block = self.new_block("if_then");
            let is_last = index + 1 == if_stmt.branches.len();
            let else_block = if is_last {
                if if_stmt.else_body.is_some() {
                    let block = self.new_block("if_else");
                    else_block_to_lower = Some(block);
                    block
                } else {
                    after_block
                }
            } else {
                self.new_block("if_next")
            };

            self.terminate(Terminator::Branch {
                condition,
                then_label: self.label(then_block),
                else_label: self.label(else_block),
            });

            self.switch_to(then_block);
            self.lower_stmts(&branch.body);
            if !self.current_terminated() {
                self.terminate(Terminator::Goto(self.label(after_block)));
            }

            next_condition_block = else_block;
        }

        if let (Some(else_body), Some(else_block)) = (&if_stmt.else_body, else_block_to_lower) {
            self.switch_to(else_block);
            self.lower_stmts(else_body);
            if !self.current_terminated() {
                self.terminate(Terminator::Goto(self.label(after_block)));
            }
        }

        self.switch_to(after_block);
    }

    fn lower_match(&mut self, match_stmt: &MatchStmt) {
        let scrutinee = self.lower_expr(&match_stmt.scrutinee);
        let scrutinee_ty = self.infer_expr_type(&match_stmt.scrutinee);
        let after_block = self.new_block("match_end");
        let arms = match_stmt
            .arms
            .iter()
            .map(|arm| {
                let arm_block = self.new_block("match_arm");
                (
                    arm_block,
                    match &arm.pattern {
                        Pattern::Variant(pattern) => MirMatchArm {
                            enum_name: pattern.enum_name.as_deref().map(|enum_name| {
                                self.resolve_enum_info(enum_name)
                                    .map(|enum_info| enum_info.decl.name.clone())
                                    .unwrap_or_else(|| enum_name.to_string())
                            }),
                            variant_name: Some(pattern.variant_name.clone()),
                            wildcard: false,
                            label: self.label(arm_block),
                        },
                        Pattern::Wildcard(_) => MirMatchArm {
                            enum_name: None,
                            variant_name: None,
                            wildcard: true,
                            label: self.label(arm_block),
                        },
                    },
                )
            })
            .collect::<Vec<_>>();

        self.terminate(Terminator::Match {
            scrutinee: scrutinee.clone(),
            arms: arms.iter().map(|(_, arm)| arm.clone()).collect(),
            otherwise: self.label(after_block),
        });

        for ((arm_block, _), arm) in arms.iter().zip(&match_stmt.arms) {
            self.switch_to(*arm_block);
            self.scoped_names.push(std::collections::HashMap::new());
            if let Pattern::Variant(pattern) = &arm.pattern {
                if let Some(binding) = &pattern.binding {
                    let target = if let Some(payload_ty) = scrutinee_ty
                        .as_ref()
                        .and_then(|ty| self.variant_payload_type(ty, &pattern.variant_name))
                    {
                        self.new_typed_temp(payload_ty)
                    } else {
                        self.new_temp()
                    };
                    self.scoped_names
                        .last_mut()
                        .expect("match arm scope should exist")
                        .insert(binding.clone(), target.clone());
                    self.emit(Instruction::Assign {
                        target,
                        value: Rvalue::VariantPayload {
                            scrutinee: scrutinee.clone(),
                        },
                    });
                }
            }
            self.lower_stmts(&arm.body);
            if !self.current_terminated() {
                self.terminate(Terminator::Goto(self.label(after_block)));
            }
            self.scoped_names.pop();
        }

        self.switch_to(after_block);
    }

    fn lower_for(&mut self, for_stmt: &crate::ast::ForStmt) {
        let iterable = self.lower_expr(&for_stmt.iterable);
        let iterable_ty = self.infer_expr_type(&for_stmt.iterable);
        let dispatch_block = self.new_block("for_iter");
        let body_block = self.new_block("for_body");
        let after_block = self.new_block("for_end");

        self.terminate(Terminator::Goto(self.label(dispatch_block)));

        match iterable_ty {
            Some(Type::Named(name, _)) if name == "Range" => {
                self.switch_to(dispatch_block);
                self.terminate(Terminator::ForRange {
                    binding: for_stmt.binding.clone(),
                    iterable,
                    body_label: self.label(body_block),
                    exit_label: self.label(after_block),
                });
            }
            Some(Type::Named(name, args)) if name == "Channel" && args.len() == 1 => {
                let element_ty = args[0].clone();
                let next_value = self
                    .new_typed_temp(Type::Named("Option".to_string(), vec![element_ty.clone()]));
                self.local_types
                    .insert(for_stmt.binding.clone(), element_ty);
                self.switch_to(dispatch_block);
                self.emit(Instruction::Assign {
                    target: next_value.clone(),
                    value: Rvalue::Call {
                        callee: CallTarget::Member {
                            object: iterable.clone(),
                            field: "recv".to_string(),
                            receiver_place: self.render_place_expr_option(&for_stmt.iterable),
                        },
                        args: Vec::new(),
                    },
                });
                self.terminate(Terminator::Match {
                    scrutinee: Operand::Place(next_value.clone()),
                    arms: vec![
                        MirMatchArm {
                            enum_name: Some("Option".to_string()),
                            variant_name: Some("Some".to_string()),
                            wildcard: false,
                            label: self.label(body_block),
                        },
                        MirMatchArm {
                            enum_name: Some("Option".to_string()),
                            variant_name: Some("None".to_string()),
                            wildcard: false,
                            label: self.label(after_block),
                        },
                    ],
                    otherwise: self.label(after_block),
                });
                self.switch_to(body_block);
                self.emit(Instruction::Assign {
                    target: for_stmt.binding.clone(),
                    value: Rvalue::VariantPayload {
                        scrutinee: Operand::Place(next_value),
                    },
                });
            }
            _ => {
                self.switch_to(dispatch_block);
                self.terminate(Terminator::ForRange {
                    binding: for_stmt.binding.clone(),
                    iterable,
                    body_label: self.label(body_block),
                    exit_label: self.label(after_block),
                });
            }
        }

        self.loop_stack.push(LoopLabels {
            break_label: self.label(after_block),
            continue_label: self.label(dispatch_block),
            cleanup_depth: self.with_stack.len(),
        });
        self.switch_to(body_block);
        self.lower_stmts(&for_stmt.body);
        if !self.current_terminated() {
            self.terminate(Terminator::Goto(self.label(dispatch_block)));
        }
        self.loop_stack.pop();

        self.switch_to(after_block);
    }

    fn lower_select(&mut self, select_stmt: &crate::ast::SelectStmt) {
        let after_block = self.new_block("select_end");
        let arms = select_stmt
            .arms
            .iter()
            .map(|arm| {
                let arm_block = self.new_block("select_arm");
                (
                    arm_block,
                    MirSelectArm {
                        binding: arm.binding.clone(),
                        kind: self.lower_select_kind(&arm.expr),
                        label: self.label(arm_block),
                    },
                )
            })
            .collect::<Vec<_>>();

        self.terminate(Terminator::Select {
            arms: arms.iter().map(|(_, arm)| arm.clone()).collect(),
            otherwise: self.label(after_block),
        });

        for ((arm_block, _), arm) in arms.iter().zip(&select_stmt.arms) {
            self.switch_to(*arm_block);
            self.lower_stmts(&arm.body);
            if !self.current_terminated() {
                self.terminate(Terminator::Goto(self.label(after_block)));
            }
        }

        self.switch_to(after_block);
    }

    fn lower_while(&mut self, while_stmt: &WhileStmt) {
        let condition_block = self.new_block("while_cond");
        let body_block = self.new_block("while_body");
        let after_block = self.new_block("while_end");

        self.terminate(Terminator::Goto(self.label(condition_block)));

        self.switch_to(condition_block);
        let condition = self.lower_expr(&while_stmt.condition);
        self.terminate(Terminator::Branch {
            condition,
            then_label: self.label(body_block),
            else_label: self.label(after_block),
        });

        self.loop_stack.push(LoopLabels {
            break_label: self.label(after_block),
            continue_label: self.label(condition_block),
            cleanup_depth: self.with_stack.len(),
        });
        self.switch_to(body_block);
        self.lower_stmts(&while_stmt.body);
        if !self.current_terminated() {
            self.terminate(Terminator::Goto(self.label(condition_block)));
        }
        self.loop_stack.pop();

        self.switch_to(after_block);
    }

    fn lower_select_kind(&mut self, expr: &Expr) -> MirSelectKind {
        let ExprKind::Call { callee, args } = &expr.kind else {
            panic!("select arm should lower from a call expression");
        };

        match &callee.kind {
            ExprKind::Name(name) if name == "after" => MirSelectKind::After {
                duration: self.lower_expr(&args[0].value),
            },
            ExprKind::Member { object, field } if field == "recv" => MirSelectKind::Recv {
                channel: self.lower_expr(object),
            },
            ExprKind::Member { object, field } if field == "send" => MirSelectKind::Send {
                channel: self.lower_expr(object),
                value: self.lower_expr(&args[0].value),
            },
            _ => panic!("unsupported select arm kind"),
        }
    }

    fn lower_expr(&mut self, expr: &Expr) -> Operand {
        match &expr.kind {
            ExprKind::Name(name) if name == "None" => Operand::Unit,
            ExprKind::Name(name) => Operand::Place(self.render_local_name(name)),
            ExprKind::Int(value) => Operand::Int(*value),
            ExprKind::DurationMillis(value) => Operand::Duration(*value),
            ExprKind::Float(value) => Operand::Float(*value),
            ExprKind::Bool(value) => Operand::Bool(*value),
            ExprKind::String(value) => Operand::String(value.clone()),
            ExprKind::FString(parts) => {
                let temp = self.new_typed_temp(Type::named("String"));
                let parts = parts
                    .iter()
                    .map(|part| match part {
                        crate::ast::FormatPart::Literal(text) => {
                            MirFormatPart::Literal(text.clone())
                        }
                        crate::ast::FormatPart::Expr(expr) => {
                            MirFormatPart::Value(self.lower_expr(expr))
                        }
                    })
                    .collect::<Vec<_>>();
                self.emit(Instruction::Assign {
                    target: temp.clone(),
                    value: Rvalue::FormatString { parts },
                });
                Operand::Place(temp)
            }
            ExprKind::Specialize { expr, .. } => self.lower_expr(expr),
            ExprKind::Group(inner) => self.lower_expr(inner),
            ExprKind::Unary { op, expr: value } => {
                let value = self.lower_expr(value);
                let temp = self.new_temp_for_expr(expr);
                self.emit(Instruction::Assign {
                    target: temp.clone(),
                    value: Rvalue::Unary {
                        op: *op,
                        value,
                        span: expr.span,
                    },
                });
                Operand::Place(temp)
            }
            ExprKind::Cast { expr: value, ty } => {
                let value = self.lower_expr(value);
                let temp = self.new_temp_for_expr(expr);
                self.emit(Instruction::Assign {
                    target: temp.clone(),
                    value: Rvalue::Cast {
                        value,
                        ty: lower_type_ref(ty),
                        span: expr.span,
                    },
                });
                Operand::Place(temp)
            }
            ExprKind::Spawn { detached, value } => self.lower_spawn(*detached, value),
            ExprKind::Try(inner) => {
                let value = self.lower_expr(inner);
                let temp = self.new_temp_for_expr(expr);
                self.emit(Instruction::Assign {
                    target: temp.clone(),
                    value: Rvalue::Try { value },
                });
                Operand::Place(temp)
            }
            ExprKind::Binary { op, left, right } => {
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    return self.lower_logical_expr(*op, left, right);
                }
                let left = self.lower_expr(left);
                let right = self.lower_expr(right);
                let temp = self.new_temp_for_expr(expr);
                self.emit(Instruction::Assign {
                    target: temp.clone(),
                    value: Rvalue::Binary {
                        op: *op,
                        left,
                        right,
                        span: expr.span,
                    },
                });
                Operand::Place(temp)
            }
            ExprKind::Member { object, field } => {
                if let Some((module_path, enum_name)) = self.qualified_module_item(object) {
                    if let Some(namespace) = self.module_namespace(&module_path) {
                        if let Some(enum_info) = namespace.enums.get(&enum_name).cloned() {
                            let temp = self.new_temp_for_expr(expr);
                            self.emit(Instruction::Assign {
                                target: temp.clone(),
                                value: Rvalue::EnumVariant {
                                    enum_name: enum_info.decl.name.clone(),
                                    variant_name: field.clone(),
                                    payload: None,
                                },
                            });
                            return Operand::Place(temp);
                        }
                    }
                }
                let base_object = match &object.kind {
                    ExprKind::Specialize { expr, .. } => &**expr,
                    _ => object,
                };
                if let ExprKind::Name(enum_name) = &base_object.kind {
                    if is_known_enum_name(self.program, enum_name) {
                        let temp = self.new_temp_for_expr(expr);
                        self.emit(Instruction::Assign {
                            target: temp.clone(),
                            value: Rvalue::EnumVariant {
                                enum_name: enum_name.clone(),
                                variant_name: field.clone(),
                                payload: None,
                            },
                        });
                        return Operand::Place(temp);
                    }
                }

                let object = self.lower_expr(object);
                let temp = self.new_temp_for_expr(expr);
                self.emit(Instruction::Assign {
                    target: temp.clone(),
                    value: Rvalue::Member {
                        object,
                        field: field.clone(),
                    },
                });
                Operand::Place(temp)
            }
            ExprKind::Call { callee, args } => self.lower_call(expr, callee, args),
        }
    }

    fn lower_logical_expr(&mut self, op: BinaryOp, left: &Expr, right: &Expr) -> Operand {
        let result = self.new_typed_temp(Type::named("bool"));
        let rhs_block = self.new_block("logic_rhs");
        let short_block = self.new_block("logic_short");
        let join_block = self.new_block("logic_join");
        let left_value = self.lower_expr(left);

        let (then_label, else_label) = match op {
            BinaryOp::And => (self.label(rhs_block), self.label(short_block)),
            BinaryOp::Or => (self.label(short_block), self.label(rhs_block)),
            _ => unreachable!("logical lowering only handles `and` / `or`"),
        };

        self.terminate(Terminator::Branch {
            condition: left_value,
            then_label,
            else_label,
        });

        self.switch_to(short_block);
        self.emit(Instruction::Assign {
            target: result.clone(),
            value: Rvalue::Use(Operand::Bool(matches!(op, BinaryOp::Or))),
        });
        self.terminate(Terminator::Goto(self.label(join_block)));

        self.switch_to(rhs_block);
        let right_value = self.lower_expr(right);
        self.emit(Instruction::Assign {
            target: result.clone(),
            value: Rvalue::Use(right_value),
        });
        self.terminate(Terminator::Goto(self.label(join_block)));

        self.switch_to(join_block);
        Operand::Place(result)
    }

    fn lower_spawn(&mut self, detached: bool, value: &Expr) -> Operand {
        let ExprKind::Call { callee, args } = &value.kind else {
            panic!("spawn should lower from a call expression");
        };
        let ExprKind::Name(function) = &callee.kind else {
            panic!("spawn should lower from a named function call");
        };

        let lowered_args = self
            .program
            .functions
            .get(function)
            .map(|function_info| {
                self.lower_user_args(
                    &format!("function `{}`", function),
                    &function_info.decl.params,
                    args,
                    callee.span,
                )
            })
            .unwrap_or_else(|| self.lower_args(args));
        let temp = self.new_temp();
        self.emit(Instruction::Assign {
            target: temp.clone(),
            value: Rvalue::Spawn {
                detached,
                task_group: None,
                function: function.clone(),
                args: lowered_args,
            },
        });
        Operand::Place(temp)
    }

    fn lower_call(&mut self, expr: &Expr, callee: &Expr, args: &[Argument]) -> Operand {
        let temp = self.new_temp_for_expr(expr);
        let base_callee = match &callee.kind {
            ExprKind::Specialize { expr, .. } => &**expr,
            _ => callee,
        };

        match &base_callee.kind {
            ExprKind::Name(name) if self.resolve_class_info(name).is_some() => {
                let class = self
                    .resolve_class_info(name)
                    .expect("class should exist during MIR lowering")
                    .clone();
                let fields = class
                    .decl
                    .fields
                    .iter()
                    .filter_map(|field| {
                        if let Some(argument) = args
                            .iter()
                            .find(|argument| argument.name.as_deref() == Some(field.name.as_str()))
                        {
                            Some(MirFieldInit {
                                name: field.name.clone(),
                                value: self.lower_expr(&argument.value),
                            })
                        } else {
                            field.default.as_ref().map(|default| MirFieldInit {
                                name: field.name.clone(),
                                value: self.lower_expr(default),
                            })
                        }
                    })
                    .collect::<Vec<_>>();
                self.emit(Instruction::Assign {
                    target: temp.clone(),
                    value: Rvalue::Construct {
                        class_name: name.clone(),
                        fields,
                    },
                });
            }
            ExprKind::Name(name) if name == "Channel" => {
                let lowered_args = self.lower_args(args);
                self.emit(Instruction::Assign {
                    target: temp.clone(),
                    value: Rvalue::Call {
                        callee: CallTarget::Name("channel".to_string()),
                        args: lowered_args,
                    },
                });
            }
            ExprKind::Member { object, field } => {
                let base_object = match &object.kind {
                    ExprKind::Specialize { expr, .. } => &**expr,
                    _ => object,
                };
                if let Some((module_path, item_name)) = self.qualified_module_item(object) {
                    if let Some(namespace) = self.module_namespace(&module_path) {
                        if let Some(class) = namespace.classes.get(&item_name).cloned() {
                            if class
                                .methods
                                .get(field)
                                .is_some_and(|method| method.decl.receiver.is_none())
                            {
                                let method = class.methods.get(field).unwrap();
                                let lowered_args = self.lower_user_args(
                                    &format!("method `{}`", field),
                                    &method.decl.params,
                                    args,
                                    callee.span,
                                );
                                self.emit(Instruction::Assign {
                                    target: temp.clone(),
                                    value: Rvalue::Call {
                                        callee: CallTarget::Name(format!(
                                            "{}.{}",
                                            class.decl.name, field
                                        )),
                                        args: lowered_args,
                                    },
                                });
                                return Operand::Place(temp);
                            }
                        }
                        if let Some(enum_info) = namespace.enums.get(&item_name).cloned() {
                            let payload = args
                                .first()
                                .map(|argument| self.lower_expr(&argument.value));
                            self.emit(Instruction::Assign {
                                target: temp.clone(),
                                value: Rvalue::EnumVariant {
                                    enum_name: enum_info.decl.name.clone(),
                                    variant_name: field.clone(),
                                    payload,
                                },
                            });
                            return Operand::Place(temp);
                        }
                    }
                }
                if let Some(module_path) = self.infer_module_path(object) {
                    if let Some(namespace) = self.module_namespace(&module_path) {
                        if let Some(function) = namespace.functions.get(field).cloned() {
                            let lowered_args = self.lower_user_args(
                                &format!("function `{}`", function.decl.name),
                                &function.decl.params,
                                args,
                                callee.span,
                            );
                            self.emit(Instruction::Assign {
                                target: temp.clone(),
                                value: Rvalue::Call {
                                    callee: CallTarget::Name(imported_module_function_name(
                                        &module_path,
                                        field,
                                    )),
                                    args: lowered_args,
                                },
                            });
                            return Operand::Place(temp);
                        }
                        if let Some(class) = namespace.classes.get(field).cloned() {
                            let fields = class
                                .decl
                                .fields
                                .iter()
                                .filter_map(|field_decl| {
                                    if let Some(argument) = args.iter().find(|argument| {
                                        argument.name.as_deref() == Some(field_decl.name.as_str())
                                    }) {
                                        Some(MirFieldInit {
                                            name: field_decl.name.clone(),
                                            value: self.lower_expr(&argument.value),
                                        })
                                    } else {
                                        field_decl.default.as_ref().map(|default| MirFieldInit {
                                            name: field_decl.name.clone(),
                                            value: self.lower_expr(default),
                                        })
                                    }
                                })
                                .collect::<Vec<_>>();
                            self.emit(Instruction::Assign {
                                target: temp.clone(),
                                value: Rvalue::Construct {
                                    class_name: class.decl.name.clone(),
                                    fields,
                                },
                            });
                            return Operand::Place(temp);
                        }
                    }
                }

                if field == "spawn" {
                    let function = match &args[0].value.kind {
                        ExprKind::Name(function) => function.clone(),
                        _ => panic!("task-group spawn should lower from a named function target"),
                    };
                    let group = self.lower_expr(object);
                    let lowered_args = if let Some(function_info) =
                        self.resolve_function_info(&function).cloned()
                    {
                        self.lower_user_args(
                            &format!("function `{}`", function),
                            &function_info.decl.params,
                            &args[1..],
                            callee.span,
                        )
                    } else {
                        self.lower_args(&args[1..])
                    };
                    self.emit(Instruction::Assign {
                        target: temp.clone(),
                        value: Rvalue::Spawn {
                            detached: false,
                            task_group: Some(group),
                            function,
                            args: lowered_args,
                        },
                    });
                    return Operand::Place(temp);
                }

                if let ExprKind::Name(class_name) = &base_object.kind {
                    if let Some(class) = self.resolve_class_info(class_name).cloned() {
                        if class
                            .methods
                            .get(field)
                            .is_some_and(|method| method.decl.receiver.is_none())
                        {
                            let method = class.methods.get(field).unwrap();
                            let lowered_args = self.lower_user_args(
                                &format!("method `{}`", field),
                                &method.decl.params,
                                args,
                                callee.span,
                            );
                            self.emit(Instruction::Assign {
                                target: temp.clone(),
                                value: Rvalue::Call {
                                    callee: CallTarget::Name(format!("{}.{}", class_name, field)),
                                    args: lowered_args,
                                },
                            });
                            return Operand::Place(temp);
                        }
                    }
                    if let Some((function_name, params)) = self
                        .trait_impl_method_for_class_name(class_name, field)
                        .filter(|(_, method)| method.decl.receiver.is_none())
                        .map(|(trait_impl, method)| {
                            (
                                format!(
                                    "{}{} for {}.{}",
                                    trait_impl.trait_name,
                                    format_trait_args(&trait_impl.trait_args),
                                    trait_impl.for_type,
                                    field
                                ),
                                method.decl.params.clone(),
                            )
                        })
                    {
                        let lowered_args = self.lower_user_args(
                            &format!("method `{}`", field),
                            &params,
                            args,
                            callee.span,
                        );
                        self.emit(Instruction::Assign {
                            target: temp.clone(),
                            value: Rvalue::Call {
                                callee: CallTarget::Name(function_name),
                                args: lowered_args,
                            },
                        });
                        return Operand::Place(temp);
                    }
                }

                if let ExprKind::Name(enum_name) = &base_object.kind {
                    if is_known_enum_name(self.program, enum_name)
                        || self.resolve_enum_info(enum_name).is_some()
                    {
                        let payload = args
                            .first()
                            .map(|argument| self.lower_expr(&argument.value));
                        self.emit(Instruction::Assign {
                            target: temp.clone(),
                            value: Rvalue::EnumVariant {
                                enum_name: enum_name.clone(),
                                variant_name: field.clone(),
                                payload,
                            },
                        });
                        return Operand::Place(temp);
                    }
                }

                let receiver_place = self.render_place_expr_option(object);
                let lowered_args = self.lower_member_call_args(callee.span, object, field, args);
                let object = self.lower_expr(object);
                self.emit(Instruction::Assign {
                    target: temp.clone(),
                    value: Rvalue::Call {
                        callee: CallTarget::Member {
                            object,
                            field: field.clone(),
                            receiver_place,
                        },
                        args: lowered_args,
                    },
                });
            }
            ExprKind::Name(name) => {
                let resolved_function = self.resolve_function_info(name).cloned();
                let lowered_args = if let Some(function_info) = resolved_function.as_ref() {
                    self.lower_user_args(
                        &format!("function `{}`", name),
                        &function_info.decl.params,
                        args,
                        callee.span,
                    )
                } else {
                    self.lower_args(args)
                };
                let callee_name = if self.program.functions.contains_key(name) {
                    name.clone()
                } else if let Some(function_info) = resolved_function {
                    if function_info.module_name == self.program.module_name {
                        name.clone()
                    } else {
                        imported_module_function_name(
                            &function_info.module_name,
                            &function_info.decl.name,
                        )
                    }
                } else {
                    name.clone()
                };
                self.emit(Instruction::Assign {
                    target: temp.clone(),
                    value: Rvalue::Call {
                        callee: CallTarget::Name(callee_name),
                        args: lowered_args,
                    },
                });
            }
            other => {
                let fallback = format!("unsupported<{:?}>", other);
                let lowered_args = self.lower_args(args);
                self.emit(Instruction::Assign {
                    target: temp.clone(),
                    value: Rvalue::Call {
                        callee: CallTarget::Name(fallback),
                        args: lowered_args,
                    },
                });
            }
        }

        Operand::Place(temp)
    }

    fn lower_member_call_args(
        &mut self,
        span: Span,
        object_expr: &Expr,
        field: &str,
        args: &[Argument],
    ) -> Vec<MirArg> {
        let Some(receiver_type) = self.infer_expr_type(object_expr) else {
            return self.lower_args(args);
        };

        if let Type::Named(class_name, _) = &receiver_type {
            if let Some(class) = self.resolve_class_info(class_name).cloned() {
                if let Some(method) = class
                    .methods
                    .get(field)
                    .filter(|method| method.decl.receiver.is_some())
                {
                    return self.lower_user_args(
                        &format!("method `{}`", field),
                        &method.decl.params,
                        args,
                        span,
                    );
                }
            }
        }

        let trait_method_params = self
            .trait_method_for_receiver(&receiver_type, field)
            .map(|(method, _)| method.decl.params.clone());

        if let Some(params) = trait_method_params {
            return self.lower_user_args(&format!("method `{}`", field), &params, args, span);
        }

        self.lower_args(args)
    }

    fn lower_user_args(
        &mut self,
        callee_name: &str,
        params: &[crate::ast::Param],
        args: &[Argument],
        span: Span,
    ) -> Vec<MirArg> {
        let ordered_args = bind_call_arguments(
            callee_name,
            &callable_params_from_decl(params),
            args,
            span,
            CallConvention::PositionalOrNamed,
        )
        .expect("type-checked user-defined call should bind during MIR lowering");

        ordered_args
            .into_iter()
            .zip(params.iter())
            .map(|(argument, param)| MirArg {
                name: None,
                value: self.lower_expr(argument.map(|argument| &argument.value).unwrap_or_else(
                    || {
                        param
                            .default
                            .as_ref()
                            .expect("optional parameter should provide a default expression")
                    },
                )),
                writeback_place: argument.and_then(|argument| {
                    if param.passing == crate::ast::ReceiverKind::BorrowMut {
                        self.render_place_expr_option(&argument.value)
                    } else {
                        None
                    }
                }),
            })
            .collect()
    }

    fn lower_args(&mut self, args: &[Argument]) -> Vec<MirArg> {
        args.iter()
            .map(|argument| MirArg {
                name: argument.name.clone(),
                value: self.lower_expr(&argument.value),
                writeback_place: None,
            })
            .collect()
    }

    fn builtin_enum_variant_type(&self, receiver_type: &Type, field: &str) -> Option<Type> {
        match receiver_type {
            Type::Named(name, args) if name == "Option" && args.len() == 1 => {
                matches!(field, "Some" | "None").then(|| receiver_type.clone())
            }
            Type::Named(name, args) if name == "Result" && args.len() == 2 => {
                matches!(field, "Ok" | "Err").then(|| receiver_type.clone())
            }
            Type::Named(name, args) if name == "SendError" && args.len() == 1 => {
                (field == "Closed").then(|| receiver_type.clone())
            }
            _ => None,
        }
    }

    fn infer_expr_type(&self, expr: &Expr) -> Option<Type> {
        match &expr.kind {
            ExprKind::Name(name) if name == "None" => Some(Type::Unit),
            ExprKind::Name(name) => {
                if let Some(mapped) = self.scoped_local_name(name) {
                    return self.local_types.get(mapped).cloned();
                }
                self.local_types
                    .get(name)
                    .cloned()
                    .or_else(|| {
                        self.program
                            .imported_modules
                            .get(name)
                            .map(|namespace| Type::Module(namespace.path.clone()))
                    })
                    .or_else(|| self.resolve_class_info(name).map(|_| Type::named(name)))
                    .or_else(|| self.resolve_enum_info(name).map(|_| Type::named(name)))
                    .or_else(|| {
                        self.resolve_function_info(name)
                            .map(|function| function.signature.return_type.clone())
                    })
            }
            ExprKind::Group(inner) => self.infer_expr_type(inner),
            ExprKind::Cast { ty, .. } => Some(lower_type_ref(ty)),
            ExprKind::Int(_) => Some(Type::named("int32")),
            ExprKind::Float(_) => Some(Type::named("float64")),
            ExprKind::Bool(_) => Some(Type::named("bool")),
            ExprKind::String(_) => Some(Type::named("String")),
            ExprKind::FString(_) => Some(Type::named("String")),
            ExprKind::Specialize { expr, type_args } => match &expr.kind {
                ExprKind::Name(name)
                    if matches!(name.as_str(), "Option" | "Result" | "SendError" | "Channel") =>
                {
                    Some(Type::Named(
                        name.clone(),
                        type_args.iter().map(lower_type_ref).collect(),
                    ))
                }
                _ => self.infer_expr_type(expr),
            },
            ExprKind::DurationMillis(_) => Some(Type::named("Duration")),
            ExprKind::Unary { op, expr } => match op {
                UnaryOp::Not => Some(Type::named("bool")),
                UnaryOp::Neg => match &expr.kind {
                    ExprKind::Int(value) => Some(minimal_signed_type_for_negative_literal(*value)),
                    _ => self.infer_expr_type(expr),
                },
            },
            ExprKind::Try(inner) => match self.infer_expr_type(inner)? {
                Type::Named(name, mut args) if name == "Result" && args.len() == 2 => {
                    Some(args.remove(0))
                }
                _ => None,
            },
            ExprKind::Spawn { detached, value } => {
                if *detached {
                    Some(Type::Unit)
                } else {
                    self.infer_expr_type(value)
                        .map(|inner| Type::Named("Task".to_string(), vec![inner]))
                }
            }
            ExprKind::Call { callee, .. } => {
                let (base_callee, explicit_type_args) = match &callee.kind {
                    ExprKind::Specialize { expr, type_args } => {
                        (&**expr, Some(type_args.as_slice()))
                    }
                    _ => (&**callee, None),
                };
                match &base_callee.kind {
                    ExprKind::Name(name) => {
                        if name == "Channel" {
                            return explicit_type_args.map(|type_args| {
                                Type::Named(
                                    "Channel".to_string(),
                                    type_args.iter().map(lower_type_ref).collect(),
                                )
                            });
                        }
                        if self.resolve_class_info(name).is_some() {
                            return Some(match explicit_type_args {
                                Some(type_args) => Type::Named(
                                    name.clone(),
                                    type_args.iter().map(lower_type_ref).collect(),
                                ),
                                None => Type::named(name),
                            });
                        }
                        self.resolve_function_info(name).map(|function| {
                            if let Some(type_args) = explicit_type_args {
                                let substitutions = function
                                    .decl
                                    .type_params
                                    .iter()
                                    .cloned()
                                    .zip(type_args.iter().map(lower_type_ref))
                                    .collect();
                                substitute_type(&function.signature.return_type, &substitutions)
                            } else {
                                function.signature.return_type.clone()
                            }
                        })
                    }
                    ExprKind::Member { object, field } => {
                        let receiver_type = match &object.kind {
                            ExprKind::Specialize { expr, type_args }
                                if matches!(&expr.kind, ExprKind::Name(_)) =>
                            {
                                let inner_name = match &expr.kind {
                                    ExprKind::Name(name) => name,
                                    _ => unreachable!(),
                                };
                                Some(Type::Named(
                                    inner_name.clone(),
                                    type_args.iter().map(lower_type_ref).collect(),
                                ))
                            }
                            _ => self.infer_expr_type(object),
                        };
                        if let Some((module_path, item_name)) = self.qualified_module_item(object) {
                            if let Some(namespace) = self.module_namespace(&module_path) {
                                if let Some(class) = namespace.classes.get(&item_name) {
                                    if let Some(method) = class.methods.get(field) {
                                        return Some(method.signature.return_type.clone());
                                    }
                                }
                                if let Some(enum_info) = namespace.enums.get(&item_name) {
                                    if enum_info.variants.contains_key(field) {
                                        return Some(Type::named(enum_info.decl.name.clone()));
                                    }
                                }
                            }
                        }
                        if let Some(module_path) = self.infer_module_path(object) {
                            if let Some(namespace) = self.module_namespace(&module_path) {
                                if let Some(child) = namespace.modules.get(field) {
                                    return Some(Type::Module(child.path.clone()));
                                }
                                if let Some(function) = namespace.functions.get(field) {
                                    return Some(function.signature.return_type.clone());
                                }
                                if let Some(class) = namespace.classes.get(field) {
                                    return Some(Type::named(class.decl.name.clone()));
                                }
                                if let Some(enum_info) = namespace.enums.get(field) {
                                    return Some(Type::named(enum_info.decl.name.clone()));
                                }
                            }
                        }
                        let receiver_type = receiver_type?;
                        if let Some(enum_ty) = self.builtin_enum_variant_type(&receiver_type, field)
                        {
                            return Some(enum_ty);
                        }
                        if let Type::Named(class_name, _) = &receiver_type {
                            if let Some(class) = self.resolve_class_info(class_name) {
                                if let Some(method) = class.methods.get(field) {
                                    return Some(method.signature.return_type.clone());
                                }
                            }
                        }
                        if let Some(runtime_ty) =
                            self.builtin_runtime_member_return_type(&receiver_type, field)
                        {
                            return Some(runtime_ty);
                        }
                        self.trait_method_for_receiver(&receiver_type, field).map(
                            |(method, substitutions)| {
                                substitute_type(&method.signature.return_type, &substitutions)
                            },
                        )
                    }
                    _ => None,
                }
            }
            ExprKind::Member { object, field } => {
                if let Some((module_path, item_name)) = self.qualified_module_item(object) {
                    if let Some(namespace) = self.module_namespace(&module_path) {
                        if let Some(enum_info) = namespace.enums.get(&item_name) {
                            if enum_info.variants.contains_key(field) {
                                return Some(Type::named(enum_info.decl.name.clone()));
                            }
                        }
                    }
                }
                let receiver_type = match &object.kind {
                    ExprKind::Specialize { expr, type_args }
                        if matches!(&expr.kind, ExprKind::Name(_)) =>
                    {
                        let inner_name = match &expr.kind {
                            ExprKind::Name(name) => name,
                            _ => unreachable!(),
                        };
                        Type::Named(
                            inner_name.clone(),
                            type_args.iter().map(lower_type_ref).collect(),
                        )
                    }
                    _ => self.infer_expr_type(object)?,
                };
                if let Some(enum_ty) = self.builtin_enum_variant_type(&receiver_type, field) {
                    return Some(enum_ty);
                }
                let Type::Named(class_name, _) = receiver_type else {
                    return None;
                };
                let class = self.resolve_class_info(&class_name)?;
                class.fields.get(field).map(|field| field.ty.clone())
            }
            ExprKind::Binary { op, left, right } => {
                if matches!(
                    op,
                    BinaryOp::Eq
                        | BinaryOp::NotEq
                        | BinaryOp::Less
                        | BinaryOp::LessEq
                        | BinaryOp::Greater
                        | BinaryOp::GreaterEq
                        | BinaryOp::And
                        | BinaryOp::Or
                ) {
                    return Some(Type::named("bool"));
                }
                let left_ty = self.infer_expr_type(left)?;
                let right_ty = self.infer_expr_type(right)?;
                if left_ty == Type::named("float64") || right_ty == Type::named("float64") {
                    Some(Type::named("float64"))
                } else if left_ty == right_ty {
                    Some(left_ty)
                } else {
                    None
                }
            }
        }
    }

    fn trait_impls_in_scope(&self) -> impl Iterator<Item = &crate::sema::TraitImplInfo> + '_ {
        self.program.trait_impls.iter().chain(
            self.program
                .module_registry
                .values()
                .flat_map(|namespace| namespace.trait_impls.iter()),
        )
    }

    fn trait_impl_substitutions(
        &self,
        trait_impl: &crate::sema::TraitImplInfo,
        actual: &Type,
    ) -> Option<std::collections::HashMap<String, Type>> {
        let type_params = trait_impl
            .type_params
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut substitutions = std::collections::HashMap::new();
        if !crate::sema::type_pattern_matches(
            &trait_impl.for_type,
            actual,
            &type_params,
            &mut substitutions,
        ) {
            return None;
        }
        Some(substitutions)
    }

    fn trait_method_for_receiver(
        &self,
        receiver_type: &Type,
        field: &str,
    ) -> Option<(
        &crate::sema::TraitImplMethodInfo,
        std::collections::HashMap<String, Type>,
    )> {
        self.trait_impls_in_scope()
            .filter_map(|trait_impl| {
                self.trait_impl_substitutions(trait_impl, receiver_type)
                    .map(|substitutions| (trait_impl, substitutions))
            })
            .find_map(|(trait_impl, substitutions)| {
                trait_impl
                    .methods
                    .get(field)
                    .map(|method| (method, substitutions))
            })
    }

    fn trait_impl_method_for_class_name(
        &self,
        class_name: &str,
        field: &str,
    ) -> Option<(crate::sema::TraitImplInfo, crate::sema::TraitImplMethodInfo)> {
        let mut matches =
            self.trait_impls_in_scope()
                .filter_map(|trait_impl| match &trait_impl.for_type {
                    Type::Named(name, _) if name == class_name => trait_impl
                        .methods
                        .get(field)
                        .cloned()
                        .map(|method| (trait_impl.clone(), method)),
                    _ => None,
                });
        let first = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        Some(first)
    }

    fn variant_payload_type(&self, enum_ty: &Type, variant_name: &str) -> Option<Type> {
        match enum_ty {
            Type::Named(name, args) if name == "Option" && args.len() == 1 => {
                (variant_name == "Some").then(|| args[0].clone())
            }
            Type::Named(name, args) if name == "Result" && args.len() == 2 => match variant_name {
                "Ok" => Some(args[0].clone()),
                "Err" => Some(args[1].clone()),
                _ => None,
            },
            Type::Named(name, args) if name == "SendError" && args.len() == 1 => {
                (variant_name == "Closed").then(|| args[0].clone())
            }
            Type::Named(name, args) => {
                let enum_info = self.resolve_enum_info(name)?;
                let payload = enum_info.variants.get(variant_name)?.payload.as_ref()?;
                let substitutions = enum_info
                    .decl
                    .type_params
                    .iter()
                    .cloned()
                    .zip(args.iter().cloned())
                    .collect::<std::collections::HashMap<_, _>>();
                Some(substitute_type(payload, &substitutions))
            }
            _ => None,
        }
    }

    fn builtin_runtime_member_return_type(
        &self,
        receiver_type: &Type,
        field: &str,
    ) -> Option<Type> {
        let Type::Named(name, args) = receiver_type else {
            return None;
        };
        match (name.as_str(), field) {
            ("Channel", "clone") => Some(Type::Named("Channel".to_string(), args.clone())),
            ("Channel", "recv") => Some(Type::Named(
                "Option".to_string(),
                vec![args
                    .first()
                    .cloned()
                    .unwrap_or_else(|| Type::named("Unknown"))],
            )),
            ("Channel", "send") => Some(Type::Named(
                "Result".to_string(),
                vec![
                    Type::Unit,
                    Type::Named(
                        "SendError".to_string(),
                        vec![args
                            .first()
                            .cloned()
                            .unwrap_or_else(|| Type::named("Unknown"))],
                    ),
                ],
            )),
            ("Channel", "close") | ("TaskGroup", "cancel") | ("TaskGroup", "close") => {
                Some(Type::Unit)
            }
            ("Task", "clone") => Some(Type::Named("Task".to_string(), args.clone())),
            ("Task", "join") => Some(args.first().cloned().unwrap_or(Type::Unit)),
            _ => None,
        }
    }

    fn render_place_expr_option(&self, expr: &Expr) -> Option<String> {
        match &expr.kind {
            ExprKind::Name(_) | ExprKind::Group(_) | ExprKind::Member { .. } => {
                let rendered = self.render_expr_place(expr);
                if rendered == "<expr>" {
                    None
                } else {
                    Some(rendered)
                }
            }
            _ => None,
        }
    }

    fn emit(&mut self, instruction: Instruction) {
        self.blocks[self.current_block]
            .instructions
            .push(instruction);
    }

    fn emit_cleanup_range(&mut self, depth: usize, cancel_before_cleanup: bool) {
        let places = self.with_stack[depth..].to_vec();
        for place in places.into_iter().rev() {
            self.emit(Instruction::PopCleanup {
                place,
                cancel_before_cleanup,
            });
        }
    }

    fn terminate(&mut self, terminator: Terminator) {
        self.blocks[self.current_block].terminator = Some(terminator);
    }

    fn current_terminated(&self) -> bool {
        self.blocks[self.current_block].terminator.is_some()
    }

    fn new_temp(&mut self) -> String {
        let name = format!("%t{}", self.temp_counter);
        self.temp_counter += 1;
        name
    }

    fn new_typed_temp(&mut self, ty: Type) -> String {
        let name = self.new_temp();
        self.local_types.insert(name.clone(), ty);
        name
    }

    fn new_temp_for_expr(&mut self, expr: &Expr) -> String {
        if let Some(ty) = self.infer_expr_type(expr) {
            self.new_typed_temp(ty)
        } else {
            self.new_temp()
        }
    }

    fn new_block(&mut self, prefix: &str) -> usize {
        let suffix = self.block_counter;
        self.block_counter += 1;
        self.blocks.push(BasicBlockBuilder {
            label: format!("{}_{}_{}", self.function_name, prefix, suffix),
            instructions: Vec::new(),
            terminator: None,
        });
        self.blocks.len() - 1
    }

    fn label(&self, block_index: usize) -> String {
        self.blocks[block_index].label.clone()
    }

    fn switch_to(&mut self, block_index: usize) {
        self.current_block = block_index;
    }
}

fn lower_type_ref(type_ref: &crate::ast::TypeRef) -> Type {
    if type_ref.name == "None" {
        return Type::Unit;
    }
    let name = if type_ref.name == "str" {
        "String"
    } else {
        &type_ref.name
    };
    Type::Named(
        name.to_string(),
        type_ref.args.iter().map(lower_type_ref).collect(),
    )
}
