use std::collections::{BTreeMap, HashMap};

use crate::ast::{Argument, Expr, ExprKind, FunctionDecl, Stmt};
use crate::diag::{Diagnostic, Result};
use crate::sema::Program;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Unit,
    Instance(InstanceValue),
}

#[derive(Clone, Debug, PartialEq)]
pub struct InstanceValue {
    pub class_name: String,
    pub fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RunOutput {
    pub value: Value,
    pub stdout: String,
}

impl Value {
    pub fn render(&self) -> String {
        match self {
            Value::Int(value) => value.to_string(),
            Value::Float(value) => value.to_string(),
            Value::Unit => String::new(),
            Value::Instance(instance) => {
                let mut rendered = format!("{}(", instance.class_name);
                for (index, (name, value)) in instance.fields.iter().enumerate() {
                    if index > 0 {
                        rendered.push_str(", ");
                    }
                    rendered.push_str(name);
                    rendered.push('=');
                    rendered.push_str(&value.render());
                }
                rendered.push(')');
                rendered
            }
        }
    }
}

pub fn run(program: &Program) -> Result<RunOutput> {
    let mut interpreter = Interpreter {
        program,
        stdout: String::new(),
    };
    let value = interpreter.run_main()?;
    Ok(RunOutput {
        value,
        stdout: interpreter.stdout,
    })
}

struct Interpreter<'a> {
    program: &'a Program,
    stdout: String,
}

enum ExecFlow {
    Continue,
    Return(Value),
}

impl<'a> Interpreter<'a> {
    fn run_main(&mut self) -> Result<Value> {
        let Some(main_fn) = self.program.functions.get("main") else {
            if self.program.top_level_stmts.is_empty() {
                return Err(Diagnostic::new(
                    "no `main` function or top-level script statements were found",
                ));
            }
            return self.run_top_level_script();
        };

        if !main_fn.signature.params.is_empty() {
            return Err(Diagnostic::at(
                main_fn.decl.span,
                "`main` must not take parameters in the bootstrap runtime",
            ));
        }

        self.call_function(&main_fn.decl, Vec::new())
    }

    fn run_top_level_script(&mut self) -> Result<Value> {
        let mut env = HashMap::new();

        for stmt in &self.program.top_level_stmts {
            match self.exec_stmt(stmt, &mut env)? {
                ExecFlow::Continue => {}
                ExecFlow::Return(_) => unreachable!("top-level return should be rejected in sema"),
            }
        }

        Ok(Value::Int(0))
    }

    fn call_function(&mut self, function: &FunctionDecl, args: Vec<Value>) -> Result<Value> {
        let mut env = HashMap::new();
        for (param, value) in function.params.iter().zip(args) {
            env.insert(param.name.clone(), value);
        }

        for stmt in &function.body {
            match self.exec_stmt(stmt, &mut env)? {
                ExecFlow::Continue => {}
                ExecFlow::Return(value) => return Ok(value),
            }
        }

        Ok(Value::Unit)
    }

    fn exec_stmt(&mut self, stmt: &Stmt, env: &mut HashMap<String, Value>) -> Result<ExecFlow> {
        match stmt {
            Stmt::Assign(assign) => {
                let value = self.eval_expr(&assign.value, env)?;
                env.insert(assign.name.clone(), value);
                Ok(ExecFlow::Continue)
            }
            Stmt::Return(return_stmt) => {
                let value = if let Some(value) = &return_stmt.value {
                    self.eval_expr(value, env)?
                } else {
                    Value::Unit
                };
                Ok(ExecFlow::Return(value))
            }
            Stmt::Expr(expr_stmt) => {
                self.eval_expr(&expr_stmt.expr, env)?;
                Ok(ExecFlow::Continue)
            }
        }
    }

    fn eval_expr(&mut self, expr: &Expr, env: &mut HashMap<String, Value>) -> Result<Value> {
        match &expr.kind {
            ExprKind::Name(name) => env
                .get(name)
                .cloned()
                .ok_or_else(|| Diagnostic::at(expr.span, format!("unknown name `{}`", name))),
            ExprKind::Int(value) => Ok(Value::Int(*value)),
            ExprKind::Float(value) => Ok(Value::Float(*value)),
            ExprKind::Group(inner) => self.eval_expr(inner, env),
            ExprKind::Binary { op, left, right } => {
                let left_value = self.eval_expr(left, env)?;
                let right_value = self.eval_expr(right, env)?;
                self.eval_binary(expr.span, *op, left_value, right_value)
            }
            ExprKind::Member { object, field } => {
                let value = self.eval_expr(object, env)?;
                match value {
                    Value::Instance(instance) => {
                        instance.fields.get(field).cloned().ok_or_else(|| {
                            Diagnostic::at(
                                expr.span,
                                format!("class `{}` has no field `{}`", instance.class_name, field),
                            )
                        })
                    }
                    _ => Err(Diagnostic::at(
                        expr.span,
                        format!("cannot access field `{}` on non-instance value", field),
                    )),
                }
            }
            ExprKind::Call { callee, args } => self.eval_call(callee, args, env),
        }
    }

    fn eval_call(
        &mut self,
        callee: &Expr,
        args: &[Argument],
        env: &mut HashMap<String, Value>,
    ) -> Result<Value> {
        match &callee.kind {
            ExprKind::Name(name) if name == "print" => {
                if args.len() != 1 {
                    return Err(Diagnostic::at(
                        callee.span,
                        "`print` expects exactly one argument",
                    ));
                }
                let value = self.eval_expr(&args[0].value, env)?;
                self.stdout.push_str(&value.render());
                self.stdout.push('\n');
                Ok(Value::Unit)
            }
            ExprKind::Name(name) if self.program.functions.contains_key(name) => {
                let function = self.program.functions.get(name).unwrap().decl.clone();
                let mut values = Vec::new();
                for argument in args {
                    values.push(self.eval_expr(&argument.value, env)?);
                }
                self.call_function(&function, values)
            }
            ExprKind::Name(name) if self.program.classes.contains_key(name) => {
                let class = self.program.classes.get(name).unwrap();
                let mut values = BTreeMap::new();
                let mut provided = HashMap::new();

                for argument in args {
                    let Some(field_name) = &argument.name else {
                        return Err(Diagnostic::at(
                            argument.span,
                            format!("constructor `{}` requires keyword arguments", name),
                        ));
                    };
                    let value = self.eval_expr(&argument.value, env)?;
                    provided.insert(field_name.clone(), ());
                    values.insert(field_name.clone(), value);
                }

                for field in &class.decl.fields {
                    if values.contains_key(&field.name) {
                        continue;
                    }
                    if let Some(default) = &field.default {
                        values.insert(field.name.clone(), self.eval_expr(default, env)?);
                    } else {
                        return Err(Diagnostic::at(
                            callee.span,
                            format!("missing required field `{}` for `{}`", field.name, name),
                        ));
                    }
                }

                Ok(Value::Instance(InstanceValue {
                    class_name: name.clone(),
                    fields: values,
                }))
            }
            ExprKind::Member { object, field } if field == "sqrt" => {
                if !args.is_empty() {
                    return Err(Diagnostic::at(
                        callee.span,
                        "`sqrt` does not take arguments",
                    ));
                }
                match self.eval_expr(object, env)? {
                    Value::Float(value) => Ok(Value::Float(value.sqrt())),
                    other => Err(Diagnostic::at(
                        callee.span,
                        format!(
                            "`sqrt` is only available on `float64`, found `{}`",
                            other.render()
                        ),
                    )),
                }
            }
            _ => Err(Diagnostic::at(callee.span, "unsupported call target")),
        }
    }

    fn eval_binary(
        &self,
        span: crate::diag::Span,
        op: crate::ast::BinaryOp,
        left: Value,
        right: Value,
    ) -> Result<Value> {
        match (left, right) {
            (Value::Int(left), Value::Int(right)) => {
                let value = match op {
                    crate::ast::BinaryOp::Add => left + right,
                    crate::ast::BinaryOp::Sub => left - right,
                    crate::ast::BinaryOp::Mul => left * right,
                    crate::ast::BinaryOp::Div => left / right,
                };
                Ok(Value::Int(value))
            }
            (Value::Float(left), Value::Float(right)) => {
                let value = match op {
                    crate::ast::BinaryOp::Add => left + right,
                    crate::ast::BinaryOp::Sub => left - right,
                    crate::ast::BinaryOp::Mul => left * right,
                    crate::ast::BinaryOp::Div => left / right,
                };
                Ok(Value::Float(value))
            }
            _ => Err(Diagnostic::at(
                span,
                "binary operands must have matching numeric types",
            )),
        }
    }
}
