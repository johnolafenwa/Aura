use super::{builtin_member_argument_type, runtime_member_result_type};
use crate::call::BuiltinMember;
use crate::sema::Type;

fn named(name: &str, args: Vec<Type>) -> Type {
    Type::Named(name.to_string(), args)
}

fn bytes() -> Type {
    named("list", vec![Type::named("uint8")])
}

#[test]
fn runtime_member_results_name_each_member_type() {
    let int64 = Type::named("int64");
    // Duration, `random.Rng`, process, `str`, and queue members are
    // resolved runtime operations; `runtime_ops_tests.rs` checks their
    // result types.
    let cases = [
        (
            "list",
            vec![int64.clone()],
            "__slice",
            Some(named("list", vec![int64.clone()])),
        ),
        ("str", vec![], "__slice", Some(Type::named("str"))),
        ("process.Pipe", vec![], "read_all", None),
        ("process.Pipe", vec![], "missing", None),
    ];
    for (name, args, field, expected) in cases {
        assert_eq!(
            runtime_member_result_type(name, &args, field),
            expected,
            "{name}.{field}"
        );
    }
}

#[test]
fn builtin_member_arguments_name_open_collection_types() {
    let int64 = Type::named("int64");
    let text = Type::named("str");
    let headers = named("dict", vec![text.clone(), text.clone()]);
    let cases = [
        (
            BuiltinMember::StringJoin,
            0,
            vec![],
            Some(named("list", vec![text.clone()])),
        ),
        (
            BuiltinMember::VecExtend,
            0,
            vec![int64.clone()],
            Some(named("list", vec![int64.clone()])),
        ),
        (BuiltinMember::VecExtend, 0, vec![], None),
        (
            BuiltinMember::MapExtend,
            0,
            vec![text.clone(), int64.clone()],
            Some(named("dict", vec![text.clone(), int64.clone()])),
        ),
        (BuiltinMember::FileWriteBytes, 0, vec![], Some(bytes())),
        (BuiltinMember::UdpSocketSendBytes, 1, vec![], Some(bytes())),
        (
            BuiltinMember::HttpExchangeRespondText,
            2,
            vec![],
            Some(headers.clone()),
        ),
        (
            BuiltinMember::ProcessSupervisorStart,
            3,
            vec![],
            Some(headers),
        ),
        (
            BuiltinMember::ProcessSupervisorStart,
            1,
            vec![],
            Some(named("list", vec![text])),
        ),
        (BuiltinMember::StringJoin, 1, vec![], None),
    ];
    for (member, index, class_args, expected) in cases {
        assert_eq!(
            builtin_member_argument_type(member, index, &class_args),
            expected,
            "{member:?} argument {index}"
        );
    }
}

const COMPLETION_SOURCE: &str = r#"
enum Shape:
    Circle(float64)
    Square(int64)

class Point:
    x: int64
    y: int64

class Box[T]:
    item: T

    def echo(self, value: own T) -> T:
        return value

trait Area:
    def area(self) -> float64

impl Area for Point:
    def area(self) -> float64:
        return 0.0

def main():
    pass
"#;

/// Completes the type of each rvalue a temporary can hold, as lowering
/// does for a temporary it created without a type. Each local the cases
/// read has the type `locals` gives it.
#[test]
fn completed_rvalue_types_follow_each_rvalue_kind() {
    use super::{CallTarget, Lowerer, Operand, Rvalue};
    use crate::ast::{BinaryOp, UnaryOp};
    use crate::diag::Span;
    use crate::mir::MirFormatPart;
    use crate::runtime_ops::{RuntimeOp, ScalarOp, ValueOp};
    use crate::sema::{optional_type, CallableType, ClosureCallKind};
    use std::collections::{BTreeMap, HashMap};

    let program = crate::check_source(COMPLETION_SOURCE).expect("the source should check");
    let mut lowerer = Lowerer::new(
        &program,
        "main",
        &program.module_name,
        Type::Unit,
        BTreeMap::new(),
    );
    let int32 = Type::named("int32");
    let int64 = Type::named("int64");
    let float64 = Type::named("float64");
    let text = Type::named("str");
    let maybe_int = optional_type(int64.clone());
    let function = Type::Function {
        params: Vec::new(),
        return_type: Box::new(int64.clone()),
    };
    for (name, ty) in [
        ("count", int32.clone()),
        ("shape", Type::named("Shape")),
        ("pair", Type::Tuple(vec![int64.clone(), text.clone()])),
        ("point", Type::named("Point")),
        ("boxed", named("Box", vec![text.clone()])),
        ("maybe", maybe_int.clone()),
        (
            "outcome",
            named("Result", vec![int64.clone(), text.clone()]),
        ),
        ("function", function.clone()),
        ("array", named("Array", vec![int64.clone()])),
        (
            "closure",
            Type::Closure {
                params: Box::default(),
                return_type: Box::new(text.clone()),
                captures: Box::default(),
                call_kind: ClosureCallKind::Repeatable,
            },
        ),
        (
            "callable",
            Type::Callable(Box::new(CallableType {
                task: false,
                call_kind: ClosureCallKind::Repeatable,
                params: Vec::new(),
                return_type: float64.clone(),
            })),
        ),
    ] {
        lowerer.local_types.insert(name.to_string(), ty);
    }
    let params = HashMap::new();
    let place = |name: &str| Operand::Place(name.to_string());
    let span = Span::new(1, 1);
    let call = |callee: CallTarget| Rvalue::Call {
        callee,
        args: Vec::new(),
    };
    let member = |object: &str, field: &str| {
        call(CallTarget::Member {
            object: place(object),
            field: field.to_string(),
            receiver_place: None,
        })
    };
    let cases = [
        (
            Rvalue::Binary {
                op: BinaryOp::Less,
                left: place("count"),
                right: Operand::Int(1),
                span,
            },
            Some(Type::named("bool")),
        ),
        (
            Rvalue::Binary {
                op: BinaryOp::Mul,
                left: Operand::Int(2),
                right: Operand::Duration(5),
                span,
            },
            Some(Type::named("Duration")),
        ),
        (
            Rvalue::VariantPayload {
                scrutinee: place("shape"),
                variant_name: "Circle".to_string(),
                index: 0,
            },
            Some(float64.clone()),
        ),
        (
            Rvalue::VariantPayload {
                scrutinee: place("pair"),
                variant_name: "Circle".to_string(),
                index: 0,
            },
            None,
        ),
        (
            Rvalue::Try {
                value: place("outcome"),
            },
            Some(int64.clone()),
        ),
        (
            Rvalue::Try {
                value: place("count"),
            },
            None,
        ),
        (
            Rvalue::Unary {
                op: UnaryOp::Not,
                value: Operand::Bool(true),
                span,
            },
            Some(Type::named("bool")),
        ),
        (
            Rvalue::Unary {
                op: UnaryOp::Neg,
                value: place("count"),
                span,
            },
            Some(int32.clone()),
        ),
        (
            Rvalue::Cast {
                value: place("count"),
                ty: float64.clone(),
                span,
            },
            Some(float64.clone()),
        ),
        (
            Rvalue::NoneTest {
                value: place("maybe"),
            },
            Some(Type::named("bool")),
        ),
        (
            Rvalue::UnionTakePayload {
                place: "maybe".to_string(),
                union_type: maybe_int.clone(),
                member_type: int64.clone(),
                member_index: 0,
            },
            Some(int64.clone()),
        ),
        (
            Rvalue::UnionInject {
                value: Operand::Int(1),
                union_type: maybe_int.clone(),
                member_type: int64.clone(),
                member_index: 0,
            },
            Some(maybe_int.clone()),
        ),
        (
            Rvalue::Closure {
                function: "main".to_string(),
                signature: function.clone(),
                captures: Vec::new(),
                consuming: false,
                mutable: false,
            },
            Some(function.clone()),
        ),
        (
            Rvalue::FormatString {
                parts: vec![MirFormatPart::Literal("x".to_string())],
            },
            Some(text.clone()),
        ),
        (
            Rvalue::Construct {
                class_name: "Point".to_string(),
                fields: Vec::new(),
            },
            Some(Type::named("Point")),
        ),
        (
            Rvalue::TupleElement {
                tuple: place("pair"),
                index: 1,
                element_type: text.clone(),
            },
            Some(text.clone()),
        ),
        (
            Rvalue::TupleTakeElement {
                place: "pair".to_string(),
                index: 0,
                element_type: int64.clone(),
            },
            Some(int64.clone()),
        ),
        (
            Rvalue::ModuleConstant {
                key: "limit".to_string(),
                initializer: "limit".to_string(),
            },
            None,
        ),
        // Operands and fields.
        (Rvalue::Use(Operand::Bool(false)), Some(Type::named("bool"))),
        (Rvalue::Use(place("pair.0")), Some(int64.clone())),
        (Rvalue::Use(place("point.y")), Some(int64.clone())),
        (Rvalue::Use(place("boxed.item")), Some(text.clone())),
        (Rvalue::Use(place("count.value")), None),
        (Rvalue::Use(place("function.value")), None),
        // Resolved runtime operations.
        (
            call(CallTarget::Runtime {
                op: RuntimeOp::Scalar(ScalarOp::WrappingAdd),
                receiver: Some(place("count")),
                receiver_place: None,
            }),
            Some(int32.clone()),
        ),
        (
            call(CallTarget::Runtime {
                op: RuntimeOp::Value(ValueOp::ToString),
                receiver: None,
                receiver_place: None,
            }),
            Some(text.clone()),
        ),
        // Calls through callable values.
        (
            call(CallTarget::Value(place("function"))),
            Some(int64.clone()),
        ),
        (
            call(CallTarget::Value(place("closure"))),
            Some(text.clone()),
        ),
        (
            call(CallTarget::Value(place("callable"))),
            Some(float64.clone()),
        ),
        (call(CallTarget::Value(place("count"))), None),
        // Member calls.
        (member("maybe", "clone"), Some(maybe_int.clone())),
        (member("maybe", "to_float"), Some(float64.clone())),
        (member("maybe", "missing"), None),
        (member("boxed", "echo"), Some(text.clone())),
        (member("count", "to_float"), Some(float64.clone())),
        (member("point", "area"), Some(float64.clone())),
        (
            member("array", "shape"),
            Some(named("list", vec![int64.clone()])),
        ),
    ];
    for (value, expected) in cases {
        assert_eq!(
            lowerer.completed_rvalue_type(&value, &params),
            expected,
            "{value:?}"
        );
    }
}
