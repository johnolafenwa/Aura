use std::collections::{BTreeMap, HashMap};
use std::fmt;

use crate::ast::{
    Argument, AssignStmt, BinaryOp, ClassDecl, Expr, ExprKind, FunctionDecl, Item, Module, Stmt,
    TypeRef,
};
use crate::diag::{Diagnostic, Result};

#[derive(Clone, Debug)]
pub struct Program {
    pub module: Module,
    pub classes: BTreeMap<String, ClassInfo>,
    pub functions: BTreeMap<String, FunctionInfo>,
}

#[derive(Clone, Debug)]
pub struct ClassInfo {
    pub decl: ClassDecl,
    pub fields: BTreeMap<String, FieldInfo>,
}

#[derive(Clone, Debug)]
pub struct FieldInfo {
    pub public: bool,
    pub ty: Type,
}

#[derive(Clone, Debug)]
pub struct FunctionInfo {
    pub decl: FunctionDecl,
    pub signature: FunctionSignature,
}

#[derive(Clone, Debug)]
pub struct FunctionSignature {
    pub params: Vec<Type>,
    pub return_type: Type,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Type {
    Named(String, Vec<Type>),
    Unit,
}

impl Type {
    pub fn named(name: impl Into<String>) -> Self {
        Self::Named(name.into(), Vec::new())
    }

    pub fn is_copy(&self) -> bool {
        match self {
            Type::Unit => true,
            Type::Named(name, args) => {
                args.is_empty()
                    && matches!(
                        name.as_str(),
                        "bool"
                            | "i8"
                            | "i16"
                            | "i32"
                            | "i64"
                            | "i128"
                            | "isize"
                            | "u8"
                            | "u16"
                            | "u32"
                            | "u64"
                            | "u128"
                            | "usize"
                            | "f32"
                            | "f64"
                            | "None"
                    )
            }
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Unit => write!(f, "Unit"),
            Type::Named(name, args) if args.is_empty() => write!(f, "{}", name),
            Type::Named(name, args) => {
                write!(f, "{}[", name)?;
                for (index, arg) in args.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, "]")
            }
        }
    }
}

pub fn check(module: Module) -> Result<Program> {
    let mut class_names = BTreeMap::<String, crate::diag::Span>::new();
    let mut function_names = BTreeMap::<String, crate::diag::Span>::new();

    for item in &module.items {
        match item {
            Item::Class(class_decl) => {
                if let Some(existing) = class_names.insert(class_decl.name.clone(), class_decl.span)
                {
                    return Err(Diagnostic::at(
                        class_decl.span,
                        format!(
                            "duplicate class `{}` (previously declared at {})",
                            class_decl.name, existing
                        ),
                    ));
                }
            }
            Item::Function(function_decl) => {
                if let Some(existing) =
                    function_names.insert(function_decl.name.clone(), function_decl.span)
                {
                    return Err(Diagnostic::at(
                        function_decl.span,
                        format!(
                            "duplicate function `{}` (previously declared at {})",
                            function_decl.name, existing
                        ),
                    ));
                }
            }
        }
    }

    let mut classes = BTreeMap::new();
    let empty_functions = BTreeMap::new();
    for item in &module.items {
        let Item::Class(class_decl) = item else {
            continue;
        };
        let mut fields = BTreeMap::new();
        for field in &class_decl.fields {
            let lowered = lower_type(&field.ty, &class_names)?;
            if fields
                .insert(
                    field.name.clone(),
                    FieldInfo {
                        public: field.public,
                        ty: lowered.clone(),
                    },
                )
                .is_some()
            {
                return Err(Diagnostic::at(
                    field.span,
                    format!(
                        "duplicate field `{}` in class `{}`",
                        field.name, class_decl.name
                    ),
                ));
            }

            if let Some(default) = &field.default {
                let checker = FunctionChecker::new(&class_names, &classes, &empty_functions);
                let default_ty = checker.type_of_expr(default, &mut HashMap::new())?;
                if default_ty != lowered {
                    return Err(Diagnostic::at(
                        field.span,
                        format!(
                            "default value for field `{}` has type `{}`, expected `{}`",
                            field.name, default_ty, lowered
                        ),
                    ));
                }
            }
        }

        classes.insert(
            class_decl.name.clone(),
            ClassInfo {
                decl: class_decl.clone(),
                fields,
            },
        );
    }

    let mut functions = BTreeMap::new();
    for item in &module.items {
        let Item::Function(function_decl) = item else {
            continue;
        };
        let params = function_decl
            .params
            .iter()
            .map(|param| lower_type(&param.ty, &class_names))
            .collect::<Result<Vec<_>>>()?;
        let return_type = lower_type(&function_decl.return_type, &class_names)?;
        functions.insert(
            function_decl.name.clone(),
            FunctionInfo {
                decl: function_decl.clone(),
                signature: FunctionSignature {
                    params,
                    return_type,
                },
            },
        );
    }

    let program = Program {
        module,
        classes,
        functions,
    };

    let checker = FunctionChecker::new(&class_names, &program.classes, &program.functions);
    for function in program.functions.values() {
        checker.check_function(&function.decl)?;
    }

    Ok(program)
}

fn lower_type(
    type_ref: &TypeRef,
    class_names: &BTreeMap<String, crate::diag::Span>,
) -> Result<Type> {
    let args = type_ref
        .args
        .iter()
        .map(|arg| lower_type(arg, class_names))
        .collect::<Result<Vec<_>>>()?;

    if is_builtin_type(&type_ref.name) || class_names.contains_key(&type_ref.name) {
        Ok(Type::Named(type_ref.name.clone(), args))
    } else {
        Err(Diagnostic::at(
            type_ref.span,
            format!("unknown type `{}`", type_ref.name),
        ))
    }
}

fn is_builtin_type(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "f32"
            | "f64"
            | "String"
            | "None"
    )
}

struct LocalBinding {
    ty: Type,
    mutable: bool,
}

struct FunctionChecker<'a> {
    class_names: &'a BTreeMap<String, crate::diag::Span>,
    classes: &'a BTreeMap<String, ClassInfo>,
    functions: &'a BTreeMap<String, FunctionInfo>,
}

impl<'a> FunctionChecker<'a> {
    fn new(
        class_names: &'a BTreeMap<String, crate::diag::Span>,
        classes: &'a BTreeMap<String, ClassInfo>,
        functions: &'a BTreeMap<String, FunctionInfo>,
    ) -> Self {
        Self {
            class_names,
            classes,
            functions,
        }
    }

    fn check_function(&self, function: &FunctionDecl) -> Result<()> {
        let return_type = lower_type(&function.return_type, self.class_names)?;
        let mut locals = HashMap::new();
        for param in &function.params {
            let ty = lower_type(&param.ty, self.class_names)?;
            locals.insert(param.name.clone(), LocalBinding { ty, mutable: false });
        }

        let mut saw_return = false;
        for stmt in &function.body {
            match stmt {
                Stmt::Assign(assign) => self.check_assign(assign, &mut locals)?,
                Stmt::Return(return_stmt) => {
                    let ty = self.type_of_expr(&return_stmt.value, &mut locals)?;
                    if ty != return_type {
                        return Err(Diagnostic::at(
                            return_stmt.span,
                            format!(
                                "return type mismatch: expected `{}`, found `{}`",
                                return_type, ty
                            ),
                        ));
                    }
                    saw_return = true;
                }
                Stmt::Expr(expr_stmt) => {
                    self.type_of_expr(&expr_stmt.expr, &mut locals)?;
                }
            }
        }

        if return_type != Type::Unit && !saw_return {
            return Err(Diagnostic::at(
                function.span,
                format!("function `{}` is missing a return", function.name),
            ));
        }

        Ok(())
    }

    fn check_assign(
        &self,
        assign: &AssignStmt,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        let value_ty = self.type_of_expr(&assign.value, locals)?;
        let annotation_ty = assign
            .annotation
            .as_ref()
            .map(|annotation| lower_type(annotation, self.class_names))
            .transpose()?;

        if let Some(existing) = locals.get_mut(&assign.name) {
            if assign.mutable {
                return Err(Diagnostic::at(
                    assign.span,
                    format!(
                        "`{}` is already declared; `mut` cannot redeclare an existing binding",
                        assign.name
                    ),
                ));
            }

            if !existing.mutable {
                return Err(Diagnostic::at(
                    assign.span,
                    format!("cannot assign to immutable binding `{}`", assign.name),
                ));
            }

            if let Some(annotation_ty) = annotation_ty {
                if annotation_ty != existing.ty {
                    return Err(Diagnostic::at(
                        assign.span,
                        format!(
                            "reassignment annotation for `{}` has type `{}`, expected `{}`",
                            assign.name, annotation_ty, existing.ty
                        ),
                    ));
                }
            }

            if value_ty != existing.ty {
                return Err(Diagnostic::at(
                    assign.span,
                    format!(
                        "cannot assign value of type `{}` to `{}` of type `{}`",
                        value_ty, assign.name, existing.ty
                    ),
                ));
            }

            return Ok(());
        }

        let final_ty = annotation_ty.unwrap_or_else(|| value_ty.clone());
        if value_ty != final_ty {
            return Err(Diagnostic::at(
                assign.span,
                format!(
                    "binding `{}` has annotated type `{}`, but value has type `{}`",
                    assign.name, final_ty, value_ty
                ),
            ));
        }

        locals.insert(
            assign.name.clone(),
            LocalBinding {
                ty: final_ty,
                mutable: assign.mutable,
            },
        );
        Ok(())
    }

    fn type_of_expr(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Type> {
        match &expr.kind {
            ExprKind::Name(name) => locals
                .get(name)
                .map(|binding| binding.ty.clone())
                .ok_or_else(|| Diagnostic::at(expr.span, format!("unknown name `{}`", name))),
            ExprKind::Int(_) => Ok(Type::named("i32")),
            ExprKind::Float(_) => Ok(Type::named("f64")),
            ExprKind::Group(inner) => self.type_of_expr(inner, locals),
            ExprKind::Binary { op, left, right } => {
                let left_ty = self.type_of_expr(left, locals)?;
                let right_ty = self.type_of_expr(right, locals)?;
                if left_ty != right_ty {
                    return Err(Diagnostic::at(
                        expr.span,
                        format!(
                            "binary operator operands must match, found `{}` and `{}`",
                            left_ty, right_ty
                        ),
                    ));
                }
                match (op, &left_ty) {
                    (
                        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div,
                        Type::Named(name, args),
                    ) if args.is_empty() && matches!(name.as_str(), "i32" | "f64") => Ok(left_ty),
                    _ => Err(Diagnostic::at(
                        expr.span,
                        format!("unsupported operands for binary expression: `{}`", left_ty),
                    )),
                }
            }
            ExprKind::Member { object, field } => {
                let object_ty = self.type_of_expr(object, locals)?;
                self.resolve_member_type(&object_ty, field, expr.span)
            }
            ExprKind::Call { callee, args } => self.type_of_call(callee, args, expr.span, locals),
        }
    }

    fn type_of_call(
        &self,
        callee: &Expr,
        args: &[Argument],
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Type> {
        match &callee.kind {
            ExprKind::Name(name) if name == "println" => {
                if args.len() != 1 {
                    return Err(Diagnostic::at(
                        span,
                        "`println` expects exactly one argument",
                    ));
                }
                if args[0].name.is_some() {
                    return Err(Diagnostic::at(
                        span,
                        "`println` does not take keyword arguments",
                    ));
                }
                self.type_of_expr(&args[0].value, locals)?;
                Ok(Type::Unit)
            }
            ExprKind::Name(name) if self.functions.contains_key(name) => {
                let function = self.functions.get(name).unwrap();
                if args.len() != function.signature.params.len() {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "function `{}` expects {} arguments, found {}",
                            name,
                            function.signature.params.len(),
                            args.len()
                        ),
                    ));
                }

                for (argument, expected) in args.iter().zip(&function.signature.params) {
                    if argument.name.is_some() {
                        return Err(Diagnostic::at(
                            argument.span,
                            format!("function `{}` does not support keyword arguments yet", name),
                        ));
                    }
                    let actual = self.type_of_expr(&argument.value, locals)?;
                    if &actual != expected {
                        return Err(Diagnostic::at(
                            argument.span,
                            format!(
                                "argument type mismatch for `{}`: expected `{}`, found `{}`",
                                name, expected, actual
                            ),
                        ));
                    }
                }

                Ok(function.signature.return_type.clone())
            }
            ExprKind::Name(name) if self.classes.contains_key(name) => {
                let class = self.classes.get(name).unwrap();
                if args.iter().any(|argument| argument.name.is_none()) {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "class constructor `{}` currently requires keyword arguments",
                            name
                        ),
                    ));
                }

                let mut provided = HashMap::new();
                for argument in args {
                    let field_name = argument.name.as_ref().unwrap();
                    let Some(field_info) = class.fields.get(field_name) else {
                        return Err(Diagnostic::at(
                            argument.span,
                            format!("class `{}` has no field named `{}`", name, field_name),
                        ));
                    };
                    if provided.insert(field_name.clone(), ()).is_some() {
                        return Err(Diagnostic::at(
                            argument.span,
                            format!("field `{}` was provided more than once", field_name),
                        ));
                    }

                    let actual = self.type_of_expr(&argument.value, locals)?;
                    if actual != field_info.ty {
                        return Err(Diagnostic::at(
                            argument.span,
                            format!(
                                "field `{}` expects `{}`, found `{}`",
                                field_name, field_info.ty, actual
                            ),
                        ));
                    }
                }

                for field in &class.decl.fields {
                    if !provided.contains_key(&field.name) && field.default.is_none() {
                        return Err(Diagnostic::at(
                            span,
                            format!(
                                "class constructor `{}` is missing required field `{}`",
                                name, field.name
                            ),
                        ));
                    }
                }

                Ok(Type::named(name))
            }
            ExprKind::Member { object, field } => {
                let receiver_ty = self.type_of_expr(object, locals)?;
                match (&receiver_ty, field.as_str()) {
                    (Type::Named(name, type_args), "sqrt")
                        if type_args.is_empty() && name == "f64" =>
                    {
                        if args.iter().any(|argument| argument.name.is_some()) {
                            return Err(Diagnostic::at(
                                span,
                                "`sqrt` does not take keyword arguments",
                            ));
                        }
                        if !args.is_empty() {
                            return Err(Diagnostic::at(span, "`sqrt` does not take arguments"));
                        }
                        Ok(Type::named("f64"))
                    }
                    _ => Err(Diagnostic::at(
                        span,
                        format!("unsupported method call `{}` on `{}`", field, receiver_ty),
                    )),
                }
            }
            _ => Err(Diagnostic::at(span, "unsupported call target")),
        }
    }

    fn resolve_member_type(
        &self,
        object_ty: &Type,
        field: &str,
        span: crate::diag::Span,
    ) -> Result<Type> {
        let Type::Named(name, args) = object_ty else {
            return Err(Diagnostic::at(
                span,
                format!("cannot access field `{}` on `{}`", field, object_ty),
            ));
        };

        if !args.is_empty() {
            return Err(Diagnostic::at(
                span,
                format!(
                    "generic type `{}` is not implemented in this bootstrap compiler",
                    name
                ),
            ));
        }

        let Some(class_info) = self.classes.get(name) else {
            return Err(Diagnostic::at(
                span,
                format!("type `{}` has no field `{}`", name, field),
            ));
        };
        class_info
            .fields
            .get(field)
            .map(|info| info.ty.clone())
            .ok_or_else(|| {
                Diagnostic::at(span, format!("class `{}` has no field `{}`", name, field))
            })
    }
}
