use std::collections::BTreeSet;
use std::collections::HashMap;
use std::path::PathBuf;

use aura_core::layout::LayoutKind;

use object::{Object, ObjectSection, ObjectSymbol, RelocationTarget};

use super::{
    box_thunk_value, cleanup_place_type, collect_type_params_from_type, direct_ffi_type_for_source,
    direct_field_type, direct_type, direct_type_to_type, emit_host_object,
    emit_host_object_with_metadata, ensure_direct_type, enum_variant_payload_types_for_target,
    infer_operand_type, is_numeric_type_name, main_signature, mangle_default_binder_symbol,
    mangle_symbol, mangle_thunk_symbol, release_direct_call_results, release_direct_values,
    render_direct_type, runtime_type_is_wildcard, signature_for, thunk_signature,
    thunk_string_constant, unbox_thunk_value, validate_function, validate_operand, validate_rvalue,
    validate_tuple_projection_operand, validate_tuple_take_place, DirectType, DirectViewPlace,
    NativeCodegen, PlainClassField, PlainClassType, ScalarKind,
};
use crate::ast::{BinaryOp, UnaryOp};
use crate::diag::Span;
use crate::mir::MirReceiverKind;
use crate::mir::{
    AssertionCapture, BasicBlock, CallTarget, Instruction, MirArg, MirExternCall, MirExternParam,
    MirFunction, MirLocalType, MirMapEntry, MirMatchArm, MirParam, Operand, Rvalue, Terminator,
};
use crate::native_backend::{types, CallConv, FunctionBackend, Signature, Var};
use crate::sema::Type;
use crate::{lower_path_to_mir, lower_source_to_mir};

#[test]
fn adr0038_direct_view_fixed_point_comparison_ignores_order_not_authority() {
    use super::{direct_view_maps_equivalent, DirectViewAlternative};
    let a = DirectViewAlternative {
        union_payloads: Vec::new(),
        place: "owner.left".into(),
        conditions: vec![(Var(1), 0), (Var(2), 1)],
        elements: Vec::new(),
    };
    let b = DirectViewAlternative {
        union_payloads: Vec::new(),
        place: "owner.right".into(),
        conditions: vec![(Var(1), 1)],
        elements: Vec::new(),
    };
    let left = HashMap::from([(
        "view".into(),
        DirectViewPlace {
            alternatives: vec![a.clone(), b.clone()],
        },
    )]);
    let mut reordered = a.clone();
    reordered.conditions.reverse();
    let right = HashMap::from([(
        "view".into(),
        DirectViewPlace {
            alternatives: vec![b.clone(), reordered],
        },
    )]);
    assert!(direct_view_maps_equivalent(&left, &right));
    assert!(!direct_view_maps_equivalent(&left, &HashMap::new()));
    assert!(!direct_view_maps_equivalent(
        &left,
        &HashMap::from([("different".into(), left["view"].clone())])
    ));
    for alternatives in [
        vec![a.clone()],
        vec![a.clone(), a.clone()],
        vec![
            a.clone(),
            DirectViewAlternative {
                union_payloads: Vec::new(),
                place: b.place.clone(),
                conditions: vec![],
                elements: Vec::new(),
            },
        ],
        vec![
            a,
            DirectViewAlternative {
                union_payloads: Vec::new(),
                place: b.place,
                conditions: vec![(Var(1), 0)],
                elements: Vec::new(),
            },
        ],
    ] {
        assert!(!direct_view_maps_equivalent(
            &left,
            &HashMap::from([("view".into(), DirectViewPlace { alternatives })])
        ));
    }
}

#[test]
fn adr0038_direct_cfg_helpers_validate_reachability_order_and_selector_tags() {
    use std::collections::HashSet;

    use super::{
        direct_reverse_postorder, direct_view_selector_tags, reachable_direct_block_labels,
    };

    let block = |label: &str, instructions: Vec<Instruction>, terminator: Terminator| BasicBlock {
        label: label.to_string(),
        instructions,
        terminator,
    };
    let function = MirFunction {
        name: "cfg_helpers".to_string(),
        module_name: "<test>".to_string(),
        source_path: None,
        span: Span::new(1, 1),
        receiver: None,
        params: Vec::new(),
        local_types: Vec::new(),
        return_type: Type::Unit,
        entry: "entry".to_string(),
        blocks: vec![
            block(
                "entry",
                vec![Instruction::BeginReturnedLoan {
                    loan: "chosen".to_string(),
                    origin: "owner".to_string(),
                    projections: vec!["right".to_string(), "left".to_string()],
                    mutable: false,
                }],
                Terminator::Branch {
                    condition: Operand::Bool(true),
                    then_label: "left".to_string(),
                    else_label: "right".to_string(),
                },
            ),
            block("left", Vec::new(), Terminator::Goto("join".to_string())),
            block("right", Vec::new(), Terminator::Goto("join".to_string())),
            block("join", Vec::new(), Terminator::Return(Operand::Unit)),
            block(
                "dead",
                vec![Instruction::BeginReturnedLoan {
                    loan: "chosen".to_string(),
                    origin: "owner".to_string(),
                    projections: vec!["ignored".to_string()],
                    mutable: false,
                }],
                Terminator::Return(Operand::Unit),
            ),
        ],
    };

    let reachable = reachable_direct_block_labels(&function).expect("valid CFG should resolve");
    assert_eq!(
        reachable,
        HashSet::from_iter(["entry", "left", "right", "join"].map(str::to_string))
    );
    let order = direct_reverse_postorder(&function, &reachable).expect("valid CFG should order");
    assert_eq!(order.first().map(String::as_str), Some("entry"));
    assert_eq!(order.last().map(String::as_str), Some("join"));
    assert_eq!(order.iter().collect::<HashSet<_>>().len(), reachable.len());

    let tags = direct_view_selector_tags(&function, &reachable);
    assert_eq!(tags["chosen"].get("left"), Some(&0));
    assert_eq!(tags["chosen"].get("right"), Some(&1));
    assert!(!tags["chosen"].contains_key("ignored"));

    let mut malformed = function.clone();
    malformed.blocks.push(malformed.blocks[0].clone());
    assert!(reachable_direct_block_labels(&malformed)
        .expect_err("duplicate labels must fail")
        .contains("duplicate MIR block `entry`"));

    malformed = function.clone();
    malformed.entry = "missing".to_string();
    assert!(reachable_direct_block_labels(&malformed)
        .expect_err("missing entry must fail")
        .contains("could not find entry block `missing`"));
    assert!(
        direct_reverse_postorder(&malformed, &HashSet::from(["missing".to_string()]))
            .expect_err("an unresolved reachable entry must fail")
            .contains("could not resolve reachable block `missing`")
    );

    malformed = function;
    malformed.blocks[1].terminator = Terminator::Goto("missing".to_string());
    assert!(reachable_direct_block_labels(&malformed)
        .expect_err("unknown successors must fail")
        .contains("targets unknown block `missing`"));
}

#[test]
fn adr0038_direct_view_and_closure_metadata_helpers_preserve_dataflow_invariants() {
    use std::collections::HashSet;

    use super::{
        direct_closure_metadata_uses, direct_closure_writeback_live_ins,
        merge_direct_closure_writebacks, DirectClosureCaptureWriteback, DirectViewAlternative,
    };
    use crate::sema::ClosureCallKind;

    let selector = Var(7);
    let projected = DirectViewPlace::static_place("owner".to_string())
        .project("")
        .project("field")
        .conditioned(selector, 1)
        .conditioned(selector, 1);
    assert_eq!(
        projected.alternatives,
        vec![DirectViewAlternative {
            union_payloads: Vec::new(),
            place: "owner.field".to_string(),
            conditions: vec![(selector, 1)],
            elements: Vec::new(),
        }]
    );

    let writeback = |index, place: &str, ty| DirectClosureCaptureWriteback {
        index,
        place: DirectViewPlace::static_place(place.to_string()),
        ty,
        env_offset: 0,
        inline: true,
    };
    let left = HashMap::from([(
        "worker".to_string(),
        vec![writeback(0, "left", DirectType::Scalar(ScalarKind::Int64))],
    )]);
    let right = HashMap::from([
        (
            "worker".to_string(),
            vec![
                writeback(0, "right", DirectType::Scalar(ScalarKind::Int64)),
                writeback(1, "other", DirectType::Scalar(ScalarKind::Bool)),
                writeback(0, "typed", DirectType::Scalar(ScalarKind::Bool)),
            ],
        ),
        (
            "second".to_string(),
            vec![writeback(
                0,
                "second",
                DirectType::Scalar(ScalarKind::Int64),
            )],
        ),
    ]);
    let merged = merge_direct_closure_writebacks(&left, &right);
    assert_eq!(merged["worker"].len(), 3);
    assert_eq!(merged["worker"][0].place.alternatives.len(), 2);
    assert_eq!(merged["second"].len(), 1);

    let transferred = Rvalue::Use(Operand::MovePlace("worker.capture".to_string()));
    assert_eq!(
        direct_closure_metadata_uses(&transferred).collect::<Vec<_>>(),
        ["worker"]
    );
    let called = Rvalue::Call {
        callee: CallTarget::Value(Operand::Place("worker.callable".to_string())),
        args: Vec::new(),
        type_args: Default::default(),
    };
    assert_eq!(
        direct_closure_metadata_uses(&called).collect::<Vec<_>>(),
        ["worker"]
    );
    assert!(direct_closure_metadata_uses(&Rvalue::Use(Operand::Int(0)))
        .next()
        .is_none());

    let closure_ty = Type::Closure {
        params: Box::new(Vec::new()),
        return_type: Box::new(Type::Unit),
        captures: Box::new(Vec::new()),
        call_kind: ClosureCallKind::Repeatable,
    };
    let function = MirFunction {
        name: "closure_liveness".to_string(),
        module_name: "<test>".to_string(),
        source_path: None,
        span: Span::new(1, 1),
        receiver: None,
        params: Vec::new(),
        local_types: vec![
            MirLocalType {
                name: "worker".to_string(),
                ty: closure_ty.clone(),
            },
            MirLocalType {
                name: "replacement".to_string(),
                ty: closure_ty,
            },
        ],
        return_type: Type::Unit,
        entry: "entry".to_string(),
        blocks: vec![
            BasicBlock {
                label: "entry".to_string(),
                instructions: Vec::new(),
                terminator: Terminator::Goto("use".to_string()),
            },
            BasicBlock {
                label: "use".to_string(),
                instructions: vec![
                    Instruction::Assign {
                        target: "replacement".to_string(),
                        value: Rvalue::Use(Operand::Place("worker.capture".to_string())),
                    },
                    Instruction::Assign {
                        target: "worker".to_string(),
                        value: Rvalue::Use(Operand::Place("replacement".to_string())),
                    },
                ],
                terminator: Terminator::Return(Operand::Unit),
            },
            BasicBlock {
                label: "dead".to_string(),
                instructions: vec![Instruction::Assign {
                    target: "value".to_string(),
                    value: Rvalue::Use(Operand::Place("worker".to_string())),
                }],
                terminator: Terminator::Return(Operand::Unit),
            },
        ],
    };
    let live = direct_closure_writeback_live_ins(
        &function,
        &HashSet::from(["entry".to_string(), "use".to_string()]),
    );
    assert_eq!(live["use"], HashSet::from(["worker".to_string()]));
    assert_eq!(live["entry"], HashSet::from(["worker".to_string()]));
    assert!(!live.contains_key("dead"));
}

#[test]
fn adr0038_direct_cfg_join_rejects_branch_dependent_view_identity() {
    let function = MirFunction {
        name: "view_join".to_string(),
        module_name: "<test>".to_string(),
        source_path: None,
        span: Span::new(1, 1),
        receiver: None,
        params: vec![
            MirParam {
                name: "left".to_string(),
                passing: MirReceiverKind::Borrow,
                ty: Type::named("int64"),
                default_function: None,
                keyword_only: false,
            },
            MirParam {
                name: "right".to_string(),
                passing: MirReceiverKind::Borrow,
                ty: Type::named("int64"),
                default_function: None,
                keyword_only: false,
            },
        ],
        local_types: Vec::new(),
        return_type: Type::Unit,
        entry: "entry".to_string(),
        blocks: vec![
            BasicBlock {
                label: "entry".to_string(),
                instructions: Vec::new(),
                terminator: Terminator::Branch {
                    condition: Operand::Bool(true),
                    then_label: "take_left".to_string(),
                    else_label: "take_right".to_string(),
                },
            },
            BasicBlock {
                label: "take_left".to_string(),
                instructions: vec![Instruction::BeginLoan {
                    loan: "selected".to_string(),
                    source: "left".to_string(),
                    mutable: false,
                }],
                terminator: Terminator::Goto("join".to_string()),
            },
            BasicBlock {
                label: "take_right".to_string(),
                instructions: vec![Instruction::BeginLoan {
                    loan: "selected".to_string(),
                    source: "right".to_string(),
                    mutable: false,
                }],
                terminator: Terminator::Goto("join".to_string()),
            },
            BasicBlock {
                label: "join".to_string(),
                instructions: Vec::new(),
                terminator: Terminator::Return(Operand::Unit),
            },
        ],
    };
    let mut setup_function = function.clone();
    for block in &mut setup_function.blocks {
        block.instructions.clear();
    }
    let module = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![setup_function],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };
    let mut codegen = NativeCodegen::new(&module, "/tmp/direct_view_join.au", "")
        .expect("direct codegen should initialize for structurally complete MIR");
    let error = codegen
        .define_function(&function)
        .expect_err("branch-dependent view identities must not cross a direct CFG join");
    assert!(
        error.contains("reaches MIR block `join`")
            && error.contains("inconsistent view identities"),
        "unexpected direct join diagnostic: {error}"
    );
}

#[test]
fn adr0038_direct_tuple_place_errors_reach_codegen_after_constructor_setup() {
    let base = MirFunction {
        name: "tuple_place".to_string(),
        module_name: "<test>".to_string(),
        source_path: None,
        span: Span::new(1, 1),
        receiver: None,
        params: vec![MirParam {
            name: "pair".to_string(),
            passing: MirReceiverKind::Value,
            ty: Type::Tuple(vec![Type::named("int64"), Type::named("bool")]),
            default_function: None,
            keyword_only: false,
        }],
        local_types: vec![MirLocalType {
            name: "result".to_string(),
            ty: Type::named("int64"),
        }],
        return_type: Type::Unit,
        entry: "entry".to_string(),
        blocks: vec![BasicBlock {
            label: "entry".to_string(),
            instructions: Vec::new(),
            terminator: Terminator::Return(Operand::Unit),
        }],
    };

    for (projection, expected) in [
        ("not_an_index", "is not a fixed position"),
        ("9", "has no element at index 9"),
    ] {
        let mut function = base.clone();
        function.blocks[0].instructions = vec![Instruction::Assign {
            target: "result".to_string(),
            value: Rvalue::Use(Operand::Place(format!("pair.{projection}"))),
        }];
        let module = crate::mir::MirModule {
            unions: Vec::new(),
            enums: Vec::new(),
            constants: Vec::new(),
            functions: vec![function.clone()],
            classes: Vec::new(),
            trait_impls: Vec::new(),
            top_level: None,
            ownership: Default::default(),
            borrowing: Default::default(),
        };
        let mut codegen = NativeCodegen::new(&module, "/tmp/direct_tuple_place.au", "")
            .expect("constructor should accept the function before place compilation");
        let error = codegen
            .define_function(&function)
            .expect_err("invalid tuple projections must fail in direct place loading");
        assert!(
            error.contains(expected),
            "unexpected projection error: {error}"
        );
    }
}

#[test]
fn adr0038_direct_method_resolution_covers_inherent_and_named_trait_fallbacks() {
    let module = lower_source_to_mir(
        r#"
trait Readable:
    def read(self) -> int64

class Counter:
    value: int64

    def read(self) -> int64:
        return self.value

class Wrapped:
    value: int64

impl Readable for Wrapped:
    def read(self) -> int64:
        return self.value

def through_bound[T: Readable](value: T) -> int64:
    return value.read()

def main() -> int32:
    direct = Counter(value=3).read()
    via_trait = through_bound(Wrapped(value=4))
    print(direct + via_trait)
    return 0
"#,
    )
    .expect("inherent and trait-dispatched methods should lower");
    emit_host_object(&module)
        .expect("direct backend should resolve inherent and named trait method fallbacks");
}

#[test]
fn adr0038_direct_metadata_diagnostics_cover_reachable_constructor_boundaries() {
    let simple = lower_source_to_mir(
        r#"
def main() -> int32:
    return 0
"#,
    )
    .expect("simple direct function should lower");
    let main = simple
        .functions
        .iter()
        .find(|function| function.name == "main")
        .cloned()
        .expect("main should lower");
    let mut missing_function_reachability =
        NativeCodegen::new(&simple, "/tmp/direct_missing_reachability.au", "")
            .expect("simple direct codegen should initialize");
    missing_function_reachability
        .reachable_blocks
        .remove("main");
    let error = missing_function_reachability
        .define_function(&main)
        .expect_err("function compilation requires reachable-block metadata");
    assert!(
        error.contains("missing reachable-block metadata for `main`"),
        "unexpected function metadata diagnostic: {error}"
    );

    let cleanup_function = cleanup_test_function("value", Type::named("int32"), "value");
    let cleanup_module = cleanup_test_module(vec![cleanup_function.clone()]);
    let mut missing_cleanup_reachability =
        NativeCodegen::new(&cleanup_module, "/tmp/direct_cleanup_reachability.au", "")
            .expect("cleanup codegen should initialize");
    missing_cleanup_reachability
        .reachable_blocks
        .remove(&cleanup_function.name);
    let error = missing_cleanup_reachability
        .define_cleanup_thunks(&cleanup_function)
        .expect_err("cleanup-thunk enumeration requires reachable-block metadata");
    assert!(
        error.contains("missing reachable-block metadata"),
        "{error}"
    );

    let initializer = MirFunction {
        name: "constant_init".to_string(),
        module_name: "<test>".to_string(),
        source_path: None,
        span: Span::new(1, 1),
        receiver: None,
        params: Vec::new(),
        local_types: Vec::new(),
        return_type: Type::named("int64"),
        entry: "entry".to_string(),
        blocks: vec![BasicBlock {
            label: "entry".to_string(),
            instructions: Vec::new(),
            terminator: Terminator::Return(Operand::Int(1)),
        }],
    };
    let constant_module = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: vec![crate::mir::MirConstant {
            key: "<test>::answer".to_string(),
            initializer: initializer.name.clone(),
            ty: Type::named("int64"),
        }],
        functions: vec![main.clone(), initializer.clone()],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };
    let mut missing_main_initializer =
        NativeCodegen::new(&constant_module, "/tmp/direct_constant_thunk.au", "")
            .expect("constant codegen should initialize");
    missing_main_initializer
        .function_thunks
        .remove(&initializer.name);
    let error = missing_main_initializer
        .define_function_thunk(&main)
        .expect_err("main thunk requires every module-constant initializer");
    assert!(
        error.contains("cannot find module constant initializer `constant_init`"),
        "unexpected main-thunk diagnostic: {error}"
    );

    let mut constant_reader = main.clone();
    constant_reader.local_types.push(MirLocalType {
        name: "answer".to_string(),
        ty: Type::named("int64"),
    });
    constant_reader.blocks[0]
        .instructions
        .push(Instruction::Assign {
            target: "answer".to_string(),
            value: Rvalue::ModuleConstant {
                key: "<test>::answer".to_string(),
                initializer: initializer.name.clone(),
            },
        });
    let reader_module = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![constant_reader.clone(), initializer.clone()],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };
    let mut missing_rvalue_initializer =
        NativeCodegen::new(&reader_module, "/tmp/direct_constant_rvalue.au", "")
            .expect("constant reader codegen should initialize");
    missing_rvalue_initializer
        .function_thunks
        .remove(&initializer.name);
    let error = missing_rvalue_initializer
        .define_function(&constant_reader)
        .expect_err("module-constant rvalues require their initializer thunk");
    assert!(
        error.contains("cannot find module constant initializer `constant_init`"),
        "unexpected constant-rvalue diagnostic: {error}"
    );
}

#[test]
fn adr0038_direct_mutable_closure_requires_source_place_metadata() {
    let source = r#"
def main():
    mut values = [1]
    mut update: def(int64) -> None = lambda [mut values] item: values.append(item)
    update(2)
"#;
    let module = lower_source_to_mir(source).expect("mutable closure should lower");
    let mut main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .cloned()
        .expect("main should lower");
    let capture = main
        .blocks
        .iter_mut()
        .flat_map(|block| block.instructions.iter_mut())
        .find_map(|instruction| match instruction {
            Instruction::Assign {
                value: Rvalue::Closure { captures, .. },
                ..
            } => captures
                .iter_mut()
                .find(|capture| capture.passing == MirReceiverKind::BorrowMut),
            _ => None,
        })
        .expect("lowered closure should have a mutable capture");
    capture.source_place = None;

    let mut codegen = NativeCodegen::new(&module, "/tmp/direct_mutable_capture.au", source)
        .expect("valid mutable closure metadata should initialize codegen");
    let error = codegen
        .define_function(&main)
        .expect_err("direct closure writeback needs an explicit capture source place");
    assert!(
        error.contains("mutable closure capture `values` has no source place"),
        "unexpected mutable-capture diagnostic: {error}"
    );
}

#[test]
fn adr0038_local_and_returned_views_compile_through_direct_backend() {
    let module = lower_source_to_mir(
        r#"
class Counter:
    value: int64

def value_mut(counter: mut Counter) -> view mut int64 from counter:
    return view mut counter.value

class Pair:
    left: int64
    right: int64

class Matrix:
    left: Pair
    right: Pair

class Labels:
    left: str
    right: str

def choose(pair: mut Pair, left: bool) -> view mut int64 from pair:
    if left:
        return view mut pair.left
    return view mut pair.right

def identity(pair: mut Pair) -> view mut Pair from pair:
    return view mut pair

def choose_pair(matrix: mut Matrix, left: bool) -> view mut Pair from matrix:
    if left:
        return view mut matrix.left
    return view mut matrix.right

def choose_label(labels: mut Labels, left: bool) -> view mut str from labels:
    if left:
        return view mut labels.left
    return view mut labels.right

def main():
    mut counter = Counter(value=1)
    view mut local = counter.value
    local = 2
    view mut returned = value_mut(counter)
    returned = 3
    print(counter.value)
    mut pair = (4, 5)
    view mut tuple_item = pair[1]
    tuple_item = 6
    print(pair)
    mut selected = Pair(left=7, right=8)
    view mut selected_right = choose(selected, false)
    selected_right = 9
    print(selected.right)
    view mut whole = identity(selected)
    whole.left = 10
    mut matrix = Matrix(left=Pair(left=11, right=12), right=Pair(left=13, right=14))
    view mut selected_pair = choose_pair(matrix, false)
    view mut nested_selection = choose(selected_pair, true)
    nested_selection = 15
    print(matrix.right.left)
    mut labels = Labels(left="Ada", right="Grace")
    view mut selected_label = choose_label(labels, false)
    print(selected_label)
    selected_label = "Lin"
    print(labels.right)
"#,
    )
    .expect("view source should lower for direct codegen");
    emit_host_object(&module).expect("direct codegen should preserve view place identity");

    let generic_trait = lower_source_to_mir(
        r#"
trait Project:
    def get(self) -> view int64 from self

class Box:
    value: int64

impl Project for Box:
    def get(self) -> view int64 from self:
        return view self.value

def read[T: Project](item: T) -> int64:
    view alias = item.get()
    return alias + 0

def main():
    print(read(Box(value=7)))
"#,
    )
    .expect("generic returned-view trait call should lower");
    emit_host_object(&generic_trait)
        .expect("generic returned-view trait calls should compile through the direct backend");

    let reviewed_edges = lower_source_to_mir(
        r#"
class Pair:
    left: int64

def inner(value: int64) -> view int64 from value:
    return view value

def outer(value: int64) -> view int64 from value:
    view alias = inner(value)
    return view alias

def field(pair: Pair) -> view int64 from pair:
    view parent = pair
    view child = parent.left
    return view child

def cleanup(value: int64) -> view int64 from value:
    with TaskGroup() as group:
        return view value

def main():
    mut value = 1
    view alias = value
    if true:
        print(alias)
        value = 2
    view current = outer(value)
    get: def() -> int64 = lambda [current]: current
    print(get())
    pair = ("Ada", "Grace")
    view first = pair[0]
    print(first)
    item = Pair(left=7)
    view selected = field(item)
    print(selected)
    view cleaned = cleanup(value)
    print(cleaned)
"#,
    )
    .expect("reviewed nested lifetime and returned-view edges should lower");
    emit_host_object(&reviewed_edges)
        .expect("reviewed nested lifetime and returned-view edges should compile directly");
}

#[test]
fn adr0038_loop_local_mutable_closure_writeback_compiles_directly() {
    let module = lower_source_to_mir(
        r#"
def add(total: mut int64, item: int64):
    total = total + item

def main():
    mut total = 0
    for item in [1, 2]:
        mut bump = lambda [mut total, item]: add(total, item)
        bump()
    print(total)
"#,
    )
    .expect("loop-local mutable closure source should lower");

    emit_host_object(&module)
        .expect("ended loop-local closure metadata must not poison the loop backedge");

    let mut reversed = module;
    reversed
        .functions
        .iter_mut()
        .find(|function| function.name == "main")
        .expect("loop source should contain main")
        .blocks
        .reverse();
    emit_host_object(&reversed)
        .expect("loop closure liveness must not depend on MIR block storage order");
}

#[test]
fn adr0038_direct_codegen_rejects_malformed_loan_and_capture_metadata() {
    let closure_source = r#"
def main():
    mut values = [1]
    mut update: def(int64) -> None = lambda [mut values] item: values.append(item)
    update(2)
"#;
    let closure_module = lower_source_to_mir(closure_source)
        .expect("mutable closure capture source should lower for direct validation");
    emit_host_object(&closure_module)
        .expect("valid mutable capture metadata should compile through direct codegen");

    let mutate_capture =
        |module: &mut crate::mir::MirModule,
         update: &mut dyn FnMut(&mut crate::mir::MirClosureCapture)| {
            let capture = module
                .functions
                .iter_mut()
                .flat_map(|function| &mut function.blocks)
                .flat_map(|block| &mut block.instructions)
                .find_map(|instruction| match instruction {
                    Instruction::Assign {
                        value: Rvalue::Closure { captures, .. },
                        ..
                    } => captures
                        .iter_mut()
                        .find(|capture| capture.passing == MirReceiverKind::BorrowMut),
                    _ => None,
                })
                .expect("lowered mutable closure should retain its capture metadata");
            update(capture);
        };

    let mut missing_source = closure_module.clone();
    mutate_capture(&mut missing_source, &mut |capture| {
        capture.source_place = None
    });
    let error = emit_host_object(&missing_source)
        .expect_err("mutable direct captures require an exact source place");
    assert!(
        error.contains("has no source place"),
        "unexpected error: {error}"
    );

    let mut generic_type = closure_module;
    mutate_capture(&mut generic_type, &mut |capture| {
        capture.ty = Type::TypeParam("Unresolved".to_string())
    });
    emit_host_object(&generic_type)
        .expect("direct closure metadata retains unresolved generic captures as opaque values");

    let mut escaped = lower_source_to_mir(
        r#"
class Pair:
    left: int64
    right: int64

def invalid(pair: mut Pair, other: mut Pair) -> view mut int64 from pair:
    return view mut pair.left
"#,
    )
    .expect("valid returned-view source should lower before metadata corruption");
    let return_loan = escaped
        .functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .flat_map(|block| &mut block.instructions)
        .find_map(|instruction| match instruction {
            Instruction::ReturnLoan { origin, .. } => Some(origin),
            _ => None,
        })
        .expect("returned view should emit ReturnLoan metadata");
    *return_loan = "other".to_string();
    let error = emit_host_object(&escaped)
        .expect_err("direct returned loans must remain within their declared origin");
    assert!(
        error.contains("resolves outside origin `other`"),
        "unexpected error: {error}"
    );
}

#[test]
fn adr0038_direct_view_place_projection_handles_root_and_nested_paths() {
    let root = DirectViewPlace::static_place("origin".to_string());
    assert_eq!(root.clone().project("").alternatives[0].place, "origin");
    assert_eq!(
        root.project("left.value").alternatives[0].place,
        "origin.left.value"
    );
}

fn test_function_operand(name: &str, params: Vec<Type>, return_type: Type) -> Operand {
    Operand::Function {
        name: name.to_string(),
        signature: Box::new(Type::Function {
            params: params
                .into_iter()
                .map(|ty| crate::sema::FunctionParamContract {
                    keyword_only: false,
                    name: String::new(),
                    ty,
                    passing: crate::ast::ReceiverKind::Value,
                    has_default: false,
                })
                .collect(),
            return_type: Box::new(return_type),
        }),
    }
}

/// A function operand whose signature matches the lowered declaration, as
/// the validator authenticates operand contracts against declarations.
fn object_contains_symbol(object: &[u8], symbol: &str) -> bool {
    object
        .windows(symbol.len())
        .any(|window| window == symbol.as_bytes())
}

fn declared_function_operand(module: &crate::mir::MirModule, name: &str) -> Operand {
    let declaration = module
        .functions
        .iter()
        .find(|function| function.name == name)
        .expect("declared function should exist");
    Operand::Function {
        name: name.to_string(),
        signature: Box::new(Type::Function {
            params: declaration
                .params
                .iter()
                .map(|param| crate::sema::FunctionParamContract {
                    keyword_only: false,
                    name: param.name.clone(),
                    ty: param.ty.clone(),
                    passing: match param.passing {
                        MirReceiverKind::Value => crate::ast::ReceiverKind::Value,
                        MirReceiverKind::Borrow => crate::ast::ReceiverKind::Borrow,
                        MirReceiverKind::BorrowMut => crate::ast::ReceiverKind::BorrowMut,
                    },
                    has_default: param.default_function.is_some(),
                })
                .collect(),
            return_type: Box::new(declaration.return_type.clone()),
        }),
    }
}

fn scalar_kind_for_tests(ty: &Type) -> Option<ScalarKind> {
    direct_type(ty, &HashMap::new()).and_then(|ty| ty.scalar_kind())
}

#[test]
fn direct_array_surface_relocates_only_to_dedicated_native_array_kernels() {
    let source = r#"
def double(value: int32) -> float64:
    return value.to_float() * 2.0

def main():
    mut values = Array[int32].from_list(values=[1, 2, 3, 4], shape=[2, 2])
    copied = values.clone()
    values.set(index=[0, 1], value=9)
    values.fill(value=2)
    item = values[0, 1]
    values[0, 1] = item
    rows = values[:1]
    added = values + copied
    scaled = 3 - values
    wrapped = values.wrapping_add(rhs=copied)
    saturated = values.saturating_mul(rhs=2)
    mapped = values.map(double)
    shape = values.shape()
    count = values.len()
    total = values.sum()
    minimum = values.min()
    maximum = values.max()
    average = values.mean()
    print(rows)
    print(added)
    print(scaled)
    print(wrapped)
    print(saturated)
    print(mapped)
    print(shape)
    print(count)
    print(total)
    print(minimum)
    print(maximum)
    print(average)
"#;
    let mir = lower_source_to_mir(source).expect("Array surface should lower to MIR");
    let object = emit_host_object(&mir).expect("Array surface should emit direct native code");
    let referenced = object_referenced_symbols(&object);
    for required in [
        "aura_direct_array_from_vec",
        "aura_direct_array_clone",
        "aura_direct_array_shape",
        "aura_direct_array_len",
        "aura_direct_array_set_in_place",
        "aura_direct_array_fill_in_place",
        "aura_direct_array_index",
        "aura_direct_array_set_index_in_place",
        "aura_direct_array_slice",
        "aura_direct_array_binary",
        "aura_direct_array_map",
        "aura_direct_array_reduce",
    ] {
        assert!(
            referenced.iter().any(|symbol| symbol.contains(required)),
            "Array lowering should reference `{required}`: {referenced:?}"
        );
    }
    assert!(
        referenced
            .iter()
            .all(|symbol| !symbol.contains("aura_direct_binary_value_at")),
        "Array operators must not fall back to the generic boxed binary helper: {referenced:?}"
    );
}

#[test]
fn direct_fixed_width_integer_methods_relocate_to_the_width_arithmetic_kernel() {
    let source = r#"
def main():
    signed: int32 = 2147483647
    signed_rhs: int32 = 2
    unsigned: uint8 = 255
    unsigned_rhs: uint8 = 1
    print(signed.wrapping_add(signed_rhs))
    print(signed.wrapping_sub(signed_rhs))
    print(signed.wrapping_mul(signed_rhs))
    print(signed.saturating_add(signed_rhs))
    print(signed.saturating_sub(signed_rhs))
    print(signed.saturating_mul(signed_rhs))
    print(unsigned.wrapping_add(unsigned_rhs))
    print(unsigned.saturating_sub(unsigned_rhs))
    print(signed.wrapping_shl(signed_rhs))
    print(signed.wrapping_shr(signed_rhs))
    print(signed.saturating_shl(signed_rhs))
    print(signed.saturating_shr(signed_rhs))
"#;
    let mir = lower_source_to_mir(source).expect("fixed-width methods should lower to MIR");
    let object =
        emit_host_object(&mir).expect("fixed-width methods should emit direct native code");
    let referenced = object_referenced_symbols(&object);

    assert!(
        referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_integer_width_binary")),
        "fixed-width methods should reference the typed width-arithmetic kernel: {referenced:?}"
    );
}

#[test]
fn direct_dynamic_binary_opcodes_match_the_runtime_abi() {
    for (operator, opcode) in [
        (BinaryOp::Add, 0),
        (BinaryOp::Sub, 1),
        (BinaryOp::Mul, 2),
        (BinaryOp::Div, 3),
        (BinaryOp::Mod, 4),
        (BinaryOp::Eq, 5),
        (BinaryOp::NotEq, 6),
        (BinaryOp::Less, 7),
        (BinaryOp::LessEq, 8),
        (BinaryOp::Greater, 9),
        (BinaryOp::GreaterEq, 10),
        (BinaryOp::And, 11),
        (BinaryOp::Or, 12),
        (BinaryOp::FloorDiv, 13),
        (BinaryOp::Pow, 14),
        (BinaryOp::BitAnd, 15),
        (BinaryOp::BitOr, 16),
        (BinaryOp::BitXor, 17),
        (BinaryOp::Shl, 18),
        (BinaryOp::Shr, 19),
    ] {
        assert_eq!(
            super::FunctionCompiler::binary_opcode(operator),
            opcode,
            "{operator:?} must retain the opcode consumed by aura_direct_binary_value_at",
        );
    }
}

#[test]
fn tuple_native_ownership_gates_separate_public_projection_from_private_destructuring() {
    assert!(validate_tuple_projection_operand(&Operand::Place("pair".to_string())).is_ok());
    let public_move = validate_tuple_projection_operand(&Operand::MovePlace("pair".to_string()))
        .expect_err("public tuple indexing must never consume a source projection");
    assert!(public_move.contains("only reads Copy elements"));

    assert!(validate_tuple_take_place("%t17").is_ok());
    let user_place = validate_tuple_take_place("pair")
        .expect_err("destructive extraction must be scoped to a private captured tuple");
    assert!(user_place.contains("whole-tuple destructuring"));

    let classes = HashMap::new();
    let tuple_type = Type::Tuple(vec![Type::named("int64"), Type::named("str")]);
    assert_eq!(
        direct_type(&tuple_type, &classes),
        Some(DirectType::Opaque(tuple_type.clone())),
        "structural tuple types use the opaque aggregate ABI"
    );
    assert!(validate_rvalue(
        &Rvalue::TupleLiteral {
            elements: vec![Operand::Int(1), Operand::String("one".to_string())],
            element_types: vec![Type::named("int64"), Type::named("str")],
        },
        &classes,
    )
    .is_ok());
    let arity_error = validate_rvalue(
        &Rvalue::TupleLiteral {
            elements: vec![Operand::Int(1)],
            element_types: vec![Type::named("int64"), Type::named("str")],
        },
        &classes,
    )
    .expect_err("tuple literal MIR metadata must preserve source arity");
    assert!(arity_error.contains("tuple literal arity 1 with 2 element types"));

    let projection_error = validate_rvalue(
        &Rvalue::TupleElement {
            tuple: Operand::MovePlace("pair".to_string()),
            index: 0,
            element_type: Type::named("int64"),
        },
        &classes,
    )
    .expect_err("public tuple projection cannot consume its source");
    assert!(projection_error.contains("only reads Copy elements"));
    assert!(validate_rvalue(
        &Rvalue::TupleElement {
            tuple: Operand::Place("pair".to_string()),
            index: 0,
            element_type: Type::named("int64"),
        },
        &classes,
    )
    .is_ok());

    let take_error = validate_rvalue(
        &Rvalue::TupleTakeElement {
            place: "pair".to_string(),
            index: 1,
            element_type: Type::named("str"),
        },
        &classes,
    )
    .expect_err("destructive extraction cannot name a user-visible place");
    assert!(take_error.contains("private captured temporary"));
    assert!(validate_rvalue(
        &Rvalue::TupleTakeElement {
            place: "%t4".to_string(),
            index: 1,
            element_type: Type::named("str"),
        },
        &classes,
    )
    .is_ok());

    assert!(!runtime_type_is_wildcard(&tuple_type));
    assert!(runtime_type_is_wildcard(&Type::Tuple(vec![
        Type::TypeParam("Element".to_string())
    ])));
    for ty in [Type::Tuple(vec![Type::named("int64")]), tuple_type.clone()] {
        let encoded = crate::native_runtime::canonical_runtime_type_name(&ty);
        assert!(
            encoded.starts_with(aura_core::type_descriptor::DESCRIPTOR_PREFIX),
            "runtime structural types use the type descriptor encoding"
        );
        assert_eq!(
            aura_core::type_descriptor::decode(&encoded)
                .expect("canonical runtime type descriptor should decode"),
            ty
        );
    }
}

/// The object the emitter asked for, before the code generator optimizes
/// it. A test that pins which functions the emitter defines, or which
/// runtime helpers it calls, reads this object, since LLVM inlines local
/// functions and folds checks it can prove.
fn emit_unoptimized_object(module: &crate::mir::MirModule) -> std::result::Result<Vec<u8>, String> {
    crate::native_backend::without_optimization(|| emit_host_object(module))
}

/// The name of the symbol a relocation refers to. LLVM's ELF objects refer
/// to a local symbol through its section's symbol and an addend, so the name
/// is the symbol defined at that place in the section. A 32-bit PC-relative
/// field counts from its own end, 4 bytes past the place it patches.
fn relocation_symbol_name(
    object: &object::File<'_>,
    relocation: &object::Relocation,
) -> Option<String> {
    use object::{RelocationKind, SymbolKind};

    let RelocationTarget::Symbol(index) = relocation.target() else {
        return None;
    };
    let symbol = object.symbol_by_index(index).ok()?;
    if symbol.kind() != SymbolKind::Section {
        return symbol.name().ok().map(str::to_string);
    }
    let section = symbol.section_index()?;
    let offset = match relocation.kind() {
        RelocationKind::Relative | RelocationKind::PltRelative if relocation.size() == 32 => {
            relocation.addend() + 4
        }
        _ => relocation.addend(),
    };
    let offset = u64::try_from(offset).ok()?;
    object
        .symbols()
        .filter(|candidate| {
            candidate.section_index() == Some(section)
                && candidate.kind() != SymbolKind::Section
                && candidate.address() <= offset
                && offset < candidate.address() + candidate.size().max(1)
        })
        .find_map(|candidate| {
            candidate
                .name()
                .ok()
                .filter(|name| !name.is_empty())
                .map(str::to_string)
        })
}

fn object_referenced_symbols(bytes: &[u8]) -> BTreeSet<String> {
    let object =
        object::File::parse(bytes).expect("direct backend output should be a readable host object");
    object
        .sections()
        .flat_map(|section| section.relocations())
        .filter_map(|(_, relocation)| relocation_symbol_name(&object, &relocation))
        .collect()
}

fn object_function_referenced_symbols(bytes: &[u8], function: &str) -> BTreeSet<String> {
    let object =
        object::File::parse(bytes).expect("direct backend output should be a readable host object");
    let symbol = object
        .symbols()
        .find(|symbol| {
            symbol
                .name()
                .is_ok_and(|name| name.trim_start_matches('_') == function)
        })
        .unwrap_or_else(|| panic!("direct object should define `{function}`"));
    let section_index = symbol
        .section_index()
        .unwrap_or_else(|| panic!("`{function}` should belong to an object section"));
    let start = symbol.address();
    let section = object
        .section_by_index(section_index)
        .expect("function section should exist");
    let start = start.saturating_sub(section.address());
    let end = if symbol.size() > 0 {
        start.saturating_add(symbol.size())
    } else {
        object
            .symbols()
            .filter(|candidate| candidate.section_index() == Some(section_index))
            .map(|candidate| candidate.address().saturating_sub(section.address()))
            .filter(|address| *address > start)
            .min()
            .unwrap_or(section.size())
    };
    section
        .relocations()
        .filter(|(offset, _)| *offset >= start && *offset < end)
        .filter_map(|(_, relocation)| relocation_symbol_name(&object, &relocation))
        .collect()
}

fn object_referenced_symbol_occurrences(bytes: &[u8], needle: &str) -> usize {
    let object =
        object::File::parse(bytes).expect("direct backend output should be a readable host object");
    object
        .sections()
        .flat_map(|section| section.relocations())
        .filter_map(|(_, relocation)| relocation_symbol_name(&object, &relocation))
        .filter(|name| name.contains(needle))
        .count()
}

#[test]
fn heterogeneous_match_arm_mutable_locals_keep_distinct_direct_slots() {
    let source =
        include_str!("../tests/fixtures/run-pass/match_arm_local_binding_slot_isolation.au");
    let module = lower_source_to_mir(source)
        .expect("heterogeneous sibling match-arm locals should lower to MIR");
    let mir_output =
        crate::run_mir(&module).expect("the MIR backend should execute both match arms");
    assert_eq!(mir_output.stdout, "Ada\n42\n");

    emit_host_object(&module)
        .expect("the direct backend should preserve each arm-local binding's own type");

    let describe = module
        .functions
        .iter()
        .find(|function| function.name == "describe")
        .expect("describe should lower");
    let arm_local_types = describe
        .local_types
        .iter()
        .filter(|local| local.name.starts_with("%t"))
        .filter_map(|local| match &local.ty {
            Type::Named(name, _) if matches!(name.as_str(), "Person" | "Score") => {
                Some((local.name.as_str(), name.as_str()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(arm_local_types.iter().any(|(_, ty)| *ty == "Person"));
    assert!(arm_local_types.iter().any(|(_, ty)| *ty == "Score"));
    assert!(
        arm_local_types.iter().all(|(slot, _)| *slot != "item"),
        "arm-local source names must be rewritten to typed MIR slots"
    );
}

#[test]
fn tuple_native_symbols_keep_public_projection_separate_from_private_take() {
    let tuple_type = Type::Tuple(vec![Type::named("int64"), Type::named("str")]);
    let module = |instructions, local_types| crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            source_path: None,
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types,
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions,
                terminator: Terminator::Return(Operand::Int(0)),
            }],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };

    let public_projection = module(
        vec![
            Instruction::Assign {
                target: "pair".to_string(),
                value: Rvalue::TupleLiteral {
                    elements: vec![Operand::Int(7), Operand::String("seven".to_string())],
                    element_types: vec![Type::named("int64"), Type::named("str")],
                },
            },
            Instruction::Assign {
                target: "first".to_string(),
                value: Rvalue::TupleElement {
                    tuple: Operand::Place("pair".to_string()),
                    index: 0,
                    element_type: Type::named("int64"),
                },
            },
        ],
        vec![
            MirLocalType {
                name: "pair".to_string(),
                ty: tuple_type.clone(),
            },
            MirLocalType {
                name: "first".to_string(),
                ty: Type::named("int64"),
            },
        ],
    );
    let public_object =
        emit_host_object(&public_projection).expect("Copy tuple projection should emit directly");
    let public_symbols = object_referenced_symbols(&public_object);
    for required in ["aura_direct_tuple_new", "aura_direct_tuple_element"] {
        assert!(
            public_symbols
                .iter()
                .any(|symbol| symbol.contains(required)),
            "public tuple projection should reference `{required}`: {public_symbols:?}"
        );
    }
    assert!(
        public_symbols
            .iter()
            .all(|symbol| !symbol.contains("aura_direct_tuple_take_element")),
        "public tuple projection must not reference destructive take: {public_symbols:?}"
    );

    let private_take = module(
        vec![
            Instruction::Assign {
                target: "pair".to_string(),
                value: Rvalue::TupleLiteral {
                    elements: vec![Operand::Int(7), Operand::String("seven".to_string())],
                    element_types: vec![Type::named("int64"), Type::named("str")],
                },
            },
            Instruction::Assign {
                target: "%t0".to_string(),
                value: Rvalue::Use(Operand::MovePlace("pair".to_string())),
            },
            Instruction::Assign {
                target: "label".to_string(),
                value: Rvalue::TupleTakeElement {
                    place: "%t0".to_string(),
                    index: 1,
                    element_type: Type::named("str"),
                },
            },
        ],
        vec![
            MirLocalType {
                name: "pair".to_string(),
                ty: tuple_type.clone(),
            },
            MirLocalType {
                name: "%t0".to_string(),
                ty: tuple_type,
            },
            MirLocalType {
                name: "label".to_string(),
                ty: Type::named("str"),
            },
        ],
    );
    let private_object =
        emit_host_object(&private_take).expect("private captured tuple take should emit directly");
    let private_symbols = object_referenced_symbols(&private_object);
    assert!(
        private_symbols
            .iter()
            .any(|symbol| symbol.contains("aura_direct_tuple_take_element")),
        "private tuple destruction should reference destructive take: {private_symbols:?}"
    );
}

#[test]
fn tuple_specialized_trait_dispatch_emits_structural_runtime_matchers() {
    let source = r#"
trait Label:
    def label(self) -> str

class Envelope[T]:
    payload: T

impl Label for Envelope[(int32, str)]:
    def label(self) -> str:
        return "integer then string"

impl Label for Envelope[(str, int32)]:
    def label(self) -> str:
        return "string then integer"

def describe[T: Label](value: T) -> str:
    return value.label()

def main():
    print(describe(Envelope[(int32, str)](payload=(7, "seven"))))
    print(describe(Envelope[(str, int32)](payload=("eight", 8))))
"#;

    // Copies of `describe` dispatch statically; the erased body matches
    // the receiver's type at run time.
    let copied = lower_source_to_mir(source).expect("tuple-specialized dispatch should lower");
    let output =
        crate::run_mir(&copied).expect("tuple-specialized dispatch should run through MIR");
    assert_eq!(output.stdout, "integer then string\nstring then integer\n");
    let bytes = emit_host_object(&copied).expect("copied dispatch should emit direct object code");
    assert!(!object_referenced_symbols(&bytes)
        .iter()
        .any(|symbol| symbol.contains("aura_direct_value_type_matches")));

    let program = crate::check_source(source).expect("tuple-specialized dispatch should check");
    let mir = crate::mir::lower_unspecialized(&program);
    let output = crate::run_mir(&mir).expect("tuple-specialized dispatch should run through MIR");
    assert_eq!(output.stdout, "integer then string\nstring then integer\n");

    let bytes =
        emit_host_object(&mir).expect("tuple-specialized dispatch should emit direct object code");
    let referenced = object_referenced_symbols(&bytes);
    assert!(
        referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_value_type_matches")),
        "dynamic tuple specialization must consult runtime type matching: {referenced:?}"
    );

    let object = object::File::parse(bytes.as_slice())
        .expect("direct tuple-dispatch output should be a readable host object");
    let data_occurrences = |needle: &[u8]| {
        object
            .sections()
            .filter_map(|section| section.data().ok())
            .map(|data| {
                data.windows(needle.len())
                    .filter(|window| *window == needle)
                    .count()
            })
            .sum::<usize>()
    };
    let tuple_payload = |elements| {
        crate::native_runtime::canonical_runtime_type_name(&Type::Tuple(elements))
            .strip_prefix(aura_core::type_descriptor::DESCRIPTOR_PREFIX)
            .expect("canonical structural runtime type tag")
            .to_string()
    };
    let int_then_string = tuple_payload(vec![Type::named("int32"), Type::named("str")]);
    let string_then_int = tuple_payload(vec![Type::named("str"), Type::named("int32")]);
    assert!(
        data_occurrences(int_then_string.as_bytes()) >= 2,
        "direct dispatch must encode a tuple matcher in addition to the enclosing class pattern"
    );
    assert!(
        data_occurrences(string_then_int.as_bytes()) >= 2,
        "direct dispatch must encode a second tuple matcher in addition to its enclosing class pattern"
    );
}

#[test]
fn d3_native_unhinted_integer_operands_use_the_unboxed_int64_path() {
    assert_eq!(
        infer_operand_type(&Operand::Int(7), &HashMap::new(), &HashMap::new()),
        Some(DirectType::Scalar(ScalarKind::Int64))
    );

    let source = r#"
def identity(value: int64) -> int64:
    return value

def main() -> int32:
    value = 2147483648
    print(identity(value))
    return 0
"#;
    let mir = lower_source_to_mir(source).expect("default int64 source should lower to MIR");
    let object = emit_host_object(&mir).expect("default int64 source should compile directly");
    let referenced = object_referenced_symbols(&object);

    for forbidden in [
        "aura_direct_box_uint_literal",
        "aura_direct_binary_value_at",
        "aura_direct_cast_value_at",
        "aura_direct_unbox_i64",
    ] {
        assert!(
            !referenced.iter().any(|symbol| symbol.contains(forbidden)),
            "default int64 values must not reference `{forbidden}`: {referenced:?}"
        );
    }
}

#[test]
fn direct_duration_api_uses_centralized_constructor_and_conversion_dispatch() {
    let source = r#"
def main() -> int32:
    base = Duration.ms(125)
    seconds = Duration.seconds(2)
    minutes = Duration.minutes(-1)
    combined = base + seconds
    scaled = 3 * combined
    reverse_scaled = combined * 3
    print(scaled // 2)
    print(reverse_scaled)
    print(minutes < 0ms)
    print(combined.to_ms())
    print(combined.to_seconds())
    return 0
"#;
    let mir = lower_source_to_mir(source).expect("Duration API source should lower to MIR");
    let object = emit_host_object(&mir).expect("Duration API should compile directly");
    let referenced = object_referenced_symbols(&object);

    for required in [
        "aura_direct_duration_from_i64",
        "aura_direct_duration_to_float",
        "aura_direct_binary_value_at",
    ] {
        assert!(
            referenced.iter().any(|symbol| symbol.contains(required)),
            "Duration direct code should reference `{required}`: {referenced:?}"
        );
    }
}

#[test]
fn direct_select_packages_variadic_sources_into_the_owned_tuple_abi() {
    let module = module_with_main_call_result_type(
        Rvalue::Call {
            callee: CallTarget::Name("select".to_string()),
            args: vec![MirArg {
                name: None,
                value: Operand::Duration(0),
                writeback_place: None,
            }],
            type_args: Default::default(),
        },
        Type::Named("SelectOutcome".to_string(), vec![Type::Unit, Type::Unit]),
    );
    let object = emit_host_object(&module)
        .expect("typed select should package direct sources into an owned tuple");
    let referenced = object_referenced_symbols(&object);
    for required in [
        "aura_direct_duration_literal",
        "aura_direct_arg_buffer_new",
        "aura_direct_arg_buffer_store_owned",
        "aura_direct_tuple_new",
        "aura_direct_select",
        "aura_direct_release_value",
    ] {
        assert!(
            referenced.iter().any(|symbol| symbol.contains(required)),
            "direct select code should reference `{required}`: {referenced:?}"
        );
    }
    assert!(
        referenced
            .iter()
            .all(|symbol| !symbol.contains("aura_host_")),
        "select must use its internal tuple ABI rather than generic host dispatch: {referenced:?}"
    );

    let nonrepeatable_task_source = r#"
def worker() -> str:
    return "value"

def main() -> int32:
    with TaskGroup() as group:
        task = group.start(worker)
        outcome = select(task, 0ms)
    return 0
"#;
    let mir = lower_source_to_mir(nonrepeatable_task_source)
        .expect("a nonrepeatable Task select source should lower");
    let object = emit_host_object(&mir)
        .expect("the direct select tuple must accept a moved nonrepeatable Task source");
    assert!(
        object_referenced_symbols(&object)
            .iter()
            .any(|symbol| symbol.contains("aura_direct_select")),
        "source-level nonrepeatable Task selection must use the direct select ABI"
    );
}

#[test]
fn direct_random_api_uses_dedicated_borrowed_runtime_symbols() {
    let source = r#"
import random

class Item:
    label: str

class Holder:
    values: list[Item]

def main() -> int32:
    mut rng = random.Rng(seed=42)
    print(rng.next_int(lo=0, hi=10))
    print(rng.next_float())
    mut holder = Holder([Item("a"), Item("b"), Item("c")])
    rng.shuffle(values=holder.values)
    print(random.secure_int(5, 6))
    print(random.secure_bytes(0).len())
    return 0
"#;
    let mir = lower_source_to_mir(source).expect("Randomness API source should lower to MIR");
    let object = emit_host_object(&mir).expect("Randomness API should compile directly");
    let referenced = object_referenced_symbols(&object);

    for required in [
        "aura_direct_rng_new",
        "aura_direct_rng_next_int",
        "aura_direct_rng_next_float",
        "aura_direct_rng_shuffle",
        "aura_direct_random_secure_int",
        "aura_direct_random_secure_bytes",
    ] {
        assert!(
            referenced.iter().any(|symbol| symbol.contains(required)),
            "Randomness direct code should reference `{required}`: {referenced:?}"
        );
    }
}

#[test]
fn adr0061_returned_element_views_hand_selectors_to_the_caller() {
    let source = include_str!("../tests/fixtures/run-pass/returned_element_views.au");
    let mir = lower_source_to_mir(source).expect("returned element views should lower to MIR");
    let object = emit_host_object(&mir).expect("returned element views should compile directly");
    let referenced = object_referenced_symbols(&object);
    for symbol in [
        "aura_direct_push_returned_view_selector",
        "aura_direct_take_returned_view_selector",
    ] {
        assert!(
            referenced.iter().any(|name| name.contains(symbol)),
            "missing {symbol}: {referenced:?}"
        );
    }
}

#[test]
fn direct_opaque_user_clone_dispatches_to_the_declared_trait_method() {
    let source = include_str!("../tests/fixtures/run-pass/random_opaque_user_clone_dispatch.au");
    let mir = lower_source_to_mir(source).expect("opaque Holder clone source should lower to MIR");
    let object = emit_host_object(&mir).expect("opaque Holder clone should compile directly");
    let referenced = object_referenced_symbols(&object);

    assert!(
        referenced
            .iter()
            .all(|symbol| !symbol.contains("aura_direct_clone_value")),
        "the user-declared clone method must not fall through to opaque runtime cloning: {referenced:?}"
    );
}

#[test]
fn direct_rng_nonbuiltin_trait_dispatch_preserves_builtin_member_dispatch() {
    let source = include_str!("../tests/fixtures/run-pass/random_rng_trait_dispatch.au");
    let mir = lower_source_to_mir(source).expect("Rng trait source should lower to MIR");
    let object = emit_host_object(&mir).expect("Rng trait source should compile directly");
    let referenced = object_referenced_symbols(&object);

    assert!(
        referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_rng_next_int")),
        "the builtin Rng.next_int call must retain its dedicated runtime dispatch: {referenced:?}"
    );
}

#[test]
fn direct_user_defined_rng_does_not_reference_random_runtime_symbols() {
    let source = r#"
class Rng:
    value: int64

    def next_int(self, lo: int64, hi: int64) -> int64:
        return self.value

    def next_float(self) -> str:
        return "local"

    def shuffle(self, value: int64) -> int64:
        return value

def main() -> int32:
    rng = Rng(5)
    print(rng.next_int(0, 10))
    print(rng.next_float())
    print(rng.shuffle(7))
    return 0
"#;
    let mir = lower_source_to_mir(source).expect("the local Rng class should lower to MIR");
    let object = emit_host_object(&mir).expect("the local Rng class should compile directly");
    let referenced = object_referenced_symbols(&object);

    assert!(
        !referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_rng_")),
        "a user class named Rng must not reference the random.Rng runtime: {referenced:?}"
    );
}

#[test]
fn direct_path_named_random_keeps_user_rng_constructors_out_of_builtin_runtime() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/run-pass/random.au");
    let mir = crate::lower_path_to_mir(&path)
        .expect("path-level user Rng fixture should lower with source provenance");
    let object = emit_host_object(&mir)
        .expect("path-level local and imported user Rng classes should compile directly");
    let referenced = object_referenced_symbols(&object);

    assert!(
        !referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_rng_")),
        "user classes named Rng must not reference the random.Rng runtime: {referenced:?}"
    );
}

#[test]
fn direct_monotonic_time_uses_scalar_runtime_abi_without_generic_host_boxing() {
    let source = r#"
import sys

def main() -> int32:
    start: int64 = sys.monotonic_time_ms()
    finish: int64 = sys.monotonic_time_ms()
    if finish < start:
        return 1
    return 0
"#;
    let mir = lower_source_to_mir(source).expect("monotonic clock source should lower to MIR");
    let object =
        emit_unoptimized_object(&mir).expect("monotonic clock source should compile directly");
    let referenced = object_function_referenced_symbols(&object, "aura_fn_main");

    assert!(
        referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_monotonic_time_ms")),
        "direct monotonic clock calls must use the scalar runtime ABI: {referenced:?}"
    );
    for generic_symbol in [
        "aura_host_sys_monotonic_time_ms",
        "aura_direct_arg_buffer_new",
        "aura_direct_box_i64",
        "aura_direct_unbox_int64",
    ] {
        assert!(
            !referenced
                .iter()
                .any(|symbol| symbol.contains(generic_symbol)),
            "direct monotonic clock calls must not use `{generic_symbol}`: {referenced:?}"
        );
    }
}

#[test]
fn direct_sleep_uses_void_runtime_abi_without_boxing_unit() {
    let source = r#"
def main() -> int32:
    sleep(0ms)
    return 0
"#;
    let mir = lower_source_to_mir(source).expect("sleep source should lower to MIR");
    let object = emit_host_object(&mir).expect("sleep source should compile directly");
    let referenced = object_referenced_symbols(&object);

    assert!(
        referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_sleep_value_void")),
        "direct sleep calls must use the void runtime ABI: {referenced:?}"
    );
    assert!(
        !referenced
            .iter()
            .any(|symbol| symbol.ends_with("aura_direct_sleep_value")),
        "direct sleep calls must not allocate a boxed Unit result: {referenced:?}"
    );
}

#[test]
fn direct_yield_now_uses_void_runtime_abi_without_boxing_unit() {
    let source = r#"
def main() -> int32:
    yield_now()
    return 0
"#;
    let mir = lower_source_to_mir(source).expect("yield_now source should lower to MIR");
    let object = emit_host_object(&mir).expect("yield_now source should compile directly");
    let referenced = object_referenced_symbols(&object);

    assert!(
        referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_yield_now")),
        "direct yield_now calls must use the void runtime ABI: {referenced:?}"
    );
    assert!(
        !referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_yield_now_value")),
        "direct yield_now calls must not use a boxed-value ABI: {referenced:?}"
    );
}

#[test]
fn native_loop_safepoint_uses_void_yield_runtime_abi_when_tasks_can_run() {
    let source = r#"
def worker(limit: int32) -> int32:
    mut value: int32 = 0
    while value < limit:
        value += 1
    return value

def main() -> int32:
    with TaskGroup() as group:
        task = group.start(worker, 8)
        mut value: int32 = 0
        while value < 8:
            value += 1
        match task.result():
            case TaskResult.Ready(result):
                return result
            case _:
                return 1
"#;
    let mir = lower_source_to_mir(source).expect("concurrent loop source should lower to MIR");
    assert!(
        mir.functions.iter().any(|function| {
            function.blocks.iter().any(|block| {
                block
                    .instructions
                    .iter()
                    .any(|instruction| matches!(instruction, Instruction::Safepoint))
            })
        }),
        "loop lowering must retain the portable MIR safepoint marker"
    );
    assert!(
        mir.functions.iter().any(|function| {
            function.blocks.iter().any(|block| {
                block.instructions.iter().any(|instruction| {
                    matches!(
                        instruction,
                        Instruction::Assign {
                            value: Rvalue::StartTask { .. },
                            ..
                        }
                    )
                })
            })
        }),
        "the fixture must preserve a runnable sibling task"
    );

    let object = emit_host_object(&mir).expect("concurrent loop source should compile directly");
    let referenced = object_referenced_symbols(&object);
    assert!(
        referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_yield_now")),
        "a concurrent native loop must call the void scheduler ABI: {referenced:?}"
    );
    assert!(
        !referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_yield_now_value")),
        "an automatic safepoint must not request a boxed Unit result: {referenced:?}"
    );
}

#[test]
fn direct_task_lowering_preserves_specialized_repeatability_and_owned_observers() {
    let source = r#"
def relay[T](value: own T) -> T:
    return value

def int_worker() -> int64:
    return 1

def string_worker() -> str:
    return "value"

def main() -> int32:
    with TaskGroup() as group:
        int_task = group.start(relay[int64], 1)
        int_result = int_task.result_or(0)

        string_task = group.start_with_stack(262144, relay[str], "value")
        string_result = string_task.result_or("")

        int_tasks = [group.start(int_worker)]
        any_int = wait_any(int_tasks)
        string_tasks = [group.start(string_worker)]
        all_strings = wait_all(string_tasks)
    return 0
"#;
    let mir = lower_source_to_mir(source)
        .expect("specialized task ownership source should lower to direct MIR");
    let main = mir
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main should lower");
    let starts = main
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::Assign {
                value:
                    Rvalue::StartTask {
                        result_is_copy,
                        stack_size,
                        ..
                    },
                ..
            } => Some((*result_is_copy, stack_size.is_some())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        starts,
        vec![(true, false), (false, true), (true, false), (false, false)]
    );
    assert!(main
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .any(|instruction| matches!(
            instruction,
            Instruction::Assign {
                value: Rvalue::Call {
                    callee: CallTarget::Runtime {
                        op: crate::runtime_ops::RuntimeOp::Task(crate::runtime_ops::TaskOp::ResultOr),
                        receiver: Some(Operand::MovePlace(place)),
                        ..
                    },
                    ..
                },
                ..
            } if place == "string_task"
        )));
    assert!(main
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .any(|instruction| matches!(
            instruction,
            Instruction::Assign {
                value: Rvalue::Call {
                    callee: CallTarget::Runtime {
                        op, receiver: None, ..
                    },
                    args, ..
                },
                ..
            } if op.name() == "wait_all"
                && matches!(
                    args.first().map(|arg| &arg.value),
                    Some(Operand::MovePlace(place)) if place == "string_tasks"
                )
        )));

    let object = emit_host_object(&mir)
        .expect("specialized task starts and consuming observers should emit directly");
    assert!(!object.is_empty());
}

#[test]
fn native_loop_safepoint_elides_runtime_call_when_no_sibling_can_run() {
    let source = r#"
def main() -> int32:
    mut value: int32 = 0
    while value < 8:
        value += 1
    return value
"#;
    let mir = lower_source_to_mir(source).expect("sequential loop source should lower to MIR");
    assert!(
        mir.functions.iter().any(|function| {
            function.blocks.iter().any(|block| {
                block
                    .instructions
                    .iter()
                    .any(|instruction| matches!(instruction, Instruction::Safepoint))
            })
        }),
        "static native elision must not remove the backend-independent MIR marker"
    );

    let object = emit_host_object(&mir).expect("sequential loop source should compile directly");
    let referenced = object_referenced_symbols(&object);
    assert!(
        !referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_yield_now")),
        "a module with no task start has no sibling to schedule and must elide the call: \
         {referenced:?}"
    );
}

#[test]
fn generated_functions_push_frames_inline_and_keep_the_runtime_paths() {
    let source = "def leaf(n: int32) -> int32:\n    return n + 1\n\ndef main() -> int32:\n    return leaf(1)\n";
    let mir = lower_source_to_mir(source).expect("source should lower");
    let object = emit_host_object_with_metadata(&mir, "/workspace/main.au", source).expect("emit");
    let referenced = object_referenced_symbols(&object);
    for symbol in [
        "aura_direct_frame_stack",
        "aura_direct_enter_frame",
        "aura_direct_exit_call",
        "aura_direct_check_frame_stack_layout",
        "aura_direct_validate_frame_descriptors",
    ] {
        assert!(
            referenced.iter().any(|name| name.ends_with(symbol)),
            "generated code must reference `{symbol}`: {referenced:?}"
        );
    }
}

#[test]
fn generated_retains_and_releases_count_in_place_and_keep_the_runtime_paths() {
    let source = "def echo(value: str):\n    print(value)\n\ndef main() -> int32:\n    mut text = \"hello\"\n    echo(text)\n    text = \"goodbye\"\n    print(text)\n    return 0\n";
    let mir = lower_source_to_mir(source).expect("source should lower");
    // LLVM removes this program's retains, so the test reads the object
    // the emitter asked for.
    let object = crate::native_backend::without_optimization(|| {
        emit_host_object_with_metadata(&mir, "/workspace/main.au", source)
    })
    .expect("emit");
    let referenced = object_referenced_symbols(&object);
    for symbol in [
        "aura_direct_release_value",
        "aura_direct_release_last_owned_value",
        "aura_direct_check_ownership_layout",
        "aura_direct_retain_value",
    ] {
        assert!(
            referenced.iter().any(|name| name.ends_with(symbol)),
            "generated code must reference `{symbol}`: {referenced:?}"
        );
    }
}

#[test]
fn the_ownership_fast_paths_report_backend_errors() {
    use super::{declare_ownership_fast_paths, declare_runtime_helper};
    use crate::native_backend::{host_module_backend, Linkage};
    use crate::runtime_abi::RuntimeTable;

    for (name, expected) in [
        (
            "aura_owned_retain",
            "failed to declare the retain fast path: ",
        ),
        (
            "aura_owned_release",
            "failed to declare the release fast path: ",
        ),
    ] {
        let mut object = host_module_backend().expect("host module");
        let mut runtime =
            RuntimeTable::try_build(|function| declare_runtime_helper(&mut *object, function))
                .expect("runtime helpers");
        object
            .declare_function(name, Linkage::Local, &Signature::new(CallConv::C))
            .expect("declare the clashing function");
        let error = match declare_ownership_fast_paths(object.as_mut(), &mut runtime) {
            Ok(_) => panic!("`{name}` clashes"),
            Err(error) => error,
        };
        assert!(error.starts_with(expected), "{error}");
    }
}

#[test]
fn the_frame_descriptor_table_reports_backend_errors() {
    use super::declare_frame_descriptor_table;
    use crate::native_backend::{host_module_backend, Linkage};

    let mut taken = host_module_backend().expect("host module");
    taken
        .declare_function(
            "aura_frame_descriptors",
            Linkage::Local,
            &Signature::new(CallConv::C),
        )
        .expect("declare the clashing function");
    let error = declare_frame_descriptor_table(taken.as_mut(), &[]).expect_err("name clash");
    assert!(
        error.starts_with("failed to declare the frame descriptor table: "),
        "{error}"
    );

    let mut twice = host_module_backend().expect("host module");
    declare_frame_descriptor_table(twice.as_mut(), &[]).expect("first table");
    let error = declare_frame_descriptor_table(twice.as_mut(), &[]).expect_err("second table");
    assert!(
        error.starts_with("failed to define the frame descriptor table: "),
        "{error}"
    );
}

#[test]
fn native_codegen_forwards_exact_call_and_task_frame_metadata_to_runtime_abis() {
    let source = r#"
def child() -> int32:
    return 7

def main() -> int32:
    with TaskGroup() as group:
        task = group.start(child)
        match task.result():
            case TaskResult.Ready(value):
                return value
            case _:
                return 0
"#;
    let mut mir = lower_source_to_mir(source).expect("task-frame source should lower");
    let child = mir
        .functions
        .iter_mut()
        .find(|function| function.name == "child")
        .expect("child should lower");
    child.source_path = Some("/workspace/pkg/child.au".to_string());
    let main = mir
        .functions
        .iter_mut()
        .find(|function| function.name == "main")
        .expect("main should lower");
    main.source_path = Some("/workspace/app/main.au".to_string());

    let object = emit_host_object_with_metadata(&mir, "/workspace/app/main.au", source)
        .expect("frame-aware task source should emit directly");
    let referenced = object_referenced_symbols(&object);
    assert!(
        referenced
            .iter()
            .any(|symbol| symbol.ends_with("aura_direct_enter_frame")),
        "generated function entry must pass one static frame descriptor: {referenced:?}"
    );
    assert!(
        !referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_enter_call_with_frame")),
        "generated function entry must not pass frame text on every call: {referenced:?}"
    );
    assert!(
        referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_start_task_function_with_frames")),
        "generated task start must use the function-value ancestry-carrying ABI: {referenced:?}"
    );
    for legacy_abi in [
        "aura_direct_start_task_call_with_frames",
        "aura_direct_start_task_call",
    ] {
        assert!(
            !referenced.iter().any(|symbol| symbol.ends_with(legacy_abi)),
            "generated code must not silently fall back to legacy task ABI `{legacy_abi}`"
        );
    }

    let parsed = object::File::parse(object.as_slice())
        .expect("frame-aware output should be a readable host object");
    let data_contains = |needle: &[u8]| {
        parsed.sections().any(|section| {
            section
                .data()
                .ok()
                .is_some_and(|data| data.windows(needle.len()).any(|window| window == needle))
        })
    };
    assert!(data_contains(b"/workspace/pkg/child.au"));
    assert!(data_contains(b"/workspace/app/main.au"));
    assert!(data_contains(b"child"));
    assert!(data_contains(b"main"));
}

#[test]
fn handbuilt_mir_safepoint_validates_and_emits_for_a_sequential_module() {
    let function = MirFunction {
        name: "main".to_string(),
        module_name: "<test>".to_string(),
        source_path: None,
        span: Span::new(1, 1),
        receiver: None,
        params: Vec::new(),
        local_types: Vec::new(),
        return_type: Type::named("int32"),
        entry: "entry".to_string(),
        blocks: vec![BasicBlock {
            label: "entry".to_string(),
            instructions: vec![Instruction::Safepoint],
            terminator: Terminator::Return(Operand::Int(0)),
        }],
    };
    validate_function(&function, &HashMap::new())
        .expect("a hand-built MIR safepoint is a valid backend instruction");

    let module = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![function],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };
    let object = emit_host_object(&module).expect("a hand-built MIR safepoint should emit");
    let referenced = object_referenced_symbols(&object);
    assert!(
        !referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_yield_now")),
        "hand-built sequential MIR must receive the same static elision: {referenced:?}"
    );
}

#[test]
fn dead_task_starts_do_not_enable_reachable_native_safepoints() {
    let worker = MirFunction {
        name: "worker".to_string(),
        module_name: "<test>".to_string(),
        source_path: None,
        span: Span::new(1, 1),
        receiver: None,
        params: Vec::new(),
        local_types: Vec::new(),
        return_type: Type::Unit,
        entry: "entry".to_string(),
        blocks: vec![BasicBlock {
            label: "entry".to_string(),
            instructions: Vec::new(),
            terminator: Terminator::Return(Operand::Unit),
        }],
    };
    let main = MirFunction {
        name: "main".to_string(),
        module_name: "<test>".to_string(),
        source_path: None,
        span: Span::new(1, 1),
        receiver: None,
        params: Vec::new(),
        local_types: vec![MirLocalType {
            name: "dead_task".to_string(),
            ty: Type::Unit,
        }],
        return_type: Type::named("int32"),
        entry: "entry".to_string(),
        blocks: vec![
            BasicBlock {
                label: "entry".to_string(),
                instructions: vec![Instruction::Safepoint],
                terminator: Terminator::Return(Operand::Int(0)),
            },
            BasicBlock {
                label: "dead".to_string(),
                instructions: vec![Instruction::Assign {
                    target: "dead_task".to_string(),
                    value: Rvalue::StartTask {
                        returns_handle: false,
                        result_is_copy: true,
                        stack_size: None,
                        task_group: Operand::Unit,
                        function: Operand::Function {
                            name: "worker".to_string(),
                            signature: Box::new(Type::Function {
                                params: Vec::new(),
                                return_type: Box::new(Type::Unit),
                            }),
                        },
                        args: Vec::new(),
                        span: Span::new(1, 1),
                    },
                }],
                terminator: Terminator::Return(Operand::Int(0)),
            },
        ],
    };
    let module = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![main, worker],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };

    let object = emit_host_object(&module)
        .expect("a dead task start must not contaminate reachable safepoint lowering");
    let referenced = object_referenced_symbols(&object);
    assert!(
        !referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_yield_now")),
        "unreachable task metadata must not enable scheduler calls: {referenced:?}"
    );
}

#[test]
fn handbuilt_mir_safepoint_does_not_mask_a_malformed_terminator() {
    let function = MirFunction {
        name: "main".to_string(),
        module_name: "<test>".to_string(),
        source_path: None,
        span: Span::new(1, 1),
        receiver: None,
        params: Vec::new(),
        local_types: Vec::new(),
        return_type: Type::named("int32"),
        entry: "entry".to_string(),
        blocks: vec![BasicBlock {
            label: "entry".to_string(),
            instructions: vec![Instruction::Safepoint],
            terminator: Terminator::Unreachable,
        }],
    };

    // The direct backend compiles every MIR terminator; an unreachable one
    // after a safepoint is a trap.
    validate_function(&function, &HashMap::new())
        .expect("a safepoint before an unreachable terminator is valid");
}

#[test]
fn direct_dynamic_json_surface_uses_the_host_builtin_abi() {
    let source = r#"
import json

def main() -> int32:
    match json.parse("{\"ready\":true,\"count\":2}"):
        case Result.Ok(value):
            print(json.dumps(value))
        case Result.Err(error):
            print(error)

    print(json.is_null(json.Value.Null))
    print(json.as_bool(json.Value.Bool(true)))
    print(json.as_int(json.Value.Int(7)))
    print(json.as_float(json.Value.Float(1.5)))
    print(json.into_string(json.Value.String("aura")))
    print(json.into_array(json.Value.Array([json.Value.Int(1)])))
    print(json.into_object(json.Value.Object({"value": json.Value.Int(1)})))
    return 0
"#;

    let mir = lower_source_to_mir(source).expect("dynamic JSON source should lower to MIR");
    let object =
        emit_unoptimized_object(&mir).expect("dynamic JSON source should compile directly");
    let referenced = object_referenced_symbols(&object);
    for entry in [
        "aura_host_json_parse",
        "aura_host_json_dumps",
        "aura_host_json_is_null",
        "aura_host_json_as_bool",
        "aura_host_json_as_int",
        "aura_host_json_as_float",
        "aura_host_json_into_string",
        "aura_host_json_into_array",
        "aura_host_json_into_object",
    ] {
        assert!(
            referenced.iter().any(|symbol| symbol.contains(entry)),
            "dynamic JSON direct code must call its own entry `{entry}`: {referenced:?}"
        );
    }
    assert!(
        referenced
            .iter()
            .all(|symbol| !symbol.contains("aura_direct_host_builtin")),
        "dynamic JSON direct code must not use a shared name dispatcher: {referenced:?}"
    );
    let parse = object_function_referenced_symbols(&object, "aura_fn_json__parse");
    assert!(
        parse
            .iter()
            .any(|symbol| symbol.trim_start_matches('_') == "aura_host_json_parse")
            && parse.iter().all(|symbol| {
                let symbol = symbol.trim_start_matches('_');
                !symbol.starts_with("aura_host_") || symbol == "aura_host_json_parse"
            }),
        "`json.parse` must call only its own entry: {parse:?}"
    );
}

#[test]
fn direct_backend_emits_object_for_supported_scalar_program() {
    let source = "def helper(value: int32) -> int32:\n    return value + 2\n\ndef main() -> int32:\n    mut current: int32 = 1\n    if current < 5:\n        current = helper(value=current)\n    print(current)\n    return 0\n";

    let mir = lower_source_to_mir(source).expect("source should lower to MIR");
    let object = emit_host_object(&mir).expect("direct backend should emit an object");

    assert!(!object.is_empty());
}

#[cfg(target_arch = "x86_64")]
#[test]
fn direct_backend_x86_64_emits_three_integer_results_for_mutable_receiver_writeback() {
    let source = r#"
class Probe:
    condition_calls: int32
    message_calls: int32

    def condition(mut self) -> bool:
        self.condition_calls += 1
        return false

def main():
    mut probe = Probe(condition_calls=0, message_calls=0)
    assert probe.condition()
"#;
    let mir = lower_source_to_mir(source).expect("mutable receiver source should lower");
    let object = emit_host_object(&mir)
        .expect("x86-64 should spill flattened writeback results through a return area");
    assert!(!object.is_empty());
}

#[test]
fn ticket9_direct_backend_emits_unboxed_int64_ten_million_loop() {
    let source = r#"
def count_to(limit: int64) -> int64:
    mut current: int64 = 0
    while current < limit:
        current += 1
    return current

def main() -> int32:
    print(count_to(10000000))
    return 0
"#;

    let mir = lower_source_to_mir(source).expect("int64 loop should lower to MIR");
    let object = emit_host_object(&mir)
        .expect("the direct backend should promote loop literals into unboxed int64 values");
    let referenced = object_referenced_symbols(&object);

    assert!(!object.is_empty());
    assert!(
        !referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_binary_value_at")),
        "int64 loop arithmetic and comparison must not use boxed runtime dispatch: {referenced:?}"
    );
    assert!(
        !referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_cast_value_at")),
        "int64 contextual literals must be promoted without a boxed runtime cast: {referenced:?}"
    );
}

#[test]
fn ticket9_wide_scalar_casts_avoid_boxed_runtime_dispatch() {
    let source = r#"
def main() -> int32:
    values: list[int64] = [42]
    signed: int64 = values[0]
    unsigned: uint64 = signed as uint64
    signed_again: int64 = unsigned as int64
    floating: float64 = signed_again as float64
    from_float: uint64 = floating as uint64
    print(from_float)
    return 0
"#;

    let mir = lower_source_to_mir(source).expect("wide scalar casts should lower to MIR");
    let object = emit_host_object(&mir).expect("wide scalar casts should emit directly");
    let referenced = object_referenced_symbols(&object);

    {
        let forbidden = "aura_direct_cast_value_at";
        assert!(
            !referenced.iter().any(|symbol| symbol.contains(forbidden)),
            "wide scalar casts must not reference `{forbidden}`: {referenced:?}"
        );
    }
    assert!(
        referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_cast_integer_to_integer")),
        "a wide integer cast that fails should report through the unboxed checked helper"
    );
    assert!(
        referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_cast_integer_to_float")),
        "an inexact integer-to-float cast should take the unboxed exactness helper"
    );
    assert!(
        referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_cast_float_to_integer")),
        "a float-to-wide-integer cast out of range should report through the unboxed checked helper"
    );
}

#[test]
fn ticket9_int64_opaque_results_use_the_checked_int64_unbox_helper() {
    // `abs` of a local computes in words; a literal argument still takes
    // the boxed helper, whose result unboxes through the checked helper.
    let source = r#"
def main() -> int32:
    value: int64 = abs(9223372036854775807)
    print(value)
    return 0
"#;

    let mir = lower_source_to_mir(source).expect("int64 abs overflow source should lower");
    let object = emit_host_object(&mir).expect("int64 abs overflow source should emit directly");
    let referenced = object_referenced_symbols(&object);

    assert!(
        referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_unbox_int64")),
        "opaque int64 results must use the target-aware checked helper: {referenced:?}"
    );
}

#[test]
fn ticket9_int64_task_thunks_use_the_checked_int64_unbox_helper() {
    let source = r#"
def echo(value: int64) -> int64:
    return value

def main() -> int32:
    with TaskGroup() as group:
        task = group.start(echo, -9223372036854775808)
        match task.result():
            case TaskResult.Ready(value):
                print(value)
            case _:
                return 1
    return 0
"#;

    let mir = lower_source_to_mir(source).expect("int64 task thunk source should lower");
    let object = emit_host_object(&mir).expect("int64 task thunk source should emit directly");
    let referenced = object_referenced_symbols(&object);

    assert!(
        referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_unbox_int64")),
        "int64 task thunks must use the target-aware checked helper: {referenced:?}"
    );
}

#[test]
fn ticket9_wide_boundary_literals_stay_unboxed() {
    let source = r#"
def main() -> int32:
    minimum: int64 = -9223372036854775808
    maximum: uint64 = 18446744073709551615
    print(minimum)
    print(maximum)
    return 0
"#;

    let mir = lower_source_to_mir(source).expect("wide boundaries should lower to MIR");
    let object = emit_host_object(&mir).expect("wide boundaries should emit directly");
    let referenced = object_referenced_symbols(&object);

    for forbidden in [
        "aura_direct_unary_value_at",
        "aura_direct_box_uint_literal",
        "aura_direct_unbox_i64",
        "aura_direct_unbox_u64",
    ] {
        assert!(
            !referenced.iter().any(|symbol| symbol.contains(forbidden)),
            "wide boundary literals must not reference `{forbidden}`: {referenced:?}"
        );
    }
}

#[test]
fn ticket9_expected_uint64_operands_avoid_generic_literal_boxing() {
    let source = r#"
class Holder:
    value: uint64

    def echo(self, value: uint64) -> uint64:
        return value

def take(value: uint64) -> uint64:
    return value

def maximum() -> uint64:
    return 18446744073709551615

def main() -> int32:
    holder = Holder(value=18446744073709551615)
    direct = take(18446744073709551615)
    method = holder.echo(18446744073709551615)
    casted: uint64 = 18446744073709551615 as uint64
    print(maximum())
    print(direct)
    print(method)
    print(casted)
    return 0
"#;

    let mir = lower_source_to_mir(source).expect("expected uint64 operands should lower");
    let object =
        emit_unoptimized_object(&mir).expect("expected uint64 operands should emit directly");
    let referenced = [
        "aura_fn_Holder_echo",
        "aura_fn_take",
        "aura_fn_maximum",
        "aura_fn_main",
    ]
    .into_iter()
    .flat_map(|function| object_function_referenced_symbols(&object, function))
    .collect::<BTreeSet<_>>();

    for forbidden in [
        "aura_direct_box_uint_literal",
        "aura_direct_unbox_u64",
        "aura_direct_cast_value_at",
    ] {
        assert!(
            !referenced.iter().any(|symbol| symbol.contains(forbidden)),
            "expected uint64 operands must not reference `{forbidden}`: {referenced:?}"
        );
    }
}

#[test]
fn ticket9_uint64_opaque_boundaries_use_typed_boxing_without_generic_detours() {
    let source = r#"
def main() -> int32:
    values: list[uint64] = [18446744073709551615]
    limits: dict[str, uint64] = {"max": 18446744073709551615}
    maybe: uint64 | None = 18446744073709551615
    print(values.len())
    print(limits.len())
    print(maybe != None)
    return 0
"#;

    let mir = lower_source_to_mir(source).expect("typed uint64 containers should lower");
    let object = emit_host_object(&mir).expect("typed uint64 containers should emit directly");
    let referenced = object_referenced_symbols(&object);

    assert!(
        referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_box_u64")),
        "opaque uint64 boundaries should use the typed boxing helper"
    );
    for forbidden in ["aura_direct_box_uint_literal", "aura_direct_unbox_u64"] {
        assert!(
            !referenced.iter().any(|symbol| symbol.contains(forbidden)),
            "typed opaque uint64 boundaries must not reference `{forbidden}`: {referenced:?}"
        );
    }
}

#[test]
fn direct_backend_emits_contextual_none_and_unit_equality() {
    let source = include_str!("../tests/fixtures/run-pass/contextual_none_equality.au");
    let mir = lower_source_to_mir(source).expect("contextual None source should lower to MIR");
    let object = emit_host_object(&mir)
        .expect("direct backend should support contextual None and scalar unit equality");

    assert!(!object.is_empty());
}

#[test]
fn direct_backend_preserves_integer_call_types_through_equality_contexts() {
    let source = include_str!("../tests/fixtures/run-pass/integer_call_equality.au");
    let mir = lower_source_to_mir(source).expect("integer equality source should lower to MIR");

    emit_host_object(&mir)
        .expect("direct equality must preserve int32 and uint64 function-call result types");
}

#[test]
fn direct_backend_emits_retain_and_release_hooks_for_opaque_call_and_local_flow() {
    let source = r#"
def echo(value: str):
    print(value)

def main() -> int32:
    mut text = "hello"
    echo(text)
    text = "goodbye"
    print(text)
    return 0
"#;

    let mir = lower_source_to_mir(source).expect("source should lower to MIR");
    let object = emit_unoptimized_object(&mir).expect("direct backend should emit an object");
    let rendered = String::from_utf8_lossy(&object);

    assert!(rendered.contains("aura_direct_retain_value"));
    assert!(rendered.contains("aura_direct_release_value"));
}

#[test]
fn direct_backend_emits_object_for_plain_class_programs() {
    let source = include_str!("../../../examples/point.au");
    let mir = lower_source_to_mir(source).expect("point example should lower to MIR");
    let object = emit_host_object(&mir).expect("plain classes should now be supported directly");

    assert!(!object.is_empty());
}

#[test]
fn direct_backend_emits_object_for_trait_impl_dispatch() {
    let source = include_str!("../../../examples/traits/greeter.au");
    let mir = lower_source_to_mir(source).expect("trait example should lower to MIR");
    let object = emit_host_object(&mir).expect("trait impl dispatch should now compile directly");

    assert!(!object.is_empty());
}

#[test]
fn direct_backend_emits_object_for_extended_feature_examples() {
    let examples = [
        (
            "collections/list_polish",
            include_str!("../../../examples/collections/list_polish.au"),
        ),
        (
            "collections/dict_basics",
            include_str!("../../../examples/collections/dict_basics.au"),
        ),
        (
            "collections/set_basics",
            include_str!("../../../examples/collections/set_basics.au"),
        ),
        (
            "control_flow/match_literals",
            include_str!("../../../examples/control_flow/match_literals.au"),
        ),
        (
            "concurrency/task_group_start",
            include_str!("../../../examples/concurrency/task_group_start.au"),
        ),
        (
            "concurrency/queue_timeout",
            include_str!("../../../examples/concurrency/queue_timeout.au"),
        ),
        (
            "concurrency/queue_get_timeout_named",
            include_str!("../../../examples/concurrency/queue_get_timeout_named.au"),
        ),
        (
            "error_handling/try_result",
            include_str!("../../../examples/error_handling/try_result.au"),
        ),
        (
            "error_handling/try_from_trait",
            include_str!("../tests/fixtures/run-pass/try_from_trait.au"),
        ),
        (
            "numbers/numeric_builtins",
            include_str!("../../../examples/numbers/numeric_builtins.au"),
        ),
        (
            "strings/string_methods",
            include_str!("../../../examples/strings/string_methods.au"),
        ),
        (
            "strings/string_parsing_and_formatting",
            include_str!("../../../examples/strings/string_parsing_and_formatting.au"),
        ),
        (
            "traits/operator_traits",
            include_str!("../../../examples/traits/operator_traits.au"),
        ),
        (
            "traits/ordering_traits",
            include_str!("../../../examples/traits/ordering_traits.au"),
        ),
        (
            "basics/borrowed_lifetime_labels",
            include_str!("../../../examples/basics/copy_return_selection.au"),
        ),
        (
            "traits/generic_trait_bounds",
            include_str!("../../../examples/traits/generic_trait_bounds.au"),
        ),
        (
            "traits/specialized_trait_dispatch",
            include_str!("../../../examples/traits/specialized_trait_dispatch.au"),
        ),
    ];

    for (name, source) in examples {
        let mir = lower_source_to_mir(source).expect("example should lower to MIR");
        let object = emit_host_object(&mir).expect("example should emit direct object");
        assert!(!object.is_empty(), "{name}");
    }
}

#[test]
fn direct_backend_emits_object_for_every_runnable_fixture() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/run-pass");
    let mut paths = std::fs::read_dir(&fixtures)
        .expect("run-pass fixture directory should be readable")
        .map(|entry| {
            entry
                .expect("run-pass fixture entry should be readable")
                .path()
        })
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("au"))
        .collect::<Vec<_>>();
    paths.sort();

    for path in paths {
        let mir = lower_path_to_mir(&path)
            .unwrap_or_else(|error| panic!("{} should lower to MIR: {error}", path.display()));
        let object = emit_host_object(&mir).unwrap_or_else(|error| {
            panic!("{} should emit a direct object: {error}", path.display())
        });
        assert!(!object.is_empty(), "{}", path.display());
    }
}

#[test]
fn direct_backend_emits_object_for_runtime_member_surface_matrix() {
    let source = r#"
def worker(value: int32) -> int32:
    return value + 1

def main() -> int32:
    text = "  Aura Repo  "
    trimmed = text.trim()
    truth = (trimmed.contains("Repo") and true) or false
    print(truth)
    print(trimmed.len())
    print(trimmed.contains("Repo"))
    print(trimmed.starts_with("Aura"))
    print(trimmed.ends_with("Repo"))
    print(trimmed.replace("Repo", "Lang"))
    print(trimmed.to_lower())
    print(trimmed.to_upper())
    words = trimmed.split(" ")
    print("/".join(words))
    match trimmed.strip_prefix("Aura "):
        case str as rest:
            print(rest)
        case None:
            print("missing")
    match trimmed.strip_suffix(" Repo"):
        case str as rest:
            print(rest)
        case None:
            print("missing")

    mut numbers = [1, 2]
    print(numbers.len())
    print(numbers.is_empty())
    mut clone_numbers = numbers.copy()
    clone_numbers.append(3)
    print(clone_numbers.pop())
    print(clone_numbers.get(0))
    print(clone_numbers[1])
    print(clone_numbers.set(0, 9))
    clone_numbers[1] = 8
    print(clone_numbers.remove(0))
    print(clone_numbers.swap(0, 0))
    print(clone_numbers.contains(8))
    print(clone_numbers.insert(1, 7))
    clone_numbers.reverse()
    clone_numbers.extend([5, 6])
    clone_numbers.clear()
    print(clone_numbers.is_empty())

    mut counts = {"a": 1}
    print(counts.len())
    print(counts.is_empty())
    copy_counts = counts.copy()
    print(copy_counts.get("a"))
    print(copy_counts["a"])
    print(counts["a"])
    counts["a"] = 2
    counts["b"] = 3
    print(counts.remove("a"))
    print("b" in counts)
    print(counts.keys().len())
    print(counts.values().len())
    print(counts.items().len())
    counts.update({"c": 4})
    counts.clear()
    print(counts.is_empty())

    mut seen = {"x"}
    print(seen.len())
    print(seen.is_empty())
    copy_seen = seen.copy()
    print("x" in copy_seen)
    print(seen.add("y"))
    print(seen.remove("x"))
    print("y" in seen)

    jobs = Queue[int32]()
    jobs_copy = jobs
    print(jobs_copy.put(1))
    print(jobs.get())
    jobs.close()

    with TaskGroup() as group:
        task = group.start(worker, 4)
        task_copy = task
        print(task_copy.result())
        group.cancel()

    return 0
"#;

    let mir = lower_source_to_mir(source).expect("runtime member matrix should lower to MIR");
    let object = emit_host_object(&mir).expect("runtime member matrix should compile directly");

    assert!(!object.is_empty());
}

#[test]
fn direct_backend_emits_object_for_call_writeback_and_cleanup_surface() {
    let source = r#"
class Counter:
    value: int32

class Resource:
    closed: bool = false
    def close(mut self):
        self.closed = true

def bump(counter: mut Counter, amount: int32 = 2) -> int32:
    counter.value += amount
    return counter.value

def copy_into(source: Counter, target: mut Counter):
    target.value = source.value

def worker(value: int32) -> int32:
    return value + 1

def main() -> int32:
    mut first = Counter(value=1)
    mut second = Counter(value=0)
    print(bump(counter=first))
    copy_into(source=first, target=second)
    print(second.value)

    mut total: int64 = 0
    for i in range(stop=3):
        total += i
    print(total)

    print(abs(-3))
    print(min(8, 2))
    print(max(8, 2))
    print(parse_int32("12"))
    print(cancelled())

    jobs = Queue[int32]()
    print(jobs.put(7))
    match jobs.get(timeout=1ms):
        case QueueReceive.Item(value):
            print(value)
        case QueueReceive.TimedOut:
            print(99)
        case QueueReceive.Closed:
            print(98)
        case QueueReceive.Cancelled:
            print(97)
    jobs.close()

    sleep(0ms)

    with Resource() as resource:
        print(resource.closed)

    with TaskGroup() as group:
        task = group.start(worker, 4)
        print(task.result())
        group.cancel()

    return second.value
"#;

    let mir = lower_source_to_mir(source).expect("writeback/cleanup matrix should lower to MIR");
    let object = emit_host_object(&mir).expect("writeback/cleanup matrix should compile directly");
    assert!(!object.is_empty());
}

#[test]
fn direct_backend_emits_object_for_supported_cleanup_and_explicit_task_group_close() {
    let source = r#"
class Resource:
    closed: bool = false
    def close(mut self):
        self.closed = true

def main() -> int32:
    with Resource() as resource:
        print(resource.closed)

    with TaskGroup() as group:
        group.cancel()

    return 0
"#;

    let mir = lower_source_to_mir(source).expect("cleanup surface should lower to MIR");
    let object = emit_host_object(&mir).expect("cleanup surface should compile directly");

    assert!(!object.is_empty());
}

fn module_with_main_call(call: Rvalue) -> crate::mir::MirModule {
    module_with_main_call_result_type(call, Type::named("int32"))
}

fn module_with_main_call_result_type(call: Rvalue, result_ty: Type) -> crate::mir::MirModule {
    crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            source_path: None,
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: vec![MirLocalType {
                name: "%t0".to_string(),
                ty: result_ty,
            }],
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: vec![Instruction::Assign {
                    target: "%t0".to_string(),
                    value: call,
                }],
                terminator: Terminator::Return(Operand::Int(0)),
            }],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    }
}

#[test]
fn direct_ffi_codegen_embeds_call_metadata_and_uses_only_the_runtime_adapter() {
    let call = Rvalue::Call {
        callee: CallTarget::Extern(MirExternCall {
            symbol: "getpid".to_string(),
            abi: "C".to_string(),
            params: Vec::new(),
            return_type: Type::named("int32"),
        }),
        args: Vec::new(),
        type_args: Default::default(),
    };
    let module = module_with_main_call_result_type(call, Type::named("int32"));
    let object = emit_host_object(&module).expect("direct getpid FFI should emit");
    let referenced = object_referenced_symbols(&object);
    assert!(
        referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_ffi_call")),
        "direct FFI must route through the shared runtime adapter: {referenced:?}"
    );
    assert!(
        referenced.iter().all(|symbol| !symbol.contains("getpid")),
        "generated objects must resolve process symbols at runtime: {referenced:?}"
    );
    assert!(
        object.windows(4).any(|bytes| bytes == b"AUFI")
            && object.windows(6).any(|bytes| bytes == b"getpid"),
        "the validated serialized call spec must be embedded in the object"
    );
}

#[test]
fn direct_ffi_codegen_handles_narrow_scalars_mutable_bytes_and_opaque_ownership() {
    let handle_ty = Type::named("ProcessHandle");
    let bytes_ty = Type::Named("list".to_string(), vec![Type::named("uint8")]);
    let module = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            source_path: None,
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: vec![
                MirLocalType {
                    name: "bytes".to_string(),
                    ty: bytes_ty.clone(),
                },
                MirLocalType {
                    name: "read_count".to_string(),
                    ty: Type::named("int64"),
                },
                MirLocalType {
                    name: "handle".to_string(),
                    ty: handle_ty.clone(),
                },
                MirLocalType {
                    name: "released".to_string(),
                    ty: Type::Unit,
                },
                MirLocalType {
                    name: "narrow".to_string(),
                    ty: Type::named("uint16"),
                },
            ],
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: vec![
                    Instruction::Assign {
                        target: "bytes".to_string(),
                        value: Rvalue::VecLiteral {
                            elements: vec![Operand::Int(0), Operand::Int(0)],
                            element_type: Type::named("uint8"),
                        },
                    },
                    Instruction::Assign {
                        target: "read_count".to_string(),
                        value: Rvalue::Call {
                            callee: CallTarget::Extern(MirExternCall {
                                symbol: "read".to_string(),
                                abi: "C".to_string(),
                                params: vec![
                                    MirExternParam {
                                        name: "fd".to_string(),
                                        passing: MirReceiverKind::Borrow,
                                        ty: Type::named("int32"),
                                    },
                                    MirExternParam {
                                        name: "bytes".to_string(),
                                        passing: MirReceiverKind::BorrowMut,
                                        ty: bytes_ty.clone(),
                                    },
                                ],
                                return_type: Type::named("int64"),
                            }),
                            args: vec![
                                MirArg {
                                    name: None,
                                    value: Operand::Int(0),
                                    writeback_place: None,
                                },
                                MirArg {
                                    name: None,
                                    value: Operand::MovePlace("bytes".to_string()),
                                    writeback_place: Some("bytes".to_string()),
                                },
                            ],
                            type_args: Default::default(),
                        },
                    },
                    Instruction::Assign {
                        target: "handle".to_string(),
                        value: Rvalue::Call {
                            callee: CallTarget::Extern(MirExternCall {
                                symbol: "malloc".to_string(),
                                abi: "C".to_string(),
                                params: vec![MirExternParam {
                                    name: "size".to_string(),
                                    passing: MirReceiverKind::Borrow,
                                    ty: Type::named("uint64"),
                                }],
                                return_type: handle_ty.clone(),
                            }),
                            args: vec![MirArg {
                                name: None,
                                value: Operand::Int(1),
                                writeback_place: None,
                            }],
                            type_args: Default::default(),
                        },
                    },
                    Instruction::Assign {
                        target: "released".to_string(),
                        value: Rvalue::Call {
                            callee: CallTarget::Extern(MirExternCall {
                                symbol: "free".to_string(),
                                abi: "C".to_string(),
                                params: vec![MirExternParam {
                                    name: "handle".to_string(),
                                    passing: MirReceiverKind::Value,
                                    ty: handle_ty,
                                }],
                                return_type: Type::Unit,
                            }),
                            args: vec![MirArg {
                                name: None,
                                value: Operand::MovePlace("handle".to_string()),
                                writeback_place: None,
                            }],
                            type_args: Default::default(),
                        },
                    },
                    Instruction::Assign {
                        target: "narrow".to_string(),
                        value: Rvalue::Call {
                            callee: CallTarget::Extern(MirExternCall {
                                symbol: "htons".to_string(),
                                abi: "C".to_string(),
                                params: vec![MirExternParam {
                                    name: "value".to_string(),
                                    passing: MirReceiverKind::Borrow,
                                    ty: Type::named("uint16"),
                                }],
                                return_type: Type::named("uint16"),
                            }),
                            args: vec![MirArg {
                                name: None,
                                value: Operand::Int(7),
                                writeback_place: None,
                            }],
                            type_args: Default::default(),
                        },
                    },
                ],
                terminator: Terminator::Return(Operand::Int(0)),
            }],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };
    let object = emit_host_object(&module)
        .expect("mutable byte views, opaque ownership, and narrow scalars should emit");
    assert!(
        object_referenced_symbol_occurrences(&object, "aura_direct_ffi_call") >= 4,
        "each extern call must reference the shared runtime adapter"
    );
}

#[test]
fn direct_ffi_validation_rejects_unvalidated_metadata() {
    let invalid_abi = module_with_main_call_result_type(
        Rvalue::Call {
            callee: CallTarget::Extern(MirExternCall {
                symbol: "foreign".to_string(),
                abi: "Rust".to_string(),
                params: Vec::new(),
                return_type: Type::named("int32"),
            }),
            args: Vec::new(),
            type_args: Default::default(),
        },
        Type::named("int32"),
    );
    let error = emit_host_object(&invalid_abi).expect_err("non-C FFI must never reach codegen");
    assert!(error.contains("unsupported FFI ABI `Rust`"));

    let wrong_arity = module_with_main_call_result_type(
        Rvalue::Call {
            callee: CallTarget::Extern(MirExternCall {
                symbol: "foreign".to_string(),
                abi: "C".to_string(),
                params: vec![MirExternParam {
                    name: "value".to_string(),
                    passing: MirReceiverKind::Borrow,
                    ty: Type::named("int32"),
                }],
                return_type: Type::named("int32"),
            }),
            args: Vec::new(),
            type_args: Default::default(),
        },
        Type::named("int32"),
    );
    assert_eq!(
        emit_host_object(&wrong_arity).expect_err("extern arity drift must be rejected"),
        "direct backend expected 1 argument(s) for extern `foreign`, found 0"
    );

    let unexpected_writeback = module_with_main_call_result_type(
        Rvalue::Call {
            callee: CallTarget::Extern(MirExternCall {
                symbol: "foreign".to_string(),
                abi: "C".to_string(),
                params: vec![MirExternParam {
                    name: "value".to_string(),
                    passing: MirReceiverKind::Borrow,
                    ty: Type::named("int32"),
                }],
                return_type: Type::named("int32"),
            }),
            args: vec![MirArg {
                name: None,
                value: Operand::Int(1),
                writeback_place: Some("value".to_string()),
            }],
            type_args: Default::default(),
        },
        Type::named("int32"),
    );
    assert_eq!(
        emit_host_object(&unexpected_writeback)
            .expect_err("shared extern arguments must not request writeback"),
        "direct backend extern argument 1 unexpectedly requests writeback"
    );

    let bytes_ty = Type::Named("list".to_string(), vec![Type::named("uint8")]);
    let missing_mut_writeback = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            source_path: None,
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: vec![
                MirLocalType {
                    name: "bytes".to_string(),
                    ty: bytes_ty.clone(),
                },
                MirLocalType {
                    name: "%t0".to_string(),
                    ty: Type::named("int64"),
                },
            ],
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: vec![
                    Instruction::Assign {
                        target: "bytes".to_string(),
                        value: Rvalue::VecLiteral {
                            elements: vec![Operand::Int(0)],
                            element_type: Type::named("uint8"),
                        },
                    },
                    Instruction::Assign {
                        target: "%t0".to_string(),
                        value: Rvalue::Call {
                            callee: CallTarget::Extern(MirExternCall {
                                symbol: "read".to_string(),
                                abi: "C".to_string(),
                                params: vec![MirExternParam {
                                    name: "bytes".to_string(),
                                    passing: MirReceiverKind::BorrowMut,
                                    ty: bytes_ty,
                                }],
                                return_type: Type::named("int64"),
                            }),
                            args: vec![MirArg {
                                name: None,
                                value: Operand::MovePlace("bytes".to_string()),
                                writeback_place: None,
                            }],
                            type_args: Default::default(),
                        },
                    },
                ],
                terminator: Terminator::Return(Operand::Int(0)),
            }],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };
    assert_eq!(
        emit_host_object(&missing_mut_writeback)
            .expect_err("mutable byte views require an explicit writeback place"),
        "direct backend extern mutable argument 1 has no writeback place"
    );
}

#[test]
fn direct_ffi_source_types_pin_the_v0_abi_and_capability_contract() {
    use crate::ffi::FfiType;

    for (name, expected) in [
        ("bool", FfiType::Bool),
        ("int8", FfiType::I8),
        ("int16", FfiType::I16),
        ("int32", FfiType::I32),
        ("int", FfiType::I64),
        ("int64", FfiType::I64),
        ("uint8", FfiType::U8),
        ("uint16", FfiType::U16),
        ("uint32", FfiType::U32),
        ("uint64", FfiType::U64),
        ("float32", FfiType::F32),
        ("float64", FfiType::F64),
    ] {
        let direct = direct_ffi_type_for_source(&Type::named(name), Some(MirReceiverKind::Borrow))
            .unwrap_or_else(|error| panic!("{name} should be a valid shared FFI scalar: {error}"));
        assert_eq!(direct.ffi_type, expected, "{name} ABI kind");
        assert_eq!(direct.opaque_name, None, "{name} is not nominal");
    }

    assert_eq!(
        direct_ffi_type_for_source(&Type::Unit, None)
            .expect("None is the FFI unit return")
            .ffi_type,
        FfiType::Unit
    );
    let string = direct_ffi_type_for_source(&Type::named("str"), Some(MirReceiverKind::Borrow))
        .expect("shared str is a UTF-8 view");
    assert_eq!(string.ffi_type, FfiType::StringView);
    let bytes_ty = Type::Named("list".to_string(), vec![Type::named("uint8")]);
    assert_eq!(
        direct_ffi_type_for_source(&bytes_ty, Some(MirReceiverKind::Borrow))
            .expect("shared bytes are a const view")
            .ffi_type,
        FfiType::BytesView
    );
    assert_eq!(
        direct_ffi_type_for_source(&bytes_ty, Some(MirReceiverKind::BorrowMut))
            .expect("mutable bytes are a scratch view")
            .ffi_type,
        FfiType::BytesViewMut
    );
    let handle =
        direct_ffi_type_for_source(&Type::named("ProcessHandle"), Some(MirReceiverKind::Value))
            .expect("owned opaque handles are consumed");
    assert_eq!(handle.ffi_type, FfiType::OpaqueHandle);
    assert_eq!(handle.opaque_name.as_deref(), Some("ProcessHandle"));

    for passing in [MirReceiverKind::BorrowMut, MirReceiverKind::Value] {
        assert_eq!(
            direct_ffi_type_for_source(&Type::named("int32"), Some(passing)),
            Err(format!(
                "direct backend cannot pass `int32` with ownership mode `{passing:?}` through FFI v0"
            ))
        );
    }
    assert_eq!(
        direct_ffi_type_for_source(&Type::named("str"), Some(MirReceiverKind::BorrowMut)),
        Err("direct backend can pass `str` through FFI v0 only as a shared view".to_string())
    );
    assert_eq!(
        direct_ffi_type_for_source(
            &Type::named("ProcessHandle"),
            Some(MirReceiverKind::BorrowMut)
        ),
        Err(
            "direct backend cannot mutably borrow opaque handle `ProcessHandle` through FFI v0"
                .to_string()
        )
    );
    assert_eq!(
        direct_ffi_type_for_source(&bytes_ty, Some(MirReceiverKind::Value)),
        Err("direct backend cannot pass `own list[uint8]` through FFI v0".to_string())
    );
    assert_eq!(
        direct_ffi_type_for_source(&bytes_ty, None),
        Err("direct backend cannot return `list[uint8]` through FFI v0".to_string())
    );
    assert_eq!(
        direct_ffi_type_for_source(
            &Type::Named("list".to_string(), vec![Type::named("int32")]),
            Some(MirReceiverKind::Borrow),
        ),
        Err("direct backend cannot lower `list[int32]` through FFI v0".to_string())
    );
    assert_eq!(
        direct_ffi_type_for_source(
            &Type::Tuple(vec![Type::named("int32")]),
            Some(MirReceiverKind::Borrow),
        ),
        Err("direct backend cannot lower `(int32,)` through FFI v0".to_string())
    );
}

fn module_with_main_member_call(
    object_name: &str,
    object_ty: Type,
    object_value: Rvalue,
    field: &str,
    args: Vec<MirArg>,
) -> crate::mir::MirModule {
    module_with_main_member_call_result_type(
        object_name,
        object_ty,
        object_value,
        Type::named("int32"),
        field,
        args,
    )
}

fn module_with_main_member_call_result_type(
    object_name: &str,
    object_ty: Type,
    object_value: Rvalue,
    result_ty: Type,
    field: &str,
    args: Vec<MirArg>,
) -> crate::mir::MirModule {
    crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            source_path: None,
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: vec![
                MirLocalType {
                    name: object_name.to_string(),
                    ty: object_ty,
                },
                MirLocalType {
                    name: "%t0".to_string(),
                    ty: result_ty,
                },
            ],
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: vec![
                    Instruction::Assign {
                        target: object_name.to_string(),
                        value: object_value,
                    },
                    Instruction::Assign {
                        target: "%t0".to_string(),
                        value: Rvalue::Call {
                            callee: CallTarget::Member {
                                object: Operand::Place(object_name.to_string()),
                                field: field.to_string(),
                                receiver_place: Some(object_name.to_string()),
                                dispatch: None,
                            },
                            args,
                            type_args: Default::default(),
                        },
                    },
                ],
                terminator: Terminator::Return(Operand::Int(0)),
            }],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    }
}

#[test]
fn direct_backend_internal_collection_member_surface_compiles() {
    let string_byte_len = module_with_main_member_call_result_type(
        "text",
        Type::named("str"),
        Rvalue::Use(Operand::String("é🎉e\u{301}".to_string())),
        Type::named("int64"),
        "byte_len",
        Vec::new(),
    );
    assert!(!emit_host_object(&string_byte_len)
        .expect("str.byte_len() should compile directly")
        .is_empty());

    let slice_args = || {
        vec![
            MirArg {
                name: None,
                value: Operand::Int(1),
                writeback_place: None,
            },
            MirArg {
                name: None,
                value: Operand::Bool(true),
                writeback_place: None,
            },
            MirArg {
                name: None,
                value: Operand::Int(3),
                writeback_place: None,
            },
            MirArg {
                name: None,
                value: Operand::Bool(true),
                writeback_place: None,
            },
            MirArg {
                name: None,
                value: Operand::Int(1),
                writeback_place: None,
            },
            MirArg {
                name: None,
                value: Operand::Int(5),
                writeback_place: None,
            },
        ]
    };
    let string_slice = module_with_main_member_call_result_type(
        "text",
        Type::named("str"),
        Rvalue::Use(Operand::String("Aé🙂Z".to_string())),
        Type::named("str"),
        "__slice",
        slice_args(),
    );
    assert!(!emit_host_object(&string_slice)
        .expect("internal owned str slicing should compile directly")
        .is_empty());

    let vec_slice = module_with_main_member_call_result_type(
        "values",
        Type::Named("list".to_string(), vec![Type::named("int32")]),
        Rvalue::VecLiteral {
            element_type: Type::named("int32"),
            elements: vec![
                Operand::Int(1),
                Operand::Int(2),
                Operand::Int(3),
                Operand::Int(4),
            ],
        },
        Type::Named("list".to_string(), vec![Type::named("int32")]),
        "__slice",
        slice_args(),
    );
    assert!(!emit_host_object(&vec_slice)
        .expect("internal owned Vec slicing should compile directly")
        .is_empty());

    let vec_index_option = module_with_main_member_call_result_type(
        "values",
        Type::Named("list".to_string(), vec![Type::named("int32")]),
        Rvalue::VecLiteral {
            element_type: Type::named("int32"),
            elements: vec![Operand::Int(1), Operand::Int(2)],
        },
        crate::sema::lookup_type(Type::named("int32")),
        "__index_option",
        vec![MirArg {
            name: None,
            value: Operand::Int(1),
            writeback_place: None,
        }],
    );
    assert!(!emit_host_object(&vec_index_option)
        .expect("internal vec optional indexing should compile directly")
        .is_empty());

    let vec_set_index = module_with_main_member_call_result_type(
        "values",
        Type::Named("list".to_string(), vec![Type::named("int32")]),
        Rvalue::VecLiteral {
            element_type: Type::named("int32"),
            elements: vec![Operand::Int(1), Operand::Int(2)],
        },
        Type::Unit,
        "__set_index",
        vec![
            MirArg {
                name: None,
                value: Operand::Int(0),
                writeback_place: None,
            },
            MirArg {
                name: None,
                value: Operand::Int(9),
                writeback_place: None,
            },
            MirArg {
                name: None,
                value: Operand::Int(1),
                writeback_place: None,
            },
            MirArg {
                name: None,
                value: Operand::Int(1),
                writeback_place: None,
            },
        ],
    );
    assert!(!emit_host_object(&vec_set_index)
        .expect("internal vec indexed assignment should compile directly")
        .is_empty());

    let map_index = module_with_main_member_call_result_type(
        "counts",
        Type::Named(
            "dict".to_string(),
            vec![Type::named("str"), Type::named("int32")],
        ),
        Rvalue::MapLiteral {
            key_type: Type::named("str"),
            value_type: Type::named("int32"),
            entries: vec![MirMapEntry {
                key: Operand::String("a".to_string()),
                value: Operand::Int(1),
            }],
        },
        Type::named("int32"),
        "__index",
        vec![
            MirArg {
                name: None,
                value: Operand::String("a".to_string()),
                writeback_place: None,
            },
            MirArg {
                name: None,
                value: Operand::Int(1),
                writeback_place: None,
            },
            MirArg {
                name: None,
                value: Operand::Int(1),
                writeback_place: None,
            },
        ],
    );
    assert!(!emit_host_object(&map_index)
        .expect("internal map indexing should compile directly")
        .is_empty());

    let map_set_index = module_with_main_member_call_result_type(
        "counts",
        Type::Named(
            "dict".to_string(),
            vec![Type::named("str"), Type::named("int32")],
        ),
        Rvalue::MapLiteral {
            key_type: Type::named("str"),
            value_type: Type::named("int32"),
            entries: vec![MirMapEntry {
                key: Operand::String("a".to_string()),
                value: Operand::Int(1),
            }],
        },
        Type::Unit,
        "__set_index",
        vec![
            MirArg {
                name: None,
                value: Operand::String("b".to_string()),
                writeback_place: None,
            },
            MirArg {
                name: None,
                value: Operand::Int(2),
                writeback_place: None,
            },
            MirArg {
                name: None,
                value: Operand::Int(1),
                writeback_place: None,
            },
            MirArg {
                name: None,
                value: Operand::Int(1),
                writeback_place: None,
            },
        ],
    );
    assert!(!emit_host_object(&map_set_index)
        .expect("internal map indexed assignment should compile directly")
        .is_empty());

    let set_index_option = module_with_main_member_call_result_type(
        "seen",
        Type::Named("set".to_string(), vec![Type::named("str")]),
        Rvalue::SetLiteral {
            element_type: Type::named("str"),
            elements: vec![Operand::String("x".to_string())],
        },
        crate::sema::lookup_type(Type::named("str")),
        "__index_option",
        vec![MirArg {
            name: None,
            value: Operand::Int(0),
            writeback_place: None,
        }],
    );
    assert!(!emit_host_object(&set_index_option)
        .expect("internal set optional indexing should compile directly")
        .is_empty());
}

#[test]
fn direct_backend_scalar_bool_range_and_coercion_paths_compile() {
    let module = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            source_path: None,
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: vec![
                MirLocalType {
                    name: "test_left".to_string(),
                    ty: Type::named("bool"),
                },
                MirLocalType {
                    name: "test_right".to_string(),
                    ty: Type::named("bool"),
                },
                MirLocalType {
                    name: "test_and".to_string(),
                    ty: Type::named("bool"),
                },
                MirLocalType {
                    name: "test_or".to_string(),
                    ty: Type::named("bool"),
                },
                MirLocalType {
                    name: "test_range".to_string(),
                    ty: Type::named("Range"),
                },
                MirLocalType {
                    name: "test_int_as_bool".to_string(),
                    ty: Type::named("bool"),
                },
                MirLocalType {
                    name: "test_int32_value".to_string(),
                    ty: Type::named("int32"),
                },
                MirLocalType {
                    name: "test_unit_as_int".to_string(),
                    ty: Type::named("int32"),
                },
            ],
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: vec![
                    Instruction::Assign {
                        target: "test_left".to_string(),
                        value: Rvalue::Use(Operand::Bool(true)),
                    },
                    Instruction::Assign {
                        target: "test_right".to_string(),
                        value: Rvalue::Use(Operand::Bool(false)),
                    },
                    Instruction::Assign {
                        target: "test_and".to_string(),
                        value: Rvalue::Binary {
                            op: BinaryOp::And,
                            left: Operand::Place("test_left".to_string()),
                            right: Operand::Place("test_right".to_string()),
                            span: Span::new(1, 1),
                            trap: crate::mir::ArithmeticTrap::None,
                        },
                    },
                    Instruction::Assign {
                        target: "test_or".to_string(),
                        value: Rvalue::Binary {
                            op: BinaryOp::Or,
                            left: Operand::Place("test_left".to_string()),
                            right: Operand::Place("test_right".to_string()),
                            span: Span::new(1, 1),
                            trap: crate::mir::ArithmeticTrap::None,
                        },
                    },
                    Instruction::Assign {
                        target: "test_range".to_string(),
                        value: Rvalue::Call {
                            callee: CallTarget::Name("range".to_string()),
                            args: vec![
                                MirArg {
                                    name: None,
                                    value: Operand::Int(0),
                                    writeback_place: None,
                                },
                                MirArg {
                                    name: Some("stop".to_string()),
                                    value: Operand::Int(4),
                                    writeback_place: None,
                                },
                            ],
                            type_args: Default::default(),
                        },
                    },
                    Instruction::Assign {
                        target: "test_int32_value".to_string(),
                        value: Rvalue::Use(Operand::Int(1)),
                    },
                    Instruction::Assign {
                        target: "test_int_as_bool".to_string(),
                        value: Rvalue::Use(Operand::Place("test_int32_value".to_string())),
                    },
                    Instruction::Assign {
                        target: "test_unit_as_int".to_string(),
                        value: Rvalue::Use(Operand::Unit),
                    },
                ],
                terminator: Terminator::Return(Operand::Int(0)),
            }],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };

    let object = emit_host_object(&module)
        .expect("manual scalar bool/range/coercion MIR should emit direct object");
    assert!(!object.is_empty());
}

#[test]
fn direct_backend_internal_collection_member_errors_are_reported() {
    let cases = [
        (
            module_with_main_member_call_result_type(
                "values",
                Type::Named("list".to_string(), vec![Type::named("int32")]),
                Rvalue::VecLiteral {
                    element_type: Type::named("int32"),
                    elements: vec![Operand::Int(1)],
                },
                crate::sema::lookup_type(Type::named("int32")),
                "__index_option",
                Vec::new(),
            ),
            "expected `__index_option()` to receive `index`",
        ),
        (
            module_with_main_member_call_result_type(
                "values",
                Type::Named("list".to_string(), vec![Type::named("int32")]),
                Rvalue::VecLiteral {
                    element_type: Type::named("int32"),
                    elements: vec![Operand::Int(1)],
                },
                Type::named("int32"),
                "__index",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(0),
                    writeback_place: None,
                }],
            ),
            "expected `__index()` to receive `line`",
        ),
        (
            module_with_main_member_call_result_type(
                "counts",
                Type::Named(
                    "dict".to_string(),
                    vec![Type::named("str"), Type::named("int32")],
                ),
                Rvalue::MapLiteral {
                    key_type: Type::named("str"),
                    value_type: Type::named("int32"),
                    entries: vec![MirMapEntry {
                        key: Operand::String("a".to_string()),
                        value: Operand::Int(1),
                    }],
                },
                Type::Unit,
                "__set_index",
                vec![MirArg {
                    name: None,
                    value: Operand::String("a".to_string()),
                    writeback_place: None,
                }],
            ),
            "expected `__set_index()` to receive `value`",
        ),
        (
            module_with_main_member_call_result_type(
                "seen",
                Type::Named("set".to_string(), vec![Type::named("str")]),
                Rvalue::SetLiteral {
                    element_type: Type::named("str"),
                    elements: vec![Operand::String("x".to_string())],
                },
                crate::sema::lookup_type(Type::named("str")),
                "__index_option",
                Vec::new(),
            ),
            "expected `__index_option()` to receive `index`",
        ),
    ];

    for (module, expected) in cases {
        let error = emit_host_object(&module).expect_err("invalid internal collection member call");
        assert!(
            error.contains(expected),
            "expected `{expected}` in `{error}`"
        );
    }
}

#[test]
fn malformed_uncalled_array_slice_reports_a_direct_diagnostic_without_panicking() {
    let mut module = module_with_main_member_call_result_type(
        "values",
        Type::Named("Array".to_string(), vec![Type::named("int32")]),
        Rvalue::Use(Operand::String("opaque-array".to_string())),
        Type::Named("Array".to_string(), vec![Type::named("int32")]),
        "__slice",
        Vec::new(),
    );
    module.functions[0].name = "unused_slice".to_string();
    module.functions.push(MirFunction {
        name: "main".to_string(),
        module_name: "<test>".to_string(),
        source_path: None,
        span: Span::new(1, 1),
        receiver: None,
        params: Vec::new(),
        local_types: Vec::new(),
        return_type: Type::named("int32"),
        entry: "entry".to_string(),
        blocks: vec![BasicBlock {
            label: "entry".to_string(),
            instructions: Vec::new(),
            terminator: Terminator::Return(Operand::Int(0)),
        }],
    });

    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| emit_host_object(&module)));
    let error = result
        .expect("malformed public Array slicing MIR must never panic")
        .expect_err("malformed public Array slicing MIR must be rejected before codegen");
    assert!(
        error.contains("`Array.Slice` in `unused_slice`: expected `__slice()` to receive `start`"),
        "unexpected Array slicing diagnostic: {error}"
    );
}

#[test]
fn direct_backend_runtime_member_matrix_covers_remaining_string_collection_and_runtime_paths() {
    let string_ty = Type::named("str");
    let string_value = Rvalue::Use(Operand::String("Aura".to_string()));
    let vec_ty = Type::Named("list".to_string(), vec![Type::named("int32")]);
    let vec_value = Rvalue::VecLiteral {
        element_type: Type::named("int32"),
        elements: vec![Operand::Int(1), Operand::Int(2)],
    };
    let map_ty = Type::Named(
        "dict".to_string(),
        vec![Type::named("str"), Type::named("int32")],
    );
    let map_value = Rvalue::MapLiteral {
        key_type: Type::named("str"),
        value_type: Type::named("int32"),
        entries: vec![MirMapEntry {
            key: Operand::String("count".to_string()),
            value: Operand::Int(1),
        }],
    };
    let set_ty = Type::Named("set".to_string(), vec![Type::named("str")]);
    let set_value = Rvalue::SetLiteral {
        element_type: Type::named("str"),
        elements: vec![Operand::String("ready".to_string())],
    };
    let channel_ty = Type::Named("Queue".to_string(), vec![Type::named("int32")]);
    let channel_value = Rvalue::Call {
        callee: CallTarget::Name("Queue".to_string()),
        args: Vec::new(),
        type_args: Default::default(),
    };
    let task_group_ty = Type::named("TaskGroup");
    let task_group_value = Rvalue::Call {
        callee: CallTarget::Name("TaskGroup".to_string()),
        args: Vec::new(),
        type_args: Default::default(),
    };
    let cases = vec![
        (
            "str.len",
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("int64"),
                "len",
                Vec::new(),
            ),
        ),
        (
            "str.byte_len",
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("int64"),
                "byte_len",
                Vec::new(),
            ),
        ),
        (
            "str.contains",
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("bool"),
                "contains",
                vec![MirArg {
                    name: None,
                    value: Operand::String("ror".to_string()),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "str.starts_with",
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("bool"),
                "starts_with",
                vec![MirArg {
                    name: None,
                    value: Operand::String("Aur".to_string()),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "str.ends_with",
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("bool"),
                "ends_with",
                vec![MirArg {
                    name: None,
                    value: Operand::String("ora".to_string()),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "str.split",
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::Named("list".to_string(), vec![Type::named("str")]),
                "split",
                vec![MirArg {
                    name: None,
                    value: Operand::String("r".to_string()),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "str.replace",
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("str"),
                "replace",
                vec![
                    MirArg {
                        name: None,
                        value: Operand::String("Aur".to_string()),
                        writeback_place: None,
                    },
                    MirArg {
                        name: None,
                        value: Operand::String("Our".to_string()),
                        writeback_place: None,
                    },
                ],
            ),
        ),
        (
            "str.to_lower",
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("str"),
                "to_lower",
                Vec::new(),
            ),
        ),
        (
            "str.to_upper",
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("str"),
                "to_upper",
                Vec::new(),
            ),
        ),
        (
            "str.strip_prefix",
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                crate::sema::optional_type(Type::named("str")),
                "strip_prefix",
                vec![MirArg {
                    name: None,
                    value: Operand::String("Au".to_string()),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "str.strip_suffix",
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                crate::sema::optional_type(Type::named("str")),
                "strip_suffix",
                vec![MirArg {
                    name: None,
                    value: Operand::String("ra".to_string()),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "str.trim",
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                Rvalue::Use(Operand::String("  Aura  ".to_string())),
                Type::named("str"),
                "trim",
                Vec::new(),
            ),
        ),
        (
            "list.len",
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::named("int64"),
                "len",
                Vec::new(),
            ),
        ),
        (
            "list.is_empty",
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::named("bool"),
                "is_empty",
                Vec::new(),
            ),
        ),
        (
            "list.append",
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::Unit,
                "append",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(3),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "list.pop",
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::named("int32"),
                "pop",
                Vec::new(),
            ),
        ),
        (
            "list.get",
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                crate::sema::lookup_type(Type::named("int32")),
                "get",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(0),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "list.set",
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::named("int32"),
                "set",
                vec![
                    MirArg {
                        name: None,
                        value: Operand::Int(0),
                        writeback_place: None,
                    },
                    MirArg {
                        name: None,
                        value: Operand::Int(9),
                        writeback_place: None,
                    },
                ],
            ),
        ),
        (
            "list.remove",
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::Unit,
                "remove",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "list.swap",
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::Unit,
                "swap",
                vec![
                    MirArg {
                        name: None,
                        value: Operand::Int(0),
                        writeback_place: None,
                    },
                    MirArg {
                        name: None,
                        value: Operand::Int(1),
                        writeback_place: None,
                    },
                ],
            ),
        ),
        (
            "list.contains",
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::named("bool"),
                "contains",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(2),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "list.insert",
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::Unit,
                "insert",
                vec![
                    MirArg {
                        name: None,
                        value: Operand::Int(1),
                        writeback_place: None,
                    },
                    MirArg {
                        name: None,
                        value: Operand::Int(5),
                        writeback_place: None,
                    },
                ],
            ),
        ),
        (
            "list.clear",
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::Unit,
                "clear",
                Vec::new(),
            ),
        ),
        (
            "list.reverse",
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::Unit,
                "reverse",
                Vec::new(),
            ),
        ),
        (
            "dict.len",
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::named("int64"),
                "len",
                Vec::new(),
            ),
        ),
        (
            "dict.is_empty",
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::named("bool"),
                "is_empty",
                Vec::new(),
            ),
        ),
        (
            "dict.get",
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                crate::sema::lookup_type(Type::named("int32")),
                "get",
                vec![MirArg {
                    name: None,
                    value: Operand::String("count".to_string()),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "dict.set",
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                crate::sema::lookup_type(Type::named("int32")),
                "set",
                vec![
                    MirArg {
                        name: None,
                        value: Operand::String("count".to_string()),
                        writeback_place: None,
                    },
                    MirArg {
                        name: None,
                        value: Operand::Int(2),
                        writeback_place: None,
                    },
                ],
            ),
        ),
        (
            "dict.remove",
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                crate::sema::lookup_type(Type::named("int32")),
                "remove",
                vec![MirArg {
                    name: None,
                    value: Operand::String("count".to_string()),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "dict membership",
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::named("bool"),
                "contains_key",
                vec![MirArg {
                    name: None,
                    value: Operand::String("count".to_string()),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "dict.keys",
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::Named("list".to_string(), vec![Type::named("str")]),
                "keys",
                Vec::new(),
            ),
        ),
        (
            "dict.values",
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::Named("list".to_string(), vec![Type::named("int32")]),
                "values",
                Vec::new(),
            ),
        ),
        (
            "dict.items",
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::Named(
                    "list".to_string(),
                    vec![Type::Tuple(vec![Type::named("str"), Type::named("int32")])],
                ),
                "items",
                Vec::new(),
            ),
        ),
        (
            "dict.clear",
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::Unit,
                "clear",
                Vec::new(),
            ),
        ),
        (
            "set.len",
            module_with_main_member_call_result_type(
                "seen",
                set_ty.clone(),
                set_value.clone(),
                Type::named("int64"),
                "len",
                Vec::new(),
            ),
        ),
        (
            "set.is_empty",
            module_with_main_member_call_result_type(
                "seen",
                set_ty.clone(),
                set_value.clone(),
                Type::named("bool"),
                "is_empty",
                Vec::new(),
            ),
        ),
        (
            "set.contains",
            module_with_main_member_call_result_type(
                "seen",
                set_ty.clone(),
                set_value.clone(),
                Type::named("bool"),
                "contains",
                vec![MirArg {
                    name: None,
                    value: Operand::String("ready".to_string()),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "set.add",
            module_with_main_member_call_result_type(
                "seen",
                set_ty.clone(),
                set_value.clone(),
                Type::Unit,
                "add",
                vec![MirArg {
                    name: None,
                    value: Operand::String("go".to_string()),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "set.remove",
            module_with_main_member_call_result_type(
                "seen",
                set_ty.clone(),
                set_value.clone(),
                Type::Unit,
                "remove",
                vec![MirArg {
                    name: None,
                    value: Operand::String("ready".to_string()),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "Queue.put",
            module_with_main_member_call_result_type(
                "jobs",
                channel_ty.clone(),
                channel_value.clone(),
                Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Unit,
                        Type::Named("SendError".to_string(), vec![Type::named("int32")]),
                    ],
                ),
                "put",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "Queue.get",
            module_with_main_member_call_result_type(
                "jobs",
                channel_ty.clone(),
                channel_value.clone(),
                Type::Named("QueueReceive".to_string(), vec![Type::named("int32")]),
                "get",
                Vec::new(),
            ),
        ),
        (
            "Queue.close",
            module_with_main_member_call_result_type(
                "jobs",
                channel_ty.clone(),
                channel_value,
                Type::Unit,
                "close",
                Vec::new(),
            ),
        ),
        (
            "TaskGroup.cancel",
            module_with_main_member_call_result_type(
                "group",
                task_group_ty,
                task_group_value,
                Type::Unit,
                "cancel",
                Vec::new(),
            ),
        ),
    ];

    for (name, module) in cases {
        let object = emit_host_object(&module).expect("runtime member surface should compile");
        assert!(!object.is_empty(), "{name}");
    }
}

#[test]
fn direct_backend_resource_member_argument_errors_cover_network_and_process_paths() {
    let arg = |name: Option<&str>, value: Operand| MirArg {
        name: name.map(str::to_string),
        value,
        writeback_place: None,
    };
    let opaque_value = || Rvalue::Use(Operand::String("opaque-resource".to_string()));
    let named = |name: &str| Type::Named(name.to_string(), Vec::new());

    let cases = vec![
        (
            named("fs.File"),
            "read_all",
            vec![arg(None, Operand::Int(1))],
            "expected `read_all()` to take no arguments",
        ),
        (
            named("fs.File"),
            "read_bytes",
            vec![arg(None, Operand::Int(1))],
            "expected `read_bytes()` to take no arguments",
        ),
        (
            named("fs.File"),
            "write_all",
            Vec::new(),
            "expected `write_all()` to receive `text`",
        ),
        (
            named("fs.File"),
            "write_bytes",
            Vec::new(),
            "expected `write_bytes()` to receive `bytes`",
        ),
        (
            named("fs.File"),
            "flush",
            vec![arg(None, Operand::Int(1))],
            "expected `flush()` to take no arguments",
        ),
        (
            named("fs.File"),
            "close",
            vec![arg(None, Operand::Int(1))],
            "expected `close()` to take no arguments",
        ),
        (
            named("fs.File"),
            "unknown",
            Vec::new(),
            "runtime has no member `fs.File.unknown`",
        ),
        (
            named("process.Child"),
            "stdin",
            vec![arg(None, Operand::Int(1))],
            "expected `stdin()` to take no arguments",
        ),
        (
            named("process.Child"),
            "stdout",
            vec![arg(None, Operand::Int(1))],
            "expected `stdout()` to take no arguments",
        ),
        (
            named("process.Child"),
            "stderr",
            vec![arg(None, Operand::Int(1))],
            "expected `stderr()` to take no arguments",
        ),
        (
            named("process.Child"),
            "kill",
            vec![arg(None, Operand::Int(1))],
            "expected `kill()` to take no arguments",
        ),
        (
            named("process.Child"),
            "terminate",
            vec![arg(None, Operand::Int(1))],
            "expected `terminate()` to take no arguments",
        ),
        (
            named("process.Child"),
            "close",
            vec![arg(None, Operand::Int(1))],
            "expected `close()` to take no arguments",
        ),
        (
            named("process.Child"),
            "unknown",
            Vec::new(),
            "runtime has no member `process.Child.unknown`",
        ),
        (
            named("process.Pipe"),
            "read_all",
            vec![arg(None, Operand::Int(1))],
            "expected `read_all()` to take no arguments",
        ),
        (
            named("process.Pipe"),
            "read_bytes",
            Vec::new(),
            "expected `read_bytes()` to receive `max_bytes`",
        ),
        (
            named("process.Pipe"),
            "write_all",
            Vec::new(),
            "expected `write_all()` to receive `text`",
        ),
        (
            named("process.Pipe"),
            "write_bytes",
            Vec::new(),
            "expected `write_bytes()` to receive `bytes`",
        ),
        (
            named("process.Pipe"),
            "flush",
            vec![arg(None, Operand::Int(1))],
            "expected `flush()` to take no arguments",
        ),
        (
            named("process.Pipe"),
            "close",
            vec![arg(None, Operand::Int(1))],
            "expected `close()` to take no arguments",
        ),
        (
            named("process.Pipe"),
            "unknown",
            Vec::new(),
            "runtime has no member `process.Pipe.unknown`",
        ),
        (
            named("process.Completed"),
            "status",
            vec![arg(None, Operand::Int(1))],
            "expected `status()` to take no arguments",
        ),
        (
            named("process.Completed"),
            "success",
            vec![arg(None, Operand::Int(1))],
            "expected `success()` to take no arguments",
        ),
        (
            named("process.Completed"),
            "stdout",
            vec![arg(None, Operand::Int(1))],
            "expected `stdout()` to take no arguments",
        ),
        (
            named("process.Completed"),
            "stderr",
            vec![arg(None, Operand::Int(1))],
            "expected `stderr()` to take no arguments",
        ),
        (
            named("process.Completed"),
            "stdout_bytes",
            vec![arg(None, Operand::Int(1))],
            "expected `stdout_bytes()` to take no arguments",
        ),
        (
            named("process.Completed"),
            "stderr_bytes",
            vec![arg(None, Operand::Int(1))],
            "expected `stderr_bytes()` to take no arguments",
        ),
        (
            named("process.Completed"),
            "check",
            vec![arg(None, Operand::Int(1))],
            "expected `check()` to take no arguments",
        ),
        (
            named("process.Completed"),
            "unknown",
            Vec::new(),
            "runtime has no member `process.Completed.unknown`",
        ),
        (
            named("process.Supervisor"),
            "start",
            Vec::new(),
            "expected `start()` to receive `name`",
        ),
        (
            named("process.Supervisor"),
            "start",
            vec![arg(Some("name"), Operand::String("worker".to_string()))],
            "expected `start()` to receive `command`",
        ),
        (
            named("process.Supervisor"),
            "stop",
            vec![arg(None, Operand::Int(1))],
            "expected `stop()` to take no arguments",
        ),
        (
            named("process.Supervisor"),
            "is_empty",
            vec![arg(None, Operand::Int(1))],
            "expected `is_empty()` to take no arguments",
        ),
        (
            named("process.Supervisor"),
            "close",
            vec![arg(None, Operand::Int(1))],
            "expected `close()` to take no arguments",
        ),
        (
            named("process.Supervisor"),
            "unknown",
            Vec::new(),
            "runtime has no member `process.Supervisor.unknown`",
        ),
        (
            named("net.TcpListener"),
            "local_addr",
            vec![arg(None, Operand::Int(1))],
            "expected `local_addr()` to take no arguments",
        ),
        (
            named("net.TcpListener"),
            "close",
            vec![arg(None, Operand::Int(1))],
            "expected `close()` to take no arguments",
        ),
        (
            named("net.TcpListener"),
            "unknown",
            Vec::new(),
            "runtime has no member `net.TcpListener.unknown`",
        ),
        (
            named("net.TcpStream"),
            "read_bytes",
            Vec::new(),
            "expected `read_bytes()` to receive `max_bytes`",
        ),
        (
            named("net.TcpStream"),
            "read_exact",
            Vec::new(),
            "expected `read_exact()` to receive `count`",
        ),
        (
            named("net.TcpStream"),
            "write_all",
            Vec::new(),
            "expected `write_all()` to receive `text`",
        ),
        (
            named("net.TcpStream"),
            "write_bytes",
            Vec::new(),
            "expected `write_bytes()` to receive `bytes`",
        ),
        (
            named("net.TcpStream"),
            "flush",
            vec![arg(None, Operand::Int(1))],
            "expected `flush()` to take no arguments",
        ),
        (
            named("net.TcpStream"),
            "local_addr",
            vec![arg(None, Operand::Int(1))],
            "expected `local_addr()` to take no arguments",
        ),
        (
            named("net.TcpStream"),
            "peer_addr",
            vec![arg(None, Operand::Int(1))],
            "expected `peer_addr()` to take no arguments",
        ),
        (
            named("net.TcpStream"),
            "shutdown_read",
            vec![arg(None, Operand::Int(1))],
            "expected `shutdown_read()` to take no arguments",
        ),
        (
            named("net.TcpStream"),
            "shutdown_write",
            vec![arg(None, Operand::Int(1))],
            "expected `shutdown_write()` to take no arguments",
        ),
        (
            named("net.TcpStream"),
            "shutdown_both",
            vec![arg(None, Operand::Int(1))],
            "expected `shutdown_both()` to take no arguments",
        ),
        (
            named("net.TcpStream"),
            "close",
            vec![arg(None, Operand::Int(1))],
            "expected `close()` to take no arguments",
        ),
        (
            named("net.TcpStream"),
            "unknown",
            Vec::new(),
            "runtime has no member `net.TcpStream.unknown`",
        ),
        (
            named("net.UdpSocket"),
            "send_text",
            Vec::new(),
            "expected `send_text()` to receive `address`",
        ),
        (
            named("net.UdpSocket"),
            "send_text",
            vec![arg(
                Some("address"),
                Operand::String("127.0.0.1:9".to_string()),
            )],
            "expected `send_text()` to receive `text`",
        ),
        (
            named("net.UdpSocket"),
            "send_bytes",
            Vec::new(),
            "expected `send_bytes()` to receive `address`",
        ),
        (
            named("net.UdpSocket"),
            "send_bytes",
            vec![arg(
                Some("address"),
                Operand::String("127.0.0.1:9".to_string()),
            )],
            "expected `send_bytes()` to receive `bytes`",
        ),
        (
            named("net.UdpSocket"),
            "recv",
            Vec::new(),
            "expected `recv()` to receive `max_bytes`",
        ),
        (
            named("net.UdpSocket"),
            "recv_from",
            Vec::new(),
            "expected `recv_from()` to receive `max_bytes`",
        ),
        (
            named("net.UdpSocket"),
            "unknown",
            Vec::new(),
            "runtime has no member `net.UdpSocket.unknown`",
        ),
        (
            named("net.UdpDatagram"),
            "unknown",
            Vec::new(),
            "runtime has no member `net.UdpDatagram.unknown`",
        ),
        (
            named("net.HttpListener"),
            "unknown",
            Vec::new(),
            "runtime has no member `net.HttpListener.unknown`",
        ),
        (
            named("net.HttpExchange"),
            "unknown",
            Vec::new(),
            "runtime has no member `net.HttpExchange.unknown`",
        ),
        (
            named("net.HttpResponse"),
            "unknown",
            Vec::new(),
            "runtime has no member `net.HttpResponse.unknown`",
        ),
        (
            named("net.WebSocketListener"),
            "unknown",
            Vec::new(),
            "runtime has no member `net.WebSocketListener.unknown`",
        ),
        (
            named("net.WebSocket"),
            "send_text",
            Vec::new(),
            "expected `send_text()` to receive `text`",
        ),
        (
            named("net.WebSocket"),
            "send_bytes",
            Vec::new(),
            "expected `send_bytes()` to receive `bytes`",
        ),
        (
            named("net.WebSocket"),
            "unknown",
            Vec::new(),
            "runtime has no member `net.WebSocket.unknown`",
        ),
        (
            named("net.UnixListener"),
            "unknown",
            Vec::new(),
            "runtime has no member `net.UnixListener.unknown`",
        ),
        (
            named("net.UnixStream"),
            "read_exact",
            Vec::new(),
            "expected `read_exact()` to receive `count`",
        ),
        (
            named("net.UnixStream"),
            "write_all",
            Vec::new(),
            "expected `write_all()` to receive `text`",
        ),
        (
            named("net.UnixStream"),
            "unknown",
            Vec::new(),
            "runtime has no member `net.UnixStream.unknown`",
        ),
        (
            named("net.TlsListener"),
            "unknown",
            Vec::new(),
            "runtime has no member `net.TlsListener.unknown`",
        ),
        (
            named("net.TlsStream"),
            "read_exact",
            Vec::new(),
            "expected `read_exact()` to receive `count`",
        ),
        (
            named("net.TlsStream"),
            "write_all",
            Vec::new(),
            "expected `write_all()` to receive `text`",
        ),
        (
            named("net.TlsStream"),
            "unknown",
            Vec::new(),
            "runtime has no member `net.TlsStream.unknown`",
        ),
    ];

    for (object_ty, field, args, expected) in cases {
        let module = module_with_main_member_call_result_type(
            "resource",
            object_ty,
            opaque_value(),
            Type::named("int32"),
            field,
            args,
        );
        let error = emit_host_object(&module).expect_err("invalid resource member call");
        assert!(
            error.contains(expected),
            "expected `{expected}` in `{error}`"
        );
    }
}

#[test]
fn direct_backend_collection_member_argument_errors_cover_core_runtime_paths() {
    let arg = |name: Option<&str>, value: Operand| MirArg {
        name: name.map(str::to_string),
        value,
        writeback_place: None,
    };
    let opaque_value = || Rvalue::Use(Operand::String("opaque-resource".to_string()));
    let vec_int = || Type::Named("list".to_string(), vec![Type::named("int32")]);
    let map_string_int = || {
        Type::Named(
            "dict".to_string(),
            vec![Type::named("str"), Type::named("int32")],
        )
    };
    let set_string = || Type::Named("set".to_string(), vec![Type::named("str")]);
    let queue_int = || Type::Named("Queue".to_string(), vec![Type::named("int32")]);
    let task_int = || Type::Named("Task".to_string(), vec![Type::named("int32")]);

    let cases = vec![
        (
            Type::named("str"),
            "to_string",
            vec![arg(None, Operand::Int(1))],
            // The checker gives `to_string()` only to numbers and `bool`.
            "runtime has no member `str.to_string`",
        ),
        (
            Type::named("str"),
            "clone",
            vec![arg(None, Operand::Int(1))],
            "expected `clone()` to take no arguments",
        ),
        (
            Type::named("str"),
            "len",
            vec![arg(None, Operand::Int(1))],
            "expected `len()` to take no arguments",
        ),
        (
            Type::named("str"),
            "contains",
            Vec::new(),
            "expected `contains()` to receive `text`",
        ),
        (
            Type::named("str"),
            "split",
            Vec::new(),
            "expected `split()` to receive `text`",
        ),
        (
            Type::named("str"),
            "replace",
            vec![arg(None, Operand::String("from".to_string()))],
            "expected `replace()` to receive `to`",
        ),
        (
            Type::named("str"),
            "add",
            Vec::new(),
            "runtime has no member `str.add`",
        ),
        (
            Type::named("str"),
            "to_lower",
            vec![arg(None, Operand::Int(1))],
            "expected `to_lower()` to take no arguments",
        ),
        (
            Type::named("str"),
            "to_upper",
            vec![arg(None, Operand::Int(1))],
            "expected `to_upper()` to take no arguments",
        ),
        (
            Type::named("str"),
            "join",
            Vec::new(),
            "expected `join()` to receive `parts`",
        ),
        (
            Type::named("str"),
            "strip_prefix",
            Vec::new(),
            "expected `strip_prefix()` to receive `text`",
        ),
        (
            Type::named("str"),
            "trim",
            vec![arg(None, Operand::Int(1))],
            "expected `trim()` to take no arguments",
        ),
        (
            Type::named("str"),
            "unknown",
            Vec::new(),
            "runtime has no member `str.unknown`",
        ),
        (
            vec_int(),
            "len",
            vec![arg(None, Operand::Int(1))],
            "expected `len()` to take no arguments",
        ),
        (
            vec_int(),
            "is_empty",
            vec![arg(None, Operand::Int(1))],
            "expected `is_empty()` to take no arguments",
        ),
        (
            vec_int(),
            "append",
            Vec::new(),
            "expected `append()` to receive `value`",
        ),
        (
            vec_int(),
            "pop",
            vec![arg(None, Operand::Int(1)), arg(None, Operand::Int(2))],
            "expected `pop()` to take at most 1 argument(s), found 2",
        ),
        (
            vec_int(),
            "get",
            Vec::new(),
            "expected `get()` to receive `index`",
        ),
        (
            vec_int(),
            "__index_option",
            Vec::new(),
            "expected `__index_option()` to receive `index`",
        ),
        (
            vec_int(),
            "__index",
            vec![arg(None, Operand::Int(0))],
            "expected `__index()` to receive `line`",
        ),
        (
            vec_int(),
            "set",
            vec![arg(None, Operand::Int(0))],
            "expected `set()` to receive `value`",
        ),
        (
            vec_int(),
            "__set_index",
            vec![arg(None, Operand::Int(0))],
            "expected `__set_index()` to receive `value`",
        ),
        (
            vec_int(),
            "remove",
            Vec::new(),
            "expected `remove()` to receive `value`",
        ),
        (
            vec_int(),
            "swap",
            vec![arg(None, Operand::Int(0))],
            "expected `swap()` to receive `second`",
        ),
        (
            vec_int(),
            "contains",
            Vec::new(),
            "expected `contains()` to receive `value`",
        ),
        (
            vec_int(),
            "insert",
            vec![arg(None, Operand::Int(0))],
            "expected `insert()` to receive `value`",
        ),
        (
            vec_int(),
            "clear",
            vec![arg(None, Operand::Int(1))],
            "expected `clear()` to take no arguments",
        ),
        (
            vec_int(),
            "reverse",
            vec![arg(None, Operand::Int(1))],
            "expected `reverse()` to take no arguments",
        ),
        (
            vec_int(),
            "extend",
            Vec::new(),
            "expected `extend()` to receive `other`",
        ),
        (
            vec_int(),
            "unknown",
            Vec::new(),
            "runtime has no member `list.unknown`",
        ),
        (
            map_string_int(),
            "len",
            vec![arg(None, Operand::Int(1))],
            "expected `len()` to take no arguments",
        ),
        (
            map_string_int(),
            "is_empty",
            vec![arg(None, Operand::Int(1))],
            "expected `is_empty()` to take no arguments",
        ),
        (
            map_string_int(),
            "get",
            Vec::new(),
            "expected `get()` to receive `key`",
        ),
        (
            map_string_int(),
            "__index",
            vec![arg(None, Operand::String("a".to_string()))],
            "expected `__index()` to receive `line`",
        ),
        (
            map_string_int(),
            "set",
            vec![arg(None, Operand::String("a".to_string()))],
            "expected `set()` to receive `value`",
        ),
        (
            map_string_int(),
            "__set_index",
            vec![arg(None, Operand::String("a".to_string()))],
            "expected `__set_index()` to receive `value`",
        ),
        (
            map_string_int(),
            "remove",
            Vec::new(),
            "expected `remove()` to receive `key`",
        ),
        (
            map_string_int(),
            "contains_key",
            Vec::new(),
            "expected `contains_key()` to receive `key`",
        ),
        (
            map_string_int(),
            "keys",
            vec![arg(None, Operand::Int(1))],
            "expected `keys()` to take no arguments",
        ),
        (
            map_string_int(),
            "values",
            vec![arg(None, Operand::Int(1))],
            "expected `values()` to take no arguments",
        ),
        (
            map_string_int(),
            "items",
            vec![arg(None, Operand::Int(1))],
            "expected `items()` to take no arguments",
        ),
        (
            map_string_int(),
            "clear",
            vec![arg(None, Operand::Int(1))],
            "expected `clear()` to take no arguments",
        ),
        (
            map_string_int(),
            "update",
            Vec::new(),
            "expected `update()` to receive `other`",
        ),
        (
            map_string_int(),
            "unknown",
            Vec::new(),
            "runtime has no member `dict.unknown`",
        ),
        (
            set_string(),
            "len",
            vec![arg(None, Operand::Int(1))],
            "expected `len()` to take no arguments",
        ),
        (
            set_string(),
            "is_empty",
            vec![arg(None, Operand::Int(1))],
            "expected `is_empty()` to take no arguments",
        ),
        (
            set_string(),
            "contains",
            Vec::new(),
            "expected `contains()` to receive `value`",
        ),
        (
            set_string(),
            "add",
            Vec::new(),
            "expected `add()` to receive `value`",
        ),
        (
            set_string(),
            "remove",
            Vec::new(),
            "expected `remove()` to receive `value`",
        ),
        (
            set_string(),
            "__index_option",
            Vec::new(),
            "expected `__index_option()` to receive `index`",
        ),
        (
            set_string(),
            "unknown",
            Vec::new(),
            "runtime has no member `set.unknown`",
        ),
        (
            queue_int(),
            "put",
            Vec::new(),
            "expected `put()` to receive `value`",
        ),
        (
            queue_int(),
            "try_put",
            Vec::new(),
            "expected `try_put()` to receive `value`",
        ),
        (
            queue_int(),
            "close",
            vec![arg(None, Operand::Int(1))],
            "expected `close()` to take no arguments",
        ),
        (
            queue_int(),
            "unknown",
            Vec::new(),
            "runtime has no member `Queue.unknown`",
        ),
        (
            Type::named("TaskGroup"),
            "cancel",
            vec![arg(None, Operand::Int(1))],
            "expected `cancel()` to take no arguments",
        ),
        (
            Type::named("TaskGroup"),
            "close",
            vec![arg(None, Operand::Int(1))],
            "runtime has no member `TaskGroup.close`",
        ),
        (
            task_int(),
            "unknown",
            Vec::new(),
            "runtime has no member `Task.unknown`",
        ),
    ];

    for (object_ty, field, args, expected) in cases {
        let module = module_with_main_member_call_result_type(
            "resource",
            object_ty,
            opaque_value(),
            Type::named("int32"),
            field,
            args,
        );
        let error = emit_host_object(&module).expect_err("invalid collection member call");
        assert!(
            error.contains(expected),
            "expected `{expected}` in `{error}`"
        );
    }
}

#[test]
fn direct_backend_resource_member_success_paths_cover_remaining_network_surfaces() {
    let arg = |name: Option<&str>, value: Operand| MirArg {
        name: name.map(str::to_string),
        value,
        writeback_place: None,
    };
    let opaque_value = || Rvalue::Use(Operand::String("opaque-resource".to_string()));
    let named = |name: &str| Type::Named(name.to_string(), Vec::new());
    let vec_uint8 = || Type::Named("list".to_string(), vec![Type::named("uint8")]);
    let option = |ty: Type| crate::sema::optional_type(ty);
    let result = |ok: Type| {
        Type::Named(
            "Result".to_string(),
            vec![ok, Type::Named("io.Error".to_string(), Vec::new())],
        )
    };
    let process_result = |ok: Type| {
        Type::Named(
            "Result".to_string(),
            vec![ok, Type::Named("process.Error".to_string(), Vec::new())],
        )
    };
    let string_arg = |name: &str, value: &str| arg(Some(name), Operand::String(value.to_string()));

    let cases = vec![
        (
            named("fs.File"),
            "read_all",
            Vec::new(),
            result(Type::named("str")),
        ),
        (
            named("fs.File"),
            "read_bytes",
            Vec::new(),
            result(vec_uint8()),
        ),
        (
            named("fs.File"),
            "write_all",
            vec![string_arg("text", "payload")],
            result(Type::Unit),
        ),
        (
            named("fs.File"),
            "write_bytes",
            vec![string_arg("bytes", "payload")],
            result(Type::Unit),
        ),
        (named("fs.File"), "flush", Vec::new(), result(Type::Unit)),
        (named("fs.File"), "close", Vec::new(), Type::Unit),
        (
            named("process.Child"),
            "stdin",
            Vec::new(),
            option(named("process.Pipe")),
        ),
        (
            named("process.Child"),
            "stdout",
            Vec::new(),
            option(named("process.Pipe")),
        ),
        (
            named("process.Child"),
            "stderr",
            Vec::new(),
            option(named("process.Pipe")),
        ),
        (
            named("process.Child"),
            "wait",
            Vec::new(),
            named("process.Wait"),
        ),
        (
            named("process.Child"),
            "wait_or_none",
            Vec::new(),
            process_result(option(named("process.ExitStatus"))),
        ),
        (
            named("process.Child"),
            "wait_ok",
            Vec::new(),
            process_result(named("process.ExitStatus")),
        ),
        (
            named("process.Child"),
            "kill",
            Vec::new(),
            process_result(Type::Unit),
        ),
        (
            named("process.Child"),
            "terminate",
            Vec::new(),
            process_result(Type::Unit),
        ),
        (named("process.Child"), "close", Vec::new(), Type::Unit),
        (
            named("process.Pipe"),
            "read_all",
            Vec::new(),
            process_result(Type::named("str")),
        ),
        (
            named("process.Pipe"),
            "read_line",
            Vec::new(),
            process_result(option(Type::named("str"))),
        ),
        (
            named("process.Pipe"),
            "read_bytes",
            vec![arg(Some("max_bytes"), Operand::Int(4))],
            process_result(option(vec_uint8())),
        ),
        (
            named("process.Pipe"),
            "write_all",
            vec![string_arg("text", "payload")],
            process_result(Type::Unit),
        ),
        (
            named("process.Pipe"),
            "write_bytes",
            vec![string_arg("bytes", "payload")],
            process_result(Type::Unit),
        ),
        (
            named("process.Pipe"),
            "flush",
            Vec::new(),
            process_result(Type::Unit),
        ),
        (named("process.Pipe"), "close", Vec::new(), Type::Unit),
        (
            named("process.Completed"),
            "status",
            Vec::new(),
            named("process.ExitStatus"),
        ),
        (
            named("process.Completed"),
            "success",
            Vec::new(),
            Type::named("bool"),
        ),
        (
            named("process.Completed"),
            "stdout",
            Vec::new(),
            Type::named("str"),
        ),
        (
            named("process.Completed"),
            "stderr",
            Vec::new(),
            Type::named("str"),
        ),
        (
            named("process.Completed"),
            "stdout_bytes",
            Vec::new(),
            vec_uint8(),
        ),
        (
            named("process.Completed"),
            "stderr_bytes",
            Vec::new(),
            vec_uint8(),
        ),
        (
            named("process.Completed"),
            "check",
            Vec::new(),
            process_result(Type::Unit),
        ),
        (
            named("process.Supervisor"),
            "start",
            vec![
                string_arg("name", "worker"),
                arg(Some("command"), Operand::String("aura-worker".to_string())),
            ],
            process_result(Type::Unit),
        ),
        (
            named("process.Supervisor"),
            "start",
            vec![
                string_arg("name", "configured"),
                arg(Some("command"), Operand::String("aura-worker".to_string())),
                arg(Some("cwd"), Operand::Unit),
                arg(Some("env"), Operand::String("env".to_string())),
                arg(Some("stdin"), Operand::String("stdin".to_string())),
                arg(Some("stdout"), Operand::String("stdout".to_string())),
                arg(Some("stderr"), Operand::String("stderr".to_string())),
                arg(Some("restart"), Operand::String("restart".to_string())),
                arg(Some("backoff"), Operand::Duration(10)),
                arg(Some("max_restarts"), Operand::Int(1)),
                arg(Some("group"), Operand::Bool(false)),
            ],
            process_result(Type::Unit),
        ),
        (
            named("process.Supervisor"),
            "wait",
            Vec::new(),
            named("process.SupervisorWait"),
        ),
        (
            named("process.Supervisor"),
            "wait_or_none",
            Vec::new(),
            process_result(option(named("process.SupervisorEvent"))),
        ),
        (
            named("process.Supervisor"),
            "stop",
            Vec::new(),
            process_result(Type::Unit),
        ),
        (
            named("process.Supervisor"),
            "is_empty",
            Vec::new(),
            Type::named("bool"),
        ),
        (named("process.Supervisor"), "close", Vec::new(), Type::Unit),
        (
            named("net.TcpStream"),
            "local_addr",
            Vec::new(),
            result(Type::named("str")),
        ),
        (
            named("net.TcpStream"),
            "read_all",
            Vec::new(),
            result(Type::named("str")),
        ),
        (
            named("net.TcpStream"),
            "peer_addr",
            Vec::new(),
            result(Type::named("str")),
        ),
        (
            named("net.TcpStream"),
            "shutdown_read",
            Vec::new(),
            result(Type::Unit),
        ),
        (
            named("net.TcpStream"),
            "shutdown_write",
            Vec::new(),
            result(Type::Unit),
        ),
        (
            named("net.TcpStream"),
            "shutdown_both",
            Vec::new(),
            result(Type::Unit),
        ),
        (
            named("net.UdpSocket"),
            "send_bytes",
            vec![
                string_arg("address", "127.0.0.1:9"),
                string_arg("bytes", "payload"),
            ],
            result(Type::Unit),
        ),
        (
            named("net.UdpSocket"),
            "recv",
            vec![arg(Some("max_bytes"), Operand::Int(32))],
            result(option(vec_uint8())),
        ),
        (
            named("net.UdpSocket"),
            "peer_addr",
            Vec::new(),
            result(Type::named("str")),
        ),
        (named("net.UdpSocket"), "close", Vec::new(), Type::Unit),
        (named("net.UdpDatagram"), "bytes", Vec::new(), vec_uint8()),
        (
            named("net.UdpDatagram"),
            "text",
            Vec::new(),
            result(Type::named("str")),
        ),
        (
            named("net.HttpExchange"),
            "body_bytes",
            Vec::new(),
            vec_uint8(),
        ),
        (
            named("net.HttpExchange"),
            "respond_bytes",
            vec![
                arg(Some("status"), Operand::Int(200)),
                string_arg("bytes", "payload"),
                string_arg("headers", "headers"),
            ],
            result(Type::Unit),
        ),
        (
            named("net.HttpResponse"),
            "reason",
            Vec::new(),
            Type::named("str"),
        ),
        (
            named("net.HttpResponse"),
            "headers",
            Vec::new(),
            Type::Named(
                "dict".to_string(),
                vec![Type::named("str"), Type::named("str")],
            ),
        ),
        (named("net.HttpResponse"), "bytes", Vec::new(), vec_uint8()),
        (
            named("net.WebSocket"),
            "send_bytes",
            vec![string_arg("bytes", "payload")],
            result(Type::Unit),
        ),
        (
            named("net.WebSocket"),
            "recv_bytes",
            Vec::new(),
            result(option(vec_uint8())),
        ),
        (named("net.WebSocket"), "close", Vec::new(), Type::Unit),
        (
            named("net.UnixStream"),
            "read_exact",
            vec![arg(Some("count"), Operand::Int(4))],
            result(vec_uint8()),
        ),
        (
            named("net.UnixStream"),
            "write_all",
            vec![string_arg("text", "payload")],
            result(Type::Unit),
        ),
        (named("net.UnixStream"), "close", Vec::new(), Type::Unit),
        (
            named("net.TlsStream"),
            "read_line",
            Vec::new(),
            result(option(Type::named("str"))),
        ),
        (
            named("net.TlsStream"),
            "read_exact",
            vec![arg(Some("count"), Operand::Int(4))],
            result(vec_uint8()),
        ),
        (
            named("net.TlsStream"),
            "write_all",
            vec![string_arg("text", "payload")],
            result(Type::Unit),
        ),
        (named("net.TlsStream"), "close", Vec::new(), Type::Unit),
    ];

    for (object_ty, field, args, result_ty) in cases {
        let module = module_with_main_member_call_result_type(
            "resource",
            object_ty,
            opaque_value(),
            result_ty,
            field,
            args,
        );
        let object = emit_host_object(&module).expect("resource member call should compile");
        assert!(!object.is_empty(), "{field}");
    }
}

#[test]
fn direct_backend_runtime_member_arity_errors_cover_string_collection_and_runtime_paths() {
    let string_ty = Type::named("str");
    let string_value = Rvalue::Use(Operand::String("Aura".to_string()));
    let vec_ty = Type::Named("list".to_string(), vec![Type::named("int32")]);
    let vec_value = Rvalue::VecLiteral {
        element_type: Type::named("int32"),
        elements: vec![Operand::Int(1), Operand::Int(2)],
    };
    let map_ty = Type::Named(
        "dict".to_string(),
        vec![Type::named("str"), Type::named("int32")],
    );
    let map_value = Rvalue::MapLiteral {
        key_type: Type::named("str"),
        value_type: Type::named("int32"),
        entries: vec![MirMapEntry {
            key: Operand::String("count".to_string()),
            value: Operand::Int(1),
        }],
    };
    let set_ty = Type::Named("set".to_string(), vec![Type::named("str")]);
    let set_value = Rvalue::SetLiteral {
        element_type: Type::named("str"),
        elements: vec![Operand::String("ready".to_string())],
    };
    let channel_ty = Type::Named("Queue".to_string(), vec![Type::named("int32")]);
    let channel_value = Rvalue::Call {
        callee: CallTarget::Name("Queue".to_string()),
        args: Vec::new(),
        type_args: Default::default(),
    };
    let task_ty = Type::Named("Task".to_string(), vec![Type::named("int32")]);
    let task_value = Rvalue::Use(Operand::String("opaque-task".to_string()));
    let task_group_ty = Type::named("TaskGroup");
    let task_group_value = Rvalue::Call {
        callee: CallTarget::Name("TaskGroup".to_string()),
        args: Vec::new(),
        type_args: Default::default(),
    };
    let arg = |value| MirArg {
        name: None,
        value,
        writeback_place: None,
    };

    let cases = vec![
        (
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("int64"),
                "len",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            ),
            "expected `len()` to take no arguments",
        ),
        (
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("bool"),
                "contains",
                Vec::new(),
            ),
            "expected `contains()` to receive `text`",
        ),
        (
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("str"),
                "replace",
                vec![MirArg {
                    name: None,
                    value: Operand::String("Aur".to_string()),
                    writeback_place: None,
                }],
            ),
            "expected `replace()` to receive `to`",
        ),
        (
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("str"),
                "trim",
                vec![MirArg {
                    name: None,
                    value: Operand::String("x".to_string()),
                    writeback_place: None,
                }],
            ),
            "expected `trim()` to take no arguments",
        ),
        (
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("str"),
                "to_string",
                vec![arg(Operand::Int(1))],
            ),
            // The checker gives `to_string()` only to numbers and `bool`.
            "runtime has no member `str.to_string`",
        ),
        (
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::Named("list".to_string(), vec![Type::named("str")]),
                "split",
                Vec::new(),
            ),
            "expected `split()` to receive `text`",
        ),
        (
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("str"),
                "add",
                Vec::new(),
            ),
            "runtime has no member `str.add`",
        ),
        (
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("str"),
                "to_lower",
                vec![arg(Operand::String("x".to_string()))],
            ),
            "expected `to_lower()` to take no arguments",
        ),
        (
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("str"),
                "to_upper",
                vec![arg(Operand::String("x".to_string()))],
            ),
            "expected `to_upper()` to take no arguments",
        ),
        (
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                crate::sema::optional_type(Type::named("str")),
                "strip_prefix",
                Vec::new(),
            ),
            "expected `strip_prefix()` to receive `text`",
        ),
        (
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value,
                Type::named("str"),
                "unknown",
                Vec::new(),
            ),
            "runtime has no member `str.unknown`",
        ),
        (
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::named("bool"),
                "append",
                Vec::new(),
            ),
            "expected `append()` to receive `value`",
        ),
        (
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::named("bool"),
                "is_empty",
                vec![arg(Operand::Int(1))],
            ),
            "expected `is_empty()` to take no arguments",
        ),
        (
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::named("int32"),
                "pop",
                vec![arg(Operand::Int(1)), arg(Operand::Int(2))],
            ),
            "expected `pop()` to take at most 1 argument(s), found 2",
        ),
        (
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                crate::sema::lookup_type(Type::named("int32")),
                "get",
                Vec::new(),
            ),
            "expected `get()` to receive `index`",
        ),
        (
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::named("int32"),
                "__index",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(0),
                    writeback_place: None,
                }],
            ),
            "expected `__index()` to receive `line`",
        ),
        (
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::Unit,
                "__set_index",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(0),
                    writeback_place: None,
                }],
            ),
            "expected `__set_index()` to receive `value`",
        ),
        (
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::named("bool"),
                "swap",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(0),
                    writeback_place: None,
                }],
            ),
            "expected `swap()` to receive `second`",
        ),
        (
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::named("int32"),
                "set",
                vec![arg(Operand::Int(0))],
            ),
            "expected `set()` to receive `value`",
        ),
        (
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::Unit,
                "remove",
                Vec::new(),
            ),
            "expected `remove()` to receive `value`",
        ),
        (
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::named("bool"),
                "contains",
                Vec::new(),
            ),
            "expected `contains()` to receive `value`",
        ),
        (
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::Unit,
                "reverse",
                vec![arg(Operand::Int(1))],
            ),
            "expected `reverse()` to take no arguments",
        ),
        (
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::Unit,
                "extend",
                Vec::new(),
            ),
            "expected `extend()` to receive `other`",
        ),
        (
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::named("bool"),
                "unknown",
                Vec::new(),
            ),
            "runtime has no member `list.unknown`",
        ),
        (
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::named("int32"),
                "__index",
                vec![MirArg {
                    name: None,
                    value: Operand::String("count".to_string()),
                    writeback_place: None,
                }],
            ),
            "expected `__index()` to receive `line`",
        ),
        (
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::Unit,
                "__set_index",
                vec![MirArg {
                    name: None,
                    value: Operand::String("count".to_string()),
                    writeback_place: None,
                }],
            ),
            "expected `__set_index()` to receive `value`",
        ),
        (
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::named("bool"),
                "contains_key",
                Vec::new(),
            ),
            "expected `contains_key()` to receive `key`",
        ),
        (
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::named("bool"),
                "is_empty",
                vec![arg(Operand::Int(1))],
            ),
            "expected `is_empty()` to take no arguments",
        ),
        (
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                crate::sema::lookup_type(Type::named("int32")),
                "get",
                Vec::new(),
            ),
            "expected `get()` to receive `key`",
        ),
        (
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                crate::sema::lookup_type(Type::named("int32")),
                "remove",
                Vec::new(),
            ),
            "expected `remove()` to receive `key`",
        ),
        (
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::Named("list".to_string(), vec![Type::named("str")]),
                "keys",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            ),
            "expected `keys()` to take no arguments",
        ),
        (
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::Named("list".to_string(), vec![Type::named("int32")]),
                "items",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            ),
            "expected `items()` to take no arguments",
        ),
        (
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::Named("list".to_string(), vec![Type::named("int32")]),
                "values",
                vec![arg(Operand::Int(1))],
            ),
            "expected `values()` to take no arguments",
        ),
        (
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::Unit,
                "clear",
                vec![arg(Operand::Int(1))],
            ),
            "expected `clear()` to take no arguments",
        ),
        (
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::named("bool"),
                "unknown",
                Vec::new(),
            ),
            "runtime has no member `dict.unknown`",
        ),
        (
            module_with_main_member_call_result_type(
                "seen",
                set_ty.clone(),
                set_value.clone(),
                Type::named("bool"),
                "contains",
                Vec::new(),
            ),
            "expected `contains()` to receive `value`",
        ),
        (
            module_with_main_member_call_result_type(
                "seen",
                set_ty.clone(),
                set_value.clone(),
                Type::named("bool"),
                "is_empty",
                vec![arg(Operand::Int(1))],
            ),
            "expected `is_empty()` to take no arguments",
        ),
        (
            module_with_main_member_call_result_type(
                "seen",
                set_ty.clone(),
                set_value.clone(),
                Type::named("bool"),
                "add",
                Vec::new(),
            ),
            "expected `add()` to receive `value`",
        ),
        (
            module_with_main_member_call_result_type(
                "seen",
                set_ty.clone(),
                set_value.clone(),
                Type::named("bool"),
                "remove",
                Vec::new(),
            ),
            "expected `remove()` to receive `value`",
        ),
        (
            module_with_main_member_call_result_type(
                "seen",
                set_ty.clone(),
                set_value.clone(),
                crate::sema::lookup_type(Type::named("str")),
                "__index_option",
                Vec::new(),
            ),
            "expected `__index_option()` to receive `index`",
        ),
        (
            module_with_main_member_call_result_type(
                "seen",
                set_ty.clone(),
                set_value.clone(),
                Type::named("bool"),
                "unknown",
                Vec::new(),
            ),
            "runtime has no member `set.unknown`",
        ),
        (
            module_with_main_member_call_result_type(
                "jobs",
                channel_ty.clone(),
                channel_value.clone(),
                Type::Unit,
                "put",
                Vec::new(),
            ),
            "expected `put()` to receive `value`",
        ),
        (
            module_with_main_member_call_result_type(
                "jobs",
                channel_ty.clone(),
                channel_value.clone(),
                Type::Unit,
                "put",
                vec![MirArg {
                    name: Some("delay".to_string()),
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            ),
            "`put()` has no parameter `delay`",
        ),
        (
            module_with_main_member_call_result_type(
                "jobs",
                channel_ty.clone(),
                channel_value.clone(),
                Type::Unit,
                "put",
                vec![
                    arg(Operand::Int(1)),
                    arg(Operand::Int(2)),
                    arg(Operand::Int(3)),
                ],
            ),
            "expected `put()` to take at most 2 argument(s), found 3",
        ),
        (
            module_with_main_member_call_result_type(
                "jobs",
                channel_ty.clone(),
                channel_value.clone(),
                Type::Unit,
                "try_put",
                Vec::new(),
            ),
            "expected `try_put()` to receive `value`",
        ),
        (
            module_with_main_member_call_result_type(
                "jobs",
                channel_ty.clone(),
                channel_value.clone(),
                Type::Unit,
                "try_put",
                vec![MirArg {
                    name: Some("item".to_string()),
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            ),
            "`try_put()` has no parameter `item`",
        ),
        (
            module_with_main_member_call_result_type(
                "jobs",
                channel_ty.clone(),
                channel_value.clone(),
                Type::Named("QueueReceive".to_string(), vec![Type::named("int32")]),
                "get",
                vec![MirArg {
                    name: Some("delay".to_string()),
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            ),
            "`get()` has no parameter `delay`",
        ),
        (
            module_with_main_member_call_result_type(
                "jobs",
                channel_ty.clone(),
                channel_value.clone(),
                Type::Named("QueueReceive".to_string(), vec![Type::named("int32")]),
                "get",
                vec![
                    MirArg {
                        name: None,
                        value: Operand::Int(1),
                        writeback_place: None,
                    },
                    MirArg {
                        name: None,
                        value: Operand::Int(2),
                        writeback_place: None,
                    },
                ],
            ),
            "expected `get()` to take at most 1 argument(s), found 2",
        ),
        (
            module_with_main_member_call_result_type(
                "jobs",
                channel_ty.clone(),
                channel_value.clone(),
                Type::Named("QueueReceive".to_string(), vec![Type::named("int32")]),
                "__get_in_task_group",
                Vec::new(),
            ),
            "expected `__get_in_task_group()` to receive `group`",
        ),
        (
            module_with_main_member_call_result_type(
                "jobs",
                channel_ty.clone(),
                channel_value.clone(),
                Type::Named("QueueReceive".to_string(), vec![Type::named("int32")]),
                "__get_with_registered_producers",
                vec![arg(Operand::Int(1))],
            ),
            "expected `__get_with_registered_producers()` to take no arguments",
        ),
        (
            module_with_main_member_call_result_type(
                "jobs",
                channel_ty.clone(),
                channel_value.clone(),
                crate::sema::poll_type(Type::named("int32")),
                "poll",
                vec![MirArg {
                    name: Some("delay".to_string()),
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            ),
            "`poll()` has no parameter `delay`",
        ),
        (
            module_with_main_member_call_result_type(
                "jobs",
                channel_ty.clone(),
                channel_value.clone(),
                crate::sema::poll_type(Type::named("int32")),
                "poll",
                vec![arg(Operand::Int(1)), arg(Operand::Int(2))],
            ),
            "expected `poll()` to take at most 1 argument(s), found 2",
        ),
        (
            module_with_main_member_call_result_type(
                "jobs",
                channel_ty.clone(),
                channel_value.clone(),
                Type::named("int32"),
                "get_or",
                Vec::new(),
            ),
            "expected `get_or()` to receive `default`",
        ),
        (
            module_with_main_member_call_result_type(
                "jobs",
                channel_ty.clone(),
                channel_value.clone(),
                Type::Unit,
                "close",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            ),
            "expected `close()` to take no arguments",
        ),
        (
            module_with_main_member_call_result_type(
                "jobs",
                channel_ty.clone(),
                channel_value.clone(),
                Type::named("bool"),
                "unknown",
                Vec::new(),
            ),
            "runtime has no member `Queue.unknown`",
        ),
        (
            module_with_main_member_call_result_type(
                "task",
                task_ty.clone(),
                task_value.clone(),
                Type::named("int32"),
                "result",
                vec![MirArg {
                    name: Some("delay".to_string()),
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            ),
            "`result()` has no parameter `delay`",
        ),
        (
            module_with_main_member_call_result_type(
                "task",
                task_ty.clone(),
                task_value.clone(),
                Type::named("int32"),
                "result",
                vec![arg(Operand::Int(1)), arg(Operand::Int(2))],
            ),
            "expected `result()` to take at most 1 argument(s), found 2",
        ),
        (
            module_with_main_member_call_result_type(
                "task",
                task_ty.clone(),
                task_value.clone(),
                crate::sema::poll_type(Type::named("int32")),
                "poll",
                vec![MirArg {
                    name: Some("delay".to_string()),
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            ),
            "`poll()` has no parameter `delay`",
        ),
        (
            module_with_main_member_call_result_type(
                "task",
                task_ty.clone(),
                task_value.clone(),
                crate::sema::poll_type(Type::named("int32")),
                "poll",
                vec![arg(Operand::Int(1)), arg(Operand::Int(2))],
            ),
            "expected `poll()` to take at most 1 argument(s), found 2",
        ),
        (
            module_with_main_member_call_result_type(
                "task",
                task_ty.clone(),
                task_value.clone(),
                Type::named("int32"),
                "result_or",
                Vec::new(),
            ),
            "expected `result_or()` to receive `default`",
        ),
        (
            module_with_main_member_call_result_type(
                "task",
                task_ty,
                task_value,
                Type::named("bool"),
                "unknown",
                Vec::new(),
            ),
            "runtime has no member `Task.unknown`",
        ),
        (
            module_with_main_member_call_result_type(
                "group",
                task_group_ty.clone(),
                task_group_value.clone(),
                Type::Unit,
                "cancel",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            ),
            "expected `cancel()` to take no arguments",
        ),
        (
            module_with_main_member_call_result_type(
                "group",
                task_group_ty.clone(),
                task_group_value.clone(),
                Type::Unit,
                "close",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            ),
            "runtime has no member `TaskGroup.close`",
        ),
        (
            module_with_main_member_call_result_type(
                "group",
                task_group_ty,
                task_group_value,
                Type::named("bool"),
                "unknown",
                Vec::new(),
            ),
            "runtime has no member `TaskGroup.unknown`",
        ),
    ];

    for (module, expected) in cases {
        let error = emit_host_object(&module).expect_err("invalid runtime member call");
        assert!(
            error.contains(expected),
            "expected `{expected}` in `{error}`"
        );
    }
}

#[test]
fn direct_backend_manual_wait_surface_compiles() {
    let source = r#"
def worker(queue: Queue[int32], value: int32) -> int32:
    match queue.put(value, timeout=2ms):
        case Result.Ok(_):
            pass
        case Result.Err(_):
            pass
    return value + 1

def main() -> int32:
    jobs = Queue[int32](capacity=1)
    with TaskGroup() as group:
        task = group.start(worker, jobs, 7)
        receive = jobs.get(timeout=5ms)
        one = wait_any([task], timeout=5ms)
        one_positional = wait_any([task], 5ms)
        all_now = wait_all([task])
        all = wait_all([task], timeout=5ms)
        all_positional = wait_all([task], 5ms)
        match receive:
            case QueueReceive.Item(value):
                print(value)
            case QueueReceive.Closed:
                print("closed")
            case QueueReceive.TimedOut:
                print("timedout")
            case QueueReceive.Cancelled:
                print("cancelled")
        match one:
            case WaitAny.Ready(index, result):
                print(index)
                print(result)
            case WaitAny.Error(index, message):
                print(index)
                print(message)
            case WaitAny.TimedOut:
                print("timedout")
            case WaitAny.Cancelled:
                print("cancelled")
        match one_positional:
            case WaitAny.Ready(index, result):
                print(index)
                print(result)
            case WaitAny.Error(index, message):
                print(index)
                print(message)
            case WaitAny.TimedOut:
                print("timedout")
            case WaitAny.Cancelled:
                print("cancelled")
        match all:
            case WaitAll.Ready(_):
                print("ready")
            case WaitAll.Error(index, message):
                print(index)
                print(message)
            case WaitAll.TimedOut:
                print("timedout")
            case WaitAll.Cancelled:
                print("cancelled")
        match all_positional:
            case WaitAll.Ready(_):
                print("ready")
            case WaitAll.Error(index, message):
                print(index)
                print(message)
            case WaitAll.TimedOut:
                print("timedout")
            case WaitAll.Cancelled:
                print("cancelled")
        match all_now:
            case WaitAll.Ready(_):
                print("ready")
            case WaitAll.Error(index, message):
                print(index)
                print(message)
            case WaitAll.TimedOut:
                print("timedout")
            case WaitAll.Cancelled:
                print("cancelled")
    return 0
"#;

    let module = lower_source_to_mir(source).expect("manual wait source should lower");
    assert!(!emit_host_object(&module)
        .expect("manual queue/task wait surface should compile directly")
        .is_empty());
}

#[test]
fn direct_backend_rejects_a_wait_result_of_unknown_payload_type() {
    let string_vec = Type::Named("list".to_string(), vec![Type::named("str")]);
    let wait_all_unknown = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            source_path: None,
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: vec![
                MirLocalType {
                    name: "tasks".to_string(),
                    ty: string_vec.clone(),
                },
                MirLocalType {
                    name: "wait_result".to_string(),
                    ty: Type::Named("WaitAll".to_string(), vec![Type::named("Unknown")]),
                },
            ],
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: vec![
                    Instruction::Assign {
                        target: "tasks".to_string(),
                        value: Rvalue::VecLiteral {
                            elements: vec![Operand::String("not-a-task".to_string())],
                            element_type: Type::named("str"),
                        },
                    },
                    Instruction::Assign {
                        target: "wait_result".to_string(),
                        value: Rvalue::Call {
                            callee: CallTarget::Name("wait_all".to_string()),
                            args: vec![MirArg {
                                name: None,
                                value: Operand::Place("tasks".to_string()),
                                writeback_place: None,
                            }],
                            type_args: Default::default(),
                        },
                    },
                ],
                terminator: Terminator::Return(Operand::Int(0)),
            }],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };

    // MIR records a complete type for every assigned local, so the shared
    // validator rejects a wait result whose payload type is unknown.
    let error = emit_host_object(&wait_all_unknown)
        .expect_err("a wait result of unknown payload type is invalid MIR");
    assert!(
        error.contains("assigns local `wait_result` of unknown type"),
        "{error}"
    );
}

#[test]
fn direct_backend_entry_thunk_handles_unit_parameters() {
    let unit_param_main = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            source_path: None,
            span: Span::new(1, 1),
            receiver: None,
            params: vec![MirParam {
                name: "marker".to_string(),
                passing: MirReceiverKind::Value,
                ty: Type::Unit,
                default_function: None,
                keyword_only: false,
            }],
            local_types: vec![MirLocalType {
                name: "marker".to_string(),
                ty: Type::Unit,
            }],
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: Vec::new(),
                terminator: Terminator::Return(Operand::Int(0)),
            }],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };

    assert!(!emit_host_object(&unit_param_main)
        .expect("direct entry thunk should lower Unit parameters")
        .is_empty());
}

#[test]
fn direct_backend_emits_object_for_process_member_surface_matrix() {
    let source = r#"
import process

def process_members() -> Result[None, process.Error]:
    completed = try process.run(["/bin/echo", "done"], stdout=process.pipe(), stderr=process.pipe(), timeout=2s, group=true)
    print(completed.status())
    print(completed.success())
    print(completed.stdout())
    print(completed.stdout_bytes())
    print(completed.stderr())
    print(completed.stderr_bytes())
    try completed.check()

    with child = try process.start(["/bin/cat"], stdin=process.pipe(), stdout=process.pipe(), stderr=process.pipe(), group=true):
        stdin: process.Pipe | None = child.stdin()
        match own stdin:
            case process.Pipe as stdin_pipe:
                try stdin_pipe.write_all(text="ping\n", timeout=500ms)
                try stdin_pipe.write_bytes(bytes=[33 as uint8, 10 as uint8], timeout=500ms)
                try stdin_pipe.flush()
                stdin_pipe.close()
            case None:
                pass
        stdout: process.Pipe | None = child.stdout()
        match own stdout:
            case process.Pipe as stdout_pipe:
                print(try stdout_pipe.read_line(timeout=500ms))
                print(try stdout_pipe.read_bytes(max_bytes=4, timeout=500ms))
                print(try stdout_pipe.read_all())
                stdout_pipe.close()
            case None:
                pass
        stderr: process.Pipe | None = child.stderr()
        match own stderr:
            case process.Pipe as stderr_pipe:
                print(try stderr_pipe.read_all())
                stderr_pipe.close()
            case None:
                pass
        print(child.wait(timeout=2s))
        print(try child.wait_or_none(timeout=1ms))
        child.close()

    with success = try process.start(["/bin/true"], stdout=process.null(), stderr=process.null(), group=true):
        print(try success.wait_ok(timeout=2s))

    with terminable = try process.start(["/bin/sleep", "10"], stdout=process.null(), stderr=process.null(), group=true):
        try terminable.terminate()
        print(terminable.wait(timeout=2s))

    with killable = try process.start(["/bin/sleep", "10"], stdout=process.null(), stderr=process.null(), group=true):
        try killable.kill()
        print(killable.wait(timeout=2s))

    with supervisor = process.supervisor():
        try supervisor.start(name="defaulted", command=["/usr/bin/false"])
        print(supervisor.wait(timeout=2s))
        env: dict[str, str] = {}
        try supervisor.start(name="explicit", command=["/usr/bin/false"], cwd=None, env=env, stdin=process.null(), stdout=process.null(), stderr=process.null(), restart=process.RestartPolicy.Never, backoff=100ms, max_restarts=0, group=true)
        print(try supervisor.wait_or_none(timeout=2s))
        print(supervisor.is_empty())
        try supervisor.stop()
    return Result.Ok(None)

def main() -> int32:
    match process_members():
        case Result.Ok(_):
            return 0
        case Result.Err(error):
            print(error)
            return 1
"#;

    let module = lower_source_to_mir(source).expect("process member source should lower");
    assert!(!emit_host_object(&module)
        .expect("process member source should compile directly")
        .is_empty());
}

#[test]
fn direct_backend_task_result_payloads_support_plain_class_values() {
    let source = r#"
class Box:
    value: int32

def make_box() -> Box:
    return Box(value=7)

def main() -> int32:
    with TaskGroup() as group:
        task = group.start(make_box)
        match task.result():
            case TaskResult.Ready(box):
                print(box.value)
            case TaskResult.Error(_message):
                print(0)
            case TaskResult.TimedOut:
                print(0)
            case TaskResult.Cancelled:
                print(0)
    return 0
"#;

    let module = lower_source_to_mir(source).expect("plain-class task result source should lower");
    assert!(!emit_host_object(&module)
        .expect("plain-class task result source should compile directly")
        .is_empty());
}

#[test]
fn native_codegen_direct_error_paths_cover_missing_entry_wrapper_and_return_type_cases() {
    let source = r#"
class Counter:
    value: int32

    def current(self) -> int32:
        return self.value

def helper(value: int32) -> int32:
    return value + 1

def main() -> int32:
    return helper(1)
"#;
    let mir = lower_source_to_mir(source).expect("source should lower to MIR");

    let method = mir
        .functions
        .iter()
        .find(|function| function.receiver.is_some())
        .cloned()
        .expect("method should be lowered as a function");
    let helper = mir
        .functions
        .iter()
        .find(|function| function.name == "helper")
        .cloned()
        .expect("helper function should exist");
    let main = mir
        .functions
        .iter()
        .find(|function| function.name == "main")
        .cloned()
        .expect("main function should exist");

    let mut method_codegen = NativeCodegen::new(&mir, "/tmp/native_codegen_errors.au", source)
        .expect("codegen should initialize");
    let thunk_error = method_codegen
        .define_function_thunk(&method)
        .expect_err("methods should still reject direct task-start thunks");
    assert!(thunk_error.contains("does not yet support task-start thunks for methods"));

    let mut broken_main = main.clone();
    broken_main.entry = "missing_block".to_string();
    let mut broken_entry_codegen =
        NativeCodegen::new(&mir, "/tmp/native_codegen_errors.au", source)
            .expect("codegen should initialize");
    let entry_error = broken_entry_codegen
        .define_function(&broken_main)
        .expect_err("missing entry blocks should be reported");
    assert!(entry_error.contains("could not find entry block `missing_block`"));

    let mut no_main_codegen = NativeCodegen::new(&mir, "/tmp/native_codegen_errors.au", source)
        .expect("codegen should initialize");
    no_main_codegen.functions.clear();
    let wrapper_error = no_main_codegen
        .define_main_wrapper()
        .expect_err("missing entrypoints should fail main wrapper generation");
    assert!(wrapper_error.contains("requires a `main` function or top-level script"));

    let mut missing_return_codegen =
        NativeCodegen::new(&mir, "/tmp/native_codegen_errors.au", source)
            .expect("codegen should initialize");
    missing_return_codegen
        .function_return_types
        .remove(&helper.name);
    let return_error = missing_return_codegen
        .define_function_thunk(&helper)
        .expect_err("missing thunk return types should fail");
    assert!(return_error.contains("does not know return type for `helper`"));
}

#[test]
fn direct_callable_objects_pin_defaults_capability_writebacks_and_task_handoff_abis() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/run-pass");

    let defaults_path = fixtures.join("function_values_storage_and_calls.au");
    let defaults_mir =
        lower_path_to_mir(&defaults_path).expect("function-value defaults should lower");
    let offset = defaults_mir
        .functions
        .iter()
        .find(|function| function.name == "offset")
        .expect("offset function should exist");
    let default_function = offset.params[0]
        .default_function
        .as_deref()
        .expect("offset should retain its default-expression thunk");
    let defaults_object = emit_unoptimized_object(&defaults_mir)
        .expect("function-value defaults should emit directly");
    let binder_references = object_function_referenced_symbols(
        &defaults_object,
        &mangle_default_binder_symbol("offset"),
    );
    assert!(
        binder_references
            .iter()
            .any(|symbol| symbol.contains(&mangle_symbol(default_function))),
        "the public function-value binder must call the selected function's concrete default thunk: {binder_references:?}"
    );
    assert!(
        binder_references
            .iter()
            .any(|symbol| symbol.contains("aura_direct_arg_buffer_store_owned")),
        "task-bound defaults must transfer their opaque handles into the scheduler-owned buffer: {binder_references:?}"
    );
    let default_main_references =
        object_function_referenced_symbols(&defaults_object, "aura_fn_main");
    assert!(
        default_main_references
            .iter()
            .any(|symbol| symbol.contains("aura_callable_")),
        "indirect calls dispatch through a callable descriptor: {default_main_references:?}"
    );
    for runtime_symbol in [
        "aura_direct_function_value",
        "aura_direct_function_bind_defaults",
        "aura_direct_function_call",
    ] {
        assert!(
            object_contains_symbol(&defaults_object, runtime_symbol),
            "the boundary adapters must keep `{runtime_symbol}`"
        );
    }
    for selected in ["first_default", "second_default"] {
        let function = defaults_mir
            .functions
            .iter()
            .find(|function| function.name == selected)
            .unwrap_or_else(|| panic!("selected default function `{selected}` should lower"));
        let default_function = function.params[0]
            .default_function
            .as_deref()
            .unwrap_or_else(|| panic!("`{selected}` should retain its concrete default thunk"));
        let binder_references = object_function_referenced_symbols(
            &defaults_object,
            &mangle_default_binder_symbol(selected),
        );
        assert!(
            binder_references
                .iter()
                .any(|symbol| symbol.contains(&mangle_symbol(default_function))),
            "the `{selected}` binder must call its own default thunk: {binder_references:?}"
        );
        for callable_symbol in [
            mangle_thunk_symbol(selected),
            mangle_default_binder_symbol(selected),
        ] {
            assert!(
                object_contains_symbol(&defaults_object, &callable_symbol),
                "runtime selection must retain `{callable_symbol}` for the shape's box adapter"
            );
        }
    }

    let capabilities_path = fixtures.join("function_value_inferred_capabilities.au");
    let capabilities_mir =
        lower_path_to_mir(&capabilities_path).expect("capability function values should lower");
    let capabilities_object = emit_unoptimized_object(&capabilities_mir)
        .expect("shared, mutable, and owned function values should emit directly");
    let mutable_thunk_references =
        object_function_referenced_symbols(&capabilities_object, &mangle_thunk_symbol("increment"));
    assert!(
        mutable_thunk_references
            .iter()
            .any(|symbol| symbol.contains(&mangle_symbol("increment"))),
        "the mutable-capability thunk must call its concrete function: {mutable_thunk_references:?}"
    );
    for runtime_symbol in [
        "aura_direct_instance_get_field",
        "aura_direct_instance_empty",
        "aura_direct_instance_set_field_owned",
    ] {
        assert!(
            mutable_thunk_references
                .iter()
                .any(|symbol| symbol.contains(runtime_symbol)),
            "the mutable plain-class parameter must round-trip through `{runtime_symbol}` for writeback: {mutable_thunk_references:?}"
        );
    }
    let owned_thunk_references =
        object_function_referenced_symbols(&capabilities_object, &mangle_thunk_symbol("take"));
    assert!(
        owned_thunk_references
            .iter()
            .any(|symbol| symbol.contains(&mangle_symbol("take"))),
        "the owned-capability thunk must call its concrete consuming function: {owned_thunk_references:?}"
    );

    let task_path = fixtures.join("function_values_task_targets.au");
    let task_mir = lower_path_to_mir(&task_path).expect("function-value task targets should lower");
    let task_object = emit_unoptimized_object(&task_mir)
        .expect("function-value task targets should emit directly");
    let task_main_references = object_function_referenced_symbols(&task_object, "aura_fn_main");
    for runtime_symbol in [
        "aura_direct_task_arg_buffer_guard",
        "aura_direct_function_bind_defaults",
        "aura_direct_task_arg_buffer_disarm",
        "aura_direct_start_task_function_with_frames",
    ] {
        assert!(
            task_main_references
                .iter()
                .any(|symbol| symbol.contains(runtime_symbol)),
            "callable Task lowering must use `{runtime_symbol}`: {task_main_references:?}"
        );
    }
    for selected in ["first_default", "second_default"] {
        let function = task_mir
            .functions
            .iter()
            .find(|function| function.name == selected)
            .unwrap_or_else(|| panic!("selected task function `{selected}` should lower"));
        let default_function = function.params[0]
            .default_function
            .as_deref()
            .unwrap_or_else(|| panic!("`{selected}` should retain its task default thunk"));
        let binder_references = object_function_referenced_symbols(
            &task_object,
            &mangle_default_binder_symbol(selected),
        );
        assert!(
            binder_references
                .iter()
                .any(|symbol| symbol.contains(&mangle_symbol(default_function))),
            "task selection must keep `{selected}` bound to its concrete default: {binder_references:?}"
        );
        for callable_symbol in [
            mangle_thunk_symbol(selected),
            mangle_default_binder_symbol(selected),
        ] {
            assert!(
            object_contains_symbol(&task_object, &callable_symbol),
            "the selected task callable must carry `{callable_symbol}` in the shape's box adapter"
        );
        }
    }
    for legacy_symbol in [
        "aura_direct_start_task_call_with_frames",
        "aura_direct_start_task_call",
    ] {
        assert!(
            !task_main_references
                .iter()
                .any(|symbol| symbol.ends_with(legacy_symbol)),
            "function-value task lowering must not fall back to `{legacy_symbol}`"
        );
    }
}

#[test]
fn direct_closure_objects_pin_environment_call_default_binding_and_task_abis() {
    let source = r#"
def main() -> int32:
    offset: int64 = 5
    factor: int64 = 2
    worker: def(int64) -> int64 = lambda value: value * factor + offset
    direct: int64 = worker(3)
    values: list[int64] = [1, 2]
    mapped: list[int64] = values.map(worker)
    with TaskGroup() as group:
        task: Task[int64] = group.start(worker, 7)
    return 0
"#;
    let module = lower_source_to_mir(source)
        .expect("a capturing closure should lower across call, callback, and task contexts");
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main should lower");
    let (lifted_name, captures) = main
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .find_map(|instruction| match instruction {
            Instruction::Assign {
                value:
                    Rvalue::Closure {
                        function, captures, ..
                    },
                ..
            } => Some((function, captures)),
            _ => None,
        })
        .expect("worker should construct a closure environment");
    assert_eq!(
        captures
            .iter()
            .map(|capture| capture.name.as_str())
            .collect::<Vec<_>>(),
        vec!["factor", "offset"],
        "native environment slots must retain MIR lexical-first-use order"
    );
    let lifted = module
        .functions
        .iter()
        .find(|function| function.name == *lifted_name)
        .expect("the closure body should be emitted as a direct-callable MIR function");
    assert_eq!(
        lifted
            .params
            .iter()
            .map(|param| param.name.as_str())
            .collect::<Vec<_>>(),
        vec!["factor", "offset", "value"],
        "the native thunk ABI must receive hidden captures before public parameters"
    );

    let object = emit_unoptimized_object(&module)
        .expect("closure calls, callbacks, and task handoff should emit");
    let main_references = object_function_referenced_symbols(&object, "aura_fn_main");
    for runtime_symbol in [
        "aura_direct_task_arg_buffer_guard",
        "aura_direct_task_arg_buffer_disarm",
        "aura_direct_start_task_function_with_frames",
    ] {
        assert!(
            main_references
                .iter()
                .any(|symbol| symbol.contains(runtime_symbol)),
            "closure lowering must use `{runtime_symbol}`: {main_references:?}"
        );
    }
    assert!(
        main_references
            .iter()
            .any(|symbol| symbol.contains("aura_callable_")),
        "closure construction names its shape descriptor: {main_references:?}"
    );
    for runtime_symbol in [
        "aura_direct_function_value",
        "aura_direct_arg_buffer_new",
        "aura_direct_arg_buffer_store_owned",
        "aura_direct_closure_value",
        "aura_direct_function_bind_defaults",
    ] {
        assert!(
            object_contains_symbol(&object, runtime_symbol),
            "the boundary adapters must keep `{runtime_symbol}`"
        );
    }
    for callable_symbol in [
        mangle_thunk_symbol(lifted_name),
        mangle_default_binder_symbol(lifted_name),
    ] {
        assert!(
            object_contains_symbol(&object, &callable_symbol),
            "the shape's box adapter must retain `{callable_symbol}`"
        );
    }
    let thunk_references =
        object_function_referenced_symbols(&object, &mangle_thunk_symbol(lifted_name));
    assert!(
        thunk_references
            .iter()
            .any(|symbol| symbol.contains(&mangle_symbol(lifted_name))),
        "the closure thunk must call its lifted implementation: {thunk_references:?}"
    );
    for legacy_symbol in [
        "aura_direct_function_thunk",
        "aura_direct_function_default_binder",
        "aura_direct_start_task_call_with_frames",
    ] {
        assert!(
            !main_references
                .iter()
                .any(|symbol| symbol.contains(legacy_symbol)),
            "closure objects must not regress to legacy ABI `{legacy_symbol}`"
        );
    }
}

#[test]
fn direct_closure_invalid_mir_reports_thunk_binder_call_and_task_contracts() {
    let source = r#"
def main() -> int32:
    offset: int32 = 5
    worker: def(int32) -> int32 = lambda value: value + offset
    direct: int32 = worker(1)
    with TaskGroup() as group:
        task: Task[int32] = group.start(worker, 2)
    return direct
"#;
    let module =
        lower_source_to_mir(source).expect("closure call and task source should lower to MIR");
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .cloned()
        .expect("main should lower");
    let lifted_name = main
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .find_map(|instruction| match instruction {
            Instruction::Assign {
                value: Rvalue::Closure { function, .. },
                ..
            } => Some(function.clone()),
            _ => None,
        })
        .expect("worker should retain its lifted implementation");

    let mut missing_thunk =
        NativeCodegen::new(&module, "/tmp/direct_closure_missing_thunk.au", source)
            .expect("codegen should initialize");
    missing_thunk.function_thunks.remove(&lifted_name);
    let thunk_error = missing_thunk
        .define_callable_shapes()
        .expect_err("closure construction must require the lifted thunk");
    assert!(
        thunk_error.contains(&format!("does not know function thunk for `{lifted_name}`")),
        "{thunk_error}"
    );

    let mut missing_binder =
        NativeCodegen::new(&module, "/tmp/direct_closure_missing_binder.au", source)
            .expect("codegen should initialize");
    missing_binder.function_default_binders.remove(&lifted_name);
    let binder_error = missing_binder
        .define_callable_shapes()
        .expect_err("closure construction must require the lifted default binder");
    assert!(
        binder_error.contains(&format!(
            "does not know function default binder for `{lifted_name}`"
        )),
        "{binder_error}"
    );

    let mut invalid_call = module.clone();
    let invalid_call_main = invalid_call
        .functions
        .iter_mut()
        .find(|function| function.name == "main")
        .expect("main should lower");
    let call_args = invalid_call_main
        .blocks
        .iter_mut()
        .flat_map(|block| block.instructions.iter_mut())
        .find_map(|instruction| match instruction {
            Instruction::Assign {
                value:
                    Rvalue::Call {
                        callee: CallTarget::Value(Operand::Place(place)),
                        args,
                        ..
                    },
                ..
            } if place == "worker" => Some(args),
            _ => None,
        })
        .expect("worker invocation should use value-call MIR");
    call_args[0].name = Some("missing".to_string());
    let call_error =
        emit_host_object(&invalid_call).expect_err("unknown closure call parameters must fail");
    assert!(
        call_error.contains("direct backend function value has no parameter named `missing`")
            || call_error.contains("unknown argument `missing`"),
        "{call_error}"
    );

    let mut invalid_task = module.clone();
    let invalid_task_main = invalid_task
        .functions
        .iter_mut()
        .find(|function| function.name == "main")
        .expect("main should lower");
    let task_args = invalid_task_main
        .blocks
        .iter_mut()
        .flat_map(|block| block.instructions.iter_mut())
        .find_map(|instruction| match instruction {
            Instruction::Assign {
                value: Rvalue::StartTask { function, args, .. },
                ..
            } if matches!(&*function, Operand::Place(place) if place == "worker") => Some(args),
            _ => None,
        })
        .expect("worker task start should retain function-value arguments");
    task_args[0].name = Some("missing".to_string());
    let task_error =
        emit_host_object(&invalid_task).expect_err("unknown closure task parameters must fail");
    assert!(
        task_error.contains("task function value has no parameter named `missing`")
            || task_error.contains("unknown argument `missing`"),
        "{task_error}"
    );

    let mut consuming_target = module.clone();
    let consuming_main = consuming_target
        .functions
        .iter_mut()
        .find(|function| function.name == "main")
        .expect("main should lower");
    let call_target = consuming_main
        .blocks
        .iter_mut()
        .flat_map(|block| block.instructions.iter_mut())
        .find_map(|instruction| match instruction {
            Instruction::Assign {
                value: Rvalue::Call { callee, .. },
                ..
            } if matches!(&*callee, CallTarget::Value(Operand::Place(place)) if place == "worker") => {
                Some(callee)
            }
            _ => None,
        })
        .expect("worker invocation should use a non-consuming call target");
    *call_target = CallTarget::Value(Operand::MovePlace("worker".to_string()));
    // `worker` is used again, so the ownership checker refuses the move at
    // the boundary first. The emitter's own check is what this case pins.
    let boundary_error = emit_host_object(&consuming_target)
        .expect_err("the boundary must refuse a use after the move");
    assert!(
        boundary_error.contains("uses `worker` after moving it"),
        "{boundary_error}"
    );
    crate::mir::ownership::strip_ownership(&mut consuming_target);
    let consuming_error = NativeCodegen::new(&consuming_target, "<aura>", "")
        .and_then(|codegen| codegen.emit())
        .expect_err("indirect call targets must not consume their callable place");
    assert!(
        consuming_error.contains(
            "only permits `MovePlace` in consuming contexts, not in an indirect-call target"
        ),
        "{consuming_error}"
    );
}

#[test]
fn native_codegen_function_value_signature_errors_are_precise() {
    fn contract(
        name: &str,
        passing: crate::ast::ReceiverKind,
    ) -> crate::sema::FunctionParamContract {
        crate::sema::FunctionParamContract {
            keyword_only: false,
            name: name.to_string(),
            ty: Type::named("int32"),
            passing,
            has_default: false,
        }
    }

    fn signature(param: crate::sema::FunctionParamContract) -> Type {
        Type::Function {
            params: vec![param],
            return_type: Box::new(Type::named("int32")),
        }
    }

    fn module(signature: Type, args: Vec<MirArg>) -> crate::mir::MirModule {
        crate::mir::MirModule {
            unions: Vec::new(),
            enums: Vec::new(),
            constants: Vec::new(),
            functions: vec![
                MirFunction {
                    name: "worker".to_string(),
                    module_name: "<test>".to_string(),
                    source_path: None,
                    span: Span::new(1, 1),
                    receiver: None,
                    params: vec![MirParam {
                        name: "value".to_string(),
                        passing: MirReceiverKind::Value,
                        ty: Type::named("int32"),
                        default_function: None,
                        keyword_only: false,
                    }],
                    local_types: vec![MirLocalType {
                        name: "value".to_string(),
                        ty: Type::named("int32"),
                    }],
                    return_type: Type::named("int32"),
                    entry: "entry".to_string(),
                    blocks: vec![BasicBlock {
                        label: "entry".to_string(),
                        instructions: Vec::new(),
                        terminator: Terminator::Return(Operand::Place("value".to_string())),
                    }],
                },
                MirFunction {
                    name: "main".to_string(),
                    module_name: "<test>".to_string(),
                    source_path: None,
                    span: Span::new(4, 1),
                    receiver: None,
                    params: Vec::new(),
                    local_types: vec![
                        MirLocalType {
                            name: "worker_value".to_string(),
                            ty: signature.clone(),
                        },
                        MirLocalType {
                            name: "call_result".to_string(),
                            ty: Type::named("int32"),
                        },
                    ],
                    return_type: Type::named("int32"),
                    entry: "entry".to_string(),
                    blocks: vec![BasicBlock {
                        label: "entry".to_string(),
                        instructions: vec![
                            Instruction::Assign {
                                target: "worker_value".to_string(),
                                value: Rvalue::Use(Operand::Function {
                                    name: "worker".to_string(),
                                    signature: Box::new(signature),
                                }),
                            },
                            Instruction::Assign {
                                target: "call_result".to_string(),
                                value: Rvalue::Call {
                                    callee: CallTarget::Value(Operand::Place(
                                        "worker_value".to_string(),
                                    )),
                                    args,
                                    type_args: Default::default(),
                                },
                            },
                        ],
                        terminator: Terminator::Return(Operand::Int(0)),
                    }],
                },
            ],
            classes: Vec::new(),
            trait_impls: Vec::new(),
            top_level: None,
            ownership: Default::default(),
            borrowing: Default::default(),
        }
    }

    let argument = |name: Option<&str>, writeback_place: Option<&str>| MirArg {
        name: name.map(str::to_string),
        value: Operand::Int(1),
        writeback_place: writeback_place.map(str::to_string),
    };
    let value_signature = || signature(contract("value", crate::ast::ReceiverKind::Value));
    let mutable_signature = || signature(contract("value", crate::ast::ReceiverKind::BorrowMut));
    let cases = vec![
        (
            "non-function signature",
            Type::named("int32"),
            vec![argument(None, None)],
            "direct backend expected an indirect function value",
        ),
        (
            "too many arguments",
            value_signature(),
            vec![argument(None, None), argument(None, None)],
            "direct backend expected at most 1 indirect-call arguments, found 2",
        ),
        (
            "unknown named argument",
            value_signature(),
            vec![argument(Some("missing"), None)],
            "direct backend function value has no parameter named `missing`",
        ),
        (
            "duplicate named argument",
            Type::Function {
                params: vec![
                    contract("value", crate::ast::ReceiverKind::Value),
                    contract("other", crate::ast::ReceiverKind::Value),
                ],
                return_type: Box::new(Type::named("int32")),
            },
            vec![argument(Some("value"), None), argument(Some("value"), None)],
            "direct backend received duplicate indirect-call arguments",
        ),
        (
            "mutable argument without writeback",
            mutable_signature(),
            vec![argument(None, None)],
            "direct backend indirect mutable argument 1 has no writeback place",
        ),
        (
            "writeback on a value argument",
            value_signature(),
            vec![argument(None, Some("value"))],
            "direct backend indirect argument 1 unexpectedly requests writeback",
        ),
    ];

    // The shared MIR validator may refuse these malformed modules before the
    // backend; each case accepts its validation-stage wording as well.
    let validator_wording = |label: &str| -> &'static [&'static str] {
        match label {
            "non-function signature" => &["has a non-function signature"],
            "too many arguments" => &["too many arguments"],
            "unknown named argument" => &["unknown argument `missing`", "no parameter named"],
            "duplicate named argument" => &[
                "duplicate",
                "too many arguments",
                "does not match declaration",
            ],
            "mutable argument without writeback" | "writeback on a value argument" => {
                &["writeback"]
            }
            _ => &[],
        }
    };
    for (label, signature, args, expected) in cases {
        let error = match emit_host_object(&module(signature, args)) {
            Ok(_) => panic!("{label} should be rejected"),
            Err(error) => error,
        };
        // Every case also authenticates the operand against `worker`'s
        // declaration, so a contract that disagrees with it is refused there.
        let operand_refused = error.contains("does not match declaration")
            || error.contains("has a non-function signature");
        assert!(
            error.contains(expected)
                || operand_refused
                || validator_wording(label)
                    .iter()
                    .any(|fragment| error.contains(fragment)),
            "{label} should report `{expected}`, found `{error}`"
        );
    }
}

#[test]
fn native_codegen_function_value_binding_skips_named_slots_for_later_positionals() {
    let source = r#"
def combine(first: int32, second: int32) -> int32:
    return first + second

def main() -> int32:
    callback = combine
    return callback(1, 2)
"#;
    let mut mir = lower_source_to_mir(source).expect("mixed binding source should lower");
    let main = mir
        .functions
        .iter_mut()
        .find(|function| function.name == "main")
        .expect("main should lower");
    let indirect_args = main
        .blocks
        .iter_mut()
        .flat_map(|block| block.instructions.iter_mut())
        .find_map(|instruction| match instruction {
            Instruction::Assign {
                value:
                    Rvalue::Call {
                        callee: CallTarget::Value(_),
                        args,
                        ..
                    },
                ..
            } => Some(args),
            _ => None,
        })
        .expect("main should contain the indirect callback call");
    assert_eq!(indirect_args.len(), 2);
    indirect_args[0].name = Some("first".to_string());

    let object = emit_unoptimized_object(&mir)
        .expect("a positional argument after a named slot should bind the next free parameter");
    let main_references = object_function_referenced_symbols(&object, "aura_fn_main");
    for runtime_symbol in ["aura_callable_"] {
        assert!(
            main_references
                .iter()
                .any(|symbol| symbol.contains(runtime_symbol)),
            "mixed binding must call through the callable descriptor `{runtime_symbol}`: {main_references:?}"
        );
    }
}

#[test]
fn native_codegen_default_binder_reports_method_and_metadata_failures() {
    let source = r#"
class Counter:
    value: int32

    def current(self) -> int32:
        return self.value

def target(value: int32 = 7) -> int32:
    return value

def main() -> int32:
    callback = target
    return callback()
"#;
    let mir = lower_source_to_mir(source).expect("defaulted function value should lower");
    let method = mir
        .functions
        .iter()
        .find(|function| function.receiver.is_some())
        .cloned()
        .expect("class method should lower");
    let target = mir
        .functions
        .iter()
        .find(|function| function.name == "target")
        .cloned()
        .expect("defaulted target should lower");
    let default_function = target.params[0]
        .default_function
        .as_deref()
        .expect("target should have a generated default thunk")
        .to_string();

    let mut method_codegen = NativeCodegen::new(&mir, "/tmp/function_default_binder.au", source)
        .expect("codegen should initialize");
    let method_error = method_codegen
        .define_function_default_binder(&method)
        .expect_err("methods are not first-class callable values");
    assert_eq!(
        method_error,
        "direct backend cannot build a function-value default binder for method `Counter.current`"
    );

    let mut missing_default_codegen =
        NativeCodegen::new(&mir, "/tmp/function_default_binder.au", source)
            .expect("codegen should initialize");
    missing_default_codegen.functions.remove(&default_function);
    let missing_default_error = missing_default_codegen
        .define_function_default_binder(&target)
        .expect_err("missing generated defaults should be diagnosed");
    assert_eq!(
        missing_default_error,
        format!("direct backend is missing default function `{default_function}` for `target`")
    );

    let mut missing_param_codegen =
        NativeCodegen::new(&mir, "/tmp/function_default_binder.au", source)
            .expect("codegen should initialize");
    missing_param_codegen.function_param_types.remove("target");
    let missing_param_error = missing_param_codegen
        .define_function_default_binder(&target)
        .expect_err("missing parameter ABI metadata should be diagnosed");
    assert_eq!(
        missing_param_error,
        "direct backend is missing parameter 1 metadata for `target`"
    );
}

#[test]
fn direct_backend_builtin_call_surface_compiles_across_success_and_error_matrix() {
    let success_source = r#"
import fs
import io
import net

def main() -> int32:
    print(7)
    print(3.5)
    print(true)
    print(None)
    text = "  Aura repo  "
    print(text)
    print("Aura " + "repo")
    print(f"value={text}")
    write_status = io.write("status")
    flushed = io.flush()
    line = io.read_line()
    entries = fs.read_dir(".")
    jobs = Queue[int32]()
    group = TaskGroup()
    ready = cancelled()
    sleep(0ms)
    value = abs(-7)
    floor = min(1, 2)
    ceil = max(1, 2)
    root = sqrt(9.0)
    rounded = round(2.5)
    pair = divmod(-7, 3)
    print(pair)
    parsed32 = parse_int32("7")
    parsed64 = parse_int64("7")
    parsedf = parse_float64("7.0")
    headers: dict[str, str] = {"X-Test": "ok"}
    body: list[uint8] = [1 as uint8, 2 as uint8]
    http_bytes = net.http_request_bytes_timeout("POST", "http://127.0.0.1/", body, headers, 5ms)
    values: list[int32] = list[int32]()
    names: set[str] = set[str]()
    counts: dict[str, int32] = dict[str, int32]()
    short = range(3)
    long = range(start=1, stop=4)
    print(ready)
    return (value + floor + ceil) as int32 + root as int32 + rounded as int32
"#;
    let success_mir =
        lower_source_to_mir(success_source).expect("builtin matrix source should lower");
    let object =
        emit_host_object(&success_mir).expect("builtin matrix source should emit direct code");
    assert!(!object.is_empty());

    let error_cases = [
        (
            "print missing arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("print".to_string()),
                args: vec![],
                type_args: Default::default(),
            }),
            "`Print.Value` in `main`: expected `print()` to receive `value`",
        ),
        (
            "channel extra arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("Queue".to_string()),
                args: vec![
                    MirArg {
                        name: None,
                        value: Operand::Int(1),
                        writeback_place: None,
                    },
                    MirArg {
                        name: None,
                        value: Operand::Int(2),
                        writeback_place: None,
                    },
                ],
                type_args: Default::default(),
            }),
            "expected `Queue()` to take at most 1 argument(s), found 2",
        ),
        (
            "channel bad named arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("Queue".to_string()),
                args: vec![MirArg {
                    name: Some("size".to_string()),
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
                type_args: Default::default(),
            }),
            "`Queue()` has no parameter `size`",
        ),
        (
            "tasks extra arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("TaskGroup".to_string()),
                args: vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
                type_args: Default::default(),
            }),
            "expected `TaskGroup()` to take no arguments",
        ),
        (
            "cancelled extra arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("cancelled".to_string()),
                args: vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
                type_args: Default::default(),
            }),
            "expected `cancelled()` to take no arguments",
        ),
        (
            "sleep missing arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("sleep".to_string()),
                args: vec![],
                type_args: Default::default(),
            }),
            "expected `sleep()` to receive `duration`",
        ),
        (
            "wait_any duplicate tasks",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("wait_any".to_string()),
                args: vec![
                    MirArg {
                        name: Some("tasks".to_string()),
                        value: Operand::Unit,
                        writeback_place: None,
                    },
                    MirArg {
                        name: Some("tasks".to_string()),
                        value: Operand::Unit,
                        writeback_place: None,
                    },
                ],
                type_args: Default::default(),
            }),
            "`wait_any()` receives `tasks` twice",
        ),
        (
            "wait_all duplicate timeout",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("wait_all".to_string()),
                args: vec![
                    MirArg {
                        name: Some("tasks".to_string()),
                        value: Operand::Unit,
                        writeback_place: None,
                    },
                    MirArg {
                        name: Some("timeout".to_string()),
                        value: Operand::Unit,
                        writeback_place: None,
                    },
                    MirArg {
                        name: Some("timeout".to_string()),
                        value: Operand::Unit,
                        writeback_place: None,
                    },
                ],
                type_args: Default::default(),
            }),
            "`wait_all()` receives `timeout` twice",
        ),
        (
            "wait_any unknown arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("wait_any".to_string()),
                args: vec![MirArg {
                    name: Some("jobs".to_string()),
                    value: Operand::Unit,
                    writeback_place: None,
                }],
                type_args: Default::default(),
            }),
            "`wait_any()` has no parameter `jobs`",
        ),
        (
            "abs missing arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("abs".to_string()),
                args: vec![],
                type_args: Default::default(),
            }),
            "expected `abs()` to receive `value`",
        ),
        (
            "parse_int32 missing arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("parse_int32".to_string()),
                args: vec![],
                type_args: Default::default(),
            }),
            "expected `parse_int32()` to receive `text`",
        ),
        (
            "min missing arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("min".to_string()),
                args: vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
                type_args: Default::default(),
            }),
            "expected `min()` to receive `right`",
        ),
        (
            "sqrt missing arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("sqrt".to_string()),
                args: vec![],
                type_args: Default::default(),
            }),
            "expected `sqrt()` to receive `value`",
        ),
        (
            "round missing arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("round".to_string()),
                args: vec![],
                type_args: Default::default(),
            }),
            "expected `round()` to receive `value`",
        ),
        (
            "divmod missing arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("divmod".to_string()),
                args: vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
                type_args: Default::default(),
            }),
            "expected `divmod()` to receive `right`",
        ),
        (
            "list extra arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("list".to_string()),
                args: vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
                type_args: Default::default(),
            }),
            "expected `list()` to take no arguments",
        ),
        (
            "range too many args",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("range".to_string()),
                args: vec![
                    MirArg {
                        name: None,
                        value: Operand::Int(1),
                        writeback_place: None,
                    },
                    MirArg {
                        name: None,
                        value: Operand::Int(2),
                        writeback_place: None,
                    },
                    MirArg {
                        name: None,
                        value: Operand::Int(3),
                        writeback_place: None,
                    },
                ],
                type_args: Default::default(),
            }),
            "expected `range()` to take at most 2 argument(s)",
        ),
        (
            "range too many mixed args",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("range".to_string()),
                args: vec![
                    MirArg {
                        name: Some("start".to_string()),
                        value: Operand::Int(0),
                        writeback_place: None,
                    },
                    MirArg {
                        name: None,
                        value: Operand::Int(1),
                        writeback_place: None,
                    },
                    MirArg {
                        name: None,
                        value: Operand::Int(2),
                        writeback_place: None,
                    },
                    MirArg {
                        name: None,
                        value: Operand::Int(3),
                        writeback_place: None,
                    },
                ],
                type_args: Default::default(),
            }),
            "expected `range()` to take at most 2 argument(s)",
        ),
        (
            "range bad named arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("range".to_string()),
                args: vec![
                    MirArg {
                        name: Some("middle".to_string()),
                        value: Operand::Int(1),
                        writeback_place: None,
                    },
                    MirArg {
                        name: Some("stop".to_string()),
                        value: Operand::Int(3),
                        writeback_place: None,
                    },
                ],
                type_args: Default::default(),
            }),
            "`range()` has no parameter `middle`",
        ),
        (
            "range missing stop",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("range".to_string()),
                args: vec![MirArg {
                    name: Some("start".to_string()),
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
                type_args: Default::default(),
            }),
            "expected `range()` to receive `stop`",
        ),
    ];

    for (label, module, expected) in error_cases {
        let error = emit_host_object(&module)
            .expect_err(&format!("{label} should be rejected by direct codegen"));
        assert!(
            error.contains(expected),
            "{label} reported `{error}` instead of containing `{expected}`"
        );
    }

    for (label, module) in [
        (
            "untyped dict constructor",
            module_with_main_call_result_type(
                Rvalue::Call {
                    callee: CallTarget::Name("dict".to_string()),
                    args: Vec::new(),
                    type_args: Default::default(),
                },
                Type::Named(
                    "dict".to_string(),
                    vec![Type::named("str"), Type::named("int32")],
                ),
            ),
        ),
        (
            "unary not on a string",
            module_with_main_call_result_type(
                Rvalue::Unary {
                    op: UnaryOp::Not,
                    value: Operand::String("truthy".to_string()),
                    span: Span::new(1, 1),
                    trap: crate::mir::ArithmeticTrap::Runtime,
                },
                Type::named("bool"),
            ),
        ),
        (
            "identity int32 cast",
            module_with_main_call(Rvalue::Cast {
                value: Operand::Int(7),
                ty: Type::named("int32"),
                span: Span::new(1, 1),
            }),
        ),
        (
            "wait_all without timeout",
            module_with_main_call_result_type(
                Rvalue::Call {
                    callee: CallTarget::Name("wait_all".to_string()),
                    args: vec![MirArg {
                        name: None,
                        value: Operand::Unit,
                        writeback_place: None,
                    }],
                    type_args: Default::default(),
                },
                Type::Named("WaitAll".to_string(), vec![Type::named("int64")]),
            ),
        ),
        (
            "wait_any positional timeout",
            module_with_main_call_result_type(
                Rvalue::Call {
                    callee: CallTarget::Name("wait_any".to_string()),
                    args: vec![
                        MirArg {
                            name: None,
                            value: Operand::Unit,
                            writeback_place: None,
                        },
                        MirArg {
                            name: None,
                            value: Operand::Duration(5),
                            writeback_place: None,
                        },
                    ],
                    type_args: Default::default(),
                },
                Type::Named("WaitAny".to_string(), vec![Type::named("int64")]),
            ),
        ),
        (
            "bool to int32 coercion",
            module_with_main_call(Rvalue::Use(Operand::Bool(true))),
        ),
        (
            "unit to int32 coercion",
            module_with_main_call(Rvalue::Use(Operand::Unit)),
        ),
        (
            "boolean and",
            module_with_main_call_result_type(
                Rvalue::Binary {
                    op: BinaryOp::And,
                    left: Operand::Bool(true),
                    right: Operand::Bool(false),
                    span: Span::new(1, 1),
                    trap: crate::mir::ArithmeticTrap::None,
                },
                Type::named("bool"),
            ),
        ),
        (
            "boolean or",
            module_with_main_call_result_type(
                Rvalue::Binary {
                    op: BinaryOp::Or,
                    left: Operand::Bool(true),
                    right: Operand::Bool(false),
                    span: Span::new(1, 1),
                    trap: crate::mir::ArithmeticTrap::None,
                },
                Type::named("bool"),
            ),
        ),
    ] {
        let object = emit_host_object(&module)
            .unwrap_or_else(|error| panic!("{label} should emit direct code: {error}"));
        assert!(!object.is_empty(), "{label} should produce object bytes");
    }
}

#[test]
fn direct_backend_match_and_branch_terminator_edges_cover_enum_and_opaque_paths() {
    let wildcard_match = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            source_path: None,
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: vec![MirLocalType {
                name: "maybe".to_string(),
                ty: crate::sema::lookup_type(Type::named("int32")),
            }],
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![
                BasicBlock {
                    label: "entry".to_string(),
                    instructions: vec![Instruction::Assign {
                        target: "maybe".to_string(),
                        value: Rvalue::EnumVariant {
                            enum_name: "Lookup".to_string(),
                            variant_name: "Found".to_string(),
                            payloads: vec![Operand::Int(1)],
                        },
                    }],
                    terminator: Terminator::Match {
                        scrutinee: Operand::Place("maybe".to_string()),
                        arms: vec![MirMatchArm {
                            enum_name: None,
                            variant_name: None,
                            wildcard: true,
                            label: "wild".to_string(),
                        }],
                        otherwise: "other".to_string(),
                    },
                },
                BasicBlock {
                    label: "wild".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Operand::Int(1)),
                },
                BasicBlock {
                    label: "other".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Operand::Int(0)),
                },
            ],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };
    assert!(!emit_host_object(&wildcard_match)
        .expect("wildcard enum matches should compile directly")
        .is_empty());

    let opaque_branch = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            source_path: None,
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: Vec::new(),
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![
                BasicBlock {
                    label: "entry".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Branch {
                        condition: Operand::String("truthy".to_string()),
                        then_label: "then".to_string(),
                        else_label: "else".to_string(),
                    },
                },
                BasicBlock {
                    label: "then".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Operand::Int(1)),
                },
                BasicBlock {
                    label: "else".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Operand::Int(0)),
                },
            ],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };
    assert!(!emit_host_object(&opaque_branch)
        .expect("opaque branch conditions should use runtime truthiness")
        .is_empty());

    let scalar_match = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            source_path: None,
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: Vec::new(),
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![
                BasicBlock {
                    label: "entry".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Match {
                        scrutinee: Operand::Int(1),
                        arms: Vec::new(),
                        otherwise: "other".to_string(),
                    },
                },
                BasicBlock {
                    label: "other".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Operand::Int(0)),
                },
            ],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };
    let scalar_error = emit_host_object(&scalar_match)
        .expect_err("scalar match scrutinees should be rejected by direct codegen");
    assert!(scalar_error.contains("expected enum matches to use opaque scrutinees"));

    let module_match = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            source_path: None,
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: vec![MirLocalType {
                name: "module_value".to_string(),
                ty: Type::Module("pkg.tools".to_string()),
            }],
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![
                BasicBlock {
                    label: "entry".to_string(),
                    instructions: vec![Instruction::Assign {
                        target: "module_value".to_string(),
                        value: Rvalue::Use(Operand::Unit),
                    }],
                    terminator: Terminator::Match {
                        scrutinee: Operand::Place("module_value".to_string()),
                        arms: Vec::new(),
                        otherwise: "other".to_string(),
                    },
                },
                BasicBlock {
                    label: "other".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Operand::Int(0)),
                },
            ],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };
    let module_error = emit_host_object(&module_match)
        .expect_err("non-named opaque match scrutinees should be rejected");
    assert!(module_error.contains("expected match scrutinee to carry an enum type name"));
}

#[test]
fn direct_backend_for_range_and_spawn_error_surface_reports_expected_diagnostics() {
    let invalid_for_range = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            source_path: None,
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: vec![MirLocalType {
                name: "item".to_string(),
                ty: Type::named("int32"),
            }],
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![
                BasicBlock {
                    label: "entry".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::ForRange {
                        binding: "item".to_string(),
                        iterable: Operand::Int(0),
                        body_label: "body".to_string(),
                        exit_label: "exit".to_string(),
                    },
                },
                BasicBlock {
                    label: "body".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Goto("exit".to_string()),
                },
                BasicBlock {
                    label: "exit".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Operand::Int(0)),
                },
            ],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };
    let for_range_error = emit_host_object(&invalid_for_range)
        .expect_err("non-place for-range iterables should be rejected");
    assert!(for_range_error.contains("requires `for range` iterables to live in a place"));

    let task_start_source = r#"
def worker(value: int32) -> int32:
    return value

def main() -> int32:
    with TaskGroup() as group:
        task = group.start(worker, 1)
        match task.result():
            case TaskResult.Ready(value):
                return value
            case TaskResult.Error(_message):
                return 0
            case TaskResult.TimedOut:
                return 0
            case TaskResult.Cancelled:
                return 0
"#;
    let mut task_start_mir =
        lower_source_to_mir(task_start_source).expect("task-start source should lower");
    let task_start_mir_snapshot = task_start_mir.clone();
    let main = task_start_mir
        .functions
        .iter_mut()
        .find(|function| function.name == "main")
        .expect("main function should exist");
    main.local_types.push(MirLocalType {
        name: "test_group".to_string(),
        ty: Type::named("TaskGroup"),
    });
    main.blocks = vec![BasicBlock {
        label: "entry".to_string(),
        instructions: vec![
            Instruction::Assign {
                target: "test_group".to_string(),
                value: Rvalue::Call {
                    callee: CallTarget::Name("TaskGroup".to_string()),
                    args: vec![],
                    type_args: Default::default(),
                },
            },
            Instruction::Assign {
                target: "test_task".to_string(),
                value: Rvalue::StartTask {
                    returns_handle: true,
                    result_is_copy: true,
                    stack_size: None,
                    task_group: Operand::Place("test_group".to_string()),
                    function: declared_function_operand(&task_start_mir_snapshot, "worker"),
                    args: vec![MirArg {
                        name: None,
                        value: Operand::Int(1),
                        writeback_place: None,
                    }],
                    span: Span::new(1, 1),
                },
            },
        ],
        terminator: Terminator::Return(Operand::Int(0)),
    }];
    main.local_types.push(MirLocalType {
        name: "test_task".to_string(),
        ty: Type::Named("Task".to_string(), vec![Type::named("int32")]),
    });
    // `define_function` skips the emitter entry that resolves named calls.
    crate::mir::resolve_runtime_ops(&mut task_start_mir);

    let main = task_start_mir
        .functions
        .iter()
        .find(|function| function.name == "main")
        .cloned()
        .expect("main function should exist");

    let mut missing_thunk_codegen = NativeCodegen::new(
        &task_start_mir,
        "/tmp/direct_task_start_missing_thunk.au",
        task_start_source,
    )
    .expect("codegen should initialize");
    missing_thunk_codegen.function_thunks.remove("worker");
    let missing_thunk_error = missing_thunk_codegen
        .define_callable_shapes()
        .expect_err("task start should reject missing thunks");
    assert!(missing_thunk_error.contains("does not know function thunk for `worker`"));

    let mut missing_binder_codegen = NativeCodegen::new(
        &task_start_mir,
        "/tmp/direct_task_start_missing_binder.au",
        task_start_source,
    )
    .expect("codegen should initialize");
    missing_binder_codegen
        .function_default_binders
        .remove("worker");
    let missing_binder_error = missing_binder_codegen
        .define_callable_shapes()
        .expect_err("task start should reject missing default binders");
    assert!(
        missing_binder_error.contains("does not know function default binder for `worker`"),
        "{missing_binder_error}"
    );

    let mut missing_return_codegen = NativeCodegen::new(
        &task_start_mir,
        "/tmp/direct_task_start_missing_return.au",
        task_start_source,
    )
    .expect("codegen should initialize");
    missing_return_codegen
        .function_return_types
        .remove("worker");
    missing_return_codegen
        .define_function(&main)
        .expect("task start repeatability is carried by MIR and needs no return-type side table");

    let mut borrowed_task_start_mir = task_start_mir.clone();
    let borrowed_main_mut = borrowed_task_start_mir
        .functions
        .iter_mut()
        .find(|function| function.name == "main")
        .expect("main function should exist");
    borrowed_main_mut.blocks[0].instructions[1] = Instruction::Assign {
        target: "test_task".to_string(),
        value: Rvalue::StartTask {
            returns_handle: true,
            result_is_copy: true,
            stack_size: None,
            task_group: Operand::Place("test_group".to_string()),
            function: declared_function_operand(&task_start_mir_snapshot, "worker"),
            args: vec![MirArg {
                name: None,
                value: Operand::Int(1),
                writeback_place: Some("value".to_string()),
            }],
            span: Span::new(1, 1),
        },
    };
    let borrowed_main = borrowed_task_start_mir
        .functions
        .iter()
        .find(|function| function.name == "main")
        .cloned()
        .expect("main function should exist");
    // Public MIR validation rejects a writeback into a borrowed parameter
    // before codegen; the backend's own diagnostic remains for MIR that
    // reaches it through the crate-internal path.
    match NativeCodegen::new(
        &borrowed_task_start_mir,
        "/tmp/direct_task_start_borrowed_arg.au",
        task_start_source,
    ) {
        Err(error) => assert!(
            error.contains("supplies writeback for non-mutable parameter"),
            "unexpected validation error: {error}"
        ),
        Ok(mut borrowed_task_start_codegen) => {
            let borrowed_error = borrowed_task_start_codegen
                .define_function(&borrowed_main)
                .expect_err("task start should reject borrowed arguments");
            assert!(borrowed_error.contains("does not yet support borrowed task-start arguments"));
        }
    }

    let named_worker = declared_function_operand(&task_start_mir_snapshot, "worker");
    let invalid_bindings = [
        (
            vec![MirArg {
                name: Some("missing".to_string()),
                value: Operand::Int(1),
                writeback_place: None,
            }],
            "task function value has no parameter named `missing`",
        ),
        (
            vec![
                MirArg {
                    name: Some("value".to_string()),
                    value: Operand::Int(1),
                    writeback_place: None,
                },
                MirArg {
                    name: None,
                    value: Operand::Int(2),
                    writeback_place: None,
                },
            ],
            "duplicate task function-value argument",
        ),
    ];
    for (args, expected) in invalid_bindings {
        let mut invalid_mir = task_start_mir.clone();
        let invalid_main = invalid_mir
            .functions
            .iter_mut()
            .find(|function| function.name == "main")
            .expect("main function should exist");
        invalid_main.blocks[0].instructions[1] = Instruction::Assign {
            target: "test_task".to_string(),
            value: Rvalue::StartTask {
                returns_handle: true,
                result_is_copy: true,
                stack_size: None,
                task_group: Operand::Place("test_group".to_string()),
                function: named_worker.clone(),
                args,
                span: Span::new(1, 1),
            },
        };
        let invalid_main = invalid_main.clone();
        let error = match NativeCodegen::new(
            &invalid_mir,
            "/tmp/direct_task_start_invalid_binding.au",
            task_start_source,
        ) {
            Err(error) => error,
            Ok(mut codegen) => codegen
                .define_function(&invalid_main)
                .expect_err("invalid task callable bindings should fail"),
        };
        // Public validation may reject the binding first with its own
        // wording; either stage must refuse the malformed task start.
        let accepted = if expected.contains("missing") {
            ["missing", "no parameter named"].as_slice()
        } else {
            ["duplicate", "too many arguments"].as_slice()
        };
        assert!(
            error.contains(expected) || accepted.iter().any(|fragment| error.contains(fragment)),
            "expected `{expected}`, got `{error}`"
        );
    }
}

#[test]
fn direct_backend_emits_object_for_member_call_surface_matrix() {
    let source = r#"
trait Named:
    def tag(self) -> str

impl Named for int32:
    def tag(self) -> str:
        return "number"

class Counter:
    value: int32

    def read(self) -> int32:
        return self.value

    def bump(mut self):
        self.value += 1

def worker(value: int32) -> int32:
    return value + 1

def main() -> int32:
    text = "  Aura repo  "
    cloned = text.clone()
    length = text.len()
    byte_length = text.byte_len()
    has_repo = text.contains("repo")
    starts = text.starts_with("  Au")
    ends = text.ends_with("  ")
    parts = text.split(" ")
    replaced = text.replace("repo", "lang")
    lowered = text.to_lower()
    uppered = text.to_upper()
    stripped_prefix = text.strip_prefix("  ")
    stripped_suffix = text.strip_suffix("  ")
    trimmed = text.trim()
    joined = ", ".join(parts)

    value: int32 = 7
    label = value.to_string()
    tagged = value.tag()
    root = 9.0.sqrt()

    mut values: list[int32] = [1, 2, 3]
    empty = values.is_empty()
    length2 = values.len()
    values.append(4)
    popped = values.pop()
    first = values.get(0)
    direct = values[0]
    previous = values.set(0, 9)
    removed = values.remove(0)
    swapped = values.swap(0, 1)
    contains = values.contains(2)
    inserted = values.insert(0, 5)
    values.reverse()
    other_values: list[int32] = [8, 9]
    values.extend(other_values)
    values.clear()

    mut counts: dict[str, int32] = {"a": 1, "b": 2}
    map_empty = counts.is_empty()
    map_len = counts.len()
    current = counts.get("a")
    direct_count = counts["a"]
    previous_count = counts["a"]
    counts["a"] = 3
    removed_count = counts.remove("b")
    has_key = "a" in counts
    keys = counts.keys()
    vals = counts.values()
    items = counts.items()
    counts.update({"c": 4})
    counts.clear()

    mut names = set[str]()
    set_empty = names.is_empty()
    names.add("aura")
    names.add("repo")
    set_len = names.len()
    has_name = "aura" in names
    removed_name = names.remove("repo")

    jobs = Queue[int32]()
    send_result = jobs.put(1)
    recv_result = jobs.get()
    jobs.close()

    with TaskGroup() as group:
        group.cancel()
        task = group.start(worker, value=1)
        joined_task = task.result()
        print(joined_task)

    mut counter = Counter(value=1)
    current_value = counter.read()
    counter.bump()
    latest = counter.value

    print(cloned)
    print(length)
    print(byte_length)
    print(has_repo)
    print(starts)
    print(ends)
    print(replaced)
    print(lowered)
    print(uppered)
    print(joined)
    print(label)
    print(tagged)
    print(root)
    print(empty)
    print(length2)
    print(contains)
    print(inserted)
    print(map_empty)
    print(map_len)
    print(has_key)
    print(set_empty)
    print(set_len)
    print(has_name)
    print(current_value)
    print(latest)
    return direct + direct_count
"#;
    let mir = lower_source_to_mir(source).expect("member-call matrix source should lower");
    let main = mir
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("member-call matrix should contain main");
    for local_name in ["length", "byte_length", "length2", "map_len", "set_len"] {
        let local_type = main
            .local_types
            .iter()
            .find(|local| local.name == local_name)
            .map(|local| local.ty.clone());
        assert_eq!(
            local_type,
            Some(Type::named("int64")),
            "`{local_name}` should infer as exactly int64"
        );
    }
    let object = emit_host_object(&mir).expect("member-call matrix should emit direct code");
    assert!(!object.is_empty());
    let referenced = object_referenced_symbols(&object);
    for required in [
        "aura_direct_string_len",
        "aura_direct_string_byte_len",
        "aura_direct_vec_len",
        "aura_direct_map_len",
        "aura_direct_set_len",
    ] {
        assert!(
            referenced.iter().any(|symbol| symbol.contains(required)),
            "member-length matrix should reference `{required}`: {referenced:?}"
        );
    }
}

#[test]
fn direct_string_to_bytes_routes_through_the_registered_host_builtin() {
    let source = r#"
def main() -> int32:
    payload = "café".to_bytes()
    print(payload)
    return 0
"#;
    let mir = lower_source_to_mir(source).expect("str.to_bytes source should lower to MIR");
    assert!(
        mir.functions
            .iter()
            .flat_map(|function| &function.blocks)
            .any(
                |block| block.instructions.iter().any(|instruction| matches!(
                    instruction,
                    Instruction::Assign {
                        value: Rvalue::Call {
                            callee: CallTarget::Runtime {
                                op, receiver: None, ..
                            },
                            ..
                        },
                        ..
                    } if op.name() == "str.to_bytes"
                ))
            ),
        "public str.to_bytes syntax must canonicalize to the registered host-builtin call"
    );
    let bytes = emit_host_object(&mir).expect("str.to_bytes should emit direct native code");
    let referenced = object_referenced_symbols(&bytes);
    assert!(
        referenced
            .iter()
            .any(|symbol| symbol.contains("aura_host_str_to_bytes")),
        "str.to_bytes must call its own host builtin entry: {referenced:?}"
    );

    let object = object::File::parse(bytes.as_slice())
        .expect("str.to_bytes direct output should be a readable host object");
    assert!(
        !object
            .sections()
            .filter_map(|section| section.data().ok())
            .any(|data| data
                .windows(b"str.to_bytes".len())
                .any(|window| window == b"str.to_bytes")),
        "direct code must not carry the builtin name for a runtime lookup"
    );
}

#[test]
fn direct_discarded_with_capacity_calls_preserve_each_collection_constructor() {
    let source = r#"
def main():
    list[int64].with_capacity(2)
    set[str].with_capacity(2)
    dict[str, int64].with_capacity(2)
    print("complete")
"#;
    let mir = lower_source_to_mir(source)
        .expect("discarded collection-capacity calls should lower to MIR");
    let object = emit_host_object(&mir)
        .expect("discarded collection-capacity calls should emit direct native code");
    let referenced = object_referenced_symbols(&object);
    for required in [
        "aura_direct_vec_empty",
        "aura_direct_set_empty",
        "aura_direct_map_empty",
        "aura_direct_collection_operation",
    ] {
        assert!(
            referenced.iter().any(|symbol| symbol.contains(required)),
            "discarded with_capacity calls must retain `{required}`: {referenced:?}"
        );
    }
}

#[test]
fn direct_member_lengths_do_not_emit_implicit_int32_range_checks() {
    let baseline =
        module_with_main_call_result_type(Rvalue::Use(Operand::Int(0)), Type::named("int64"));
    let baseline_object =
        emit_host_object(&baseline).expect("matched int64-result baseline should emit directly");
    let baseline_overflow_references =
        object_referenced_symbol_occurrences(&baseline_object, "aura_direct_fail_int32_overflow");

    let cases = vec![
        (
            "str.len",
            module_with_main_member_call_result_type(
                "text",
                Type::named("str"),
                Rvalue::Use(Operand::String("Aura".to_string())),
                Type::named("int64"),
                "len",
                Vec::new(),
            ),
            "aura_direct_string_len",
        ),
        (
            "str.byte_len",
            module_with_main_member_call_result_type(
                "text",
                Type::named("str"),
                Rvalue::Use(Operand::String("é🎉e\u{301}".to_string())),
                Type::named("int64"),
                "byte_len",
                Vec::new(),
            ),
            "aura_direct_string_byte_len",
        ),
        (
            "list.len",
            module_with_main_member_call_result_type(
                "values",
                Type::Named("list".to_string(), vec![Type::named("str")]),
                Rvalue::VecLiteral {
                    element_type: Type::named("str"),
                    elements: vec![Operand::String("value".to_string())],
                },
                Type::named("int64"),
                "len",
                Vec::new(),
            ),
            "aura_direct_vec_len",
        ),
        (
            "dict.len",
            module_with_main_member_call_result_type(
                "counts",
                Type::Named(
                    "dict".to_string(),
                    vec![Type::named("str"), Type::named("str")],
                ),
                Rvalue::MapLiteral {
                    key_type: Type::named("str"),
                    value_type: Type::named("str"),
                    entries: vec![MirMapEntry {
                        key: Operand::String("key".to_string()),
                        value: Operand::String("value".to_string()),
                    }],
                },
                Type::named("int64"),
                "len",
                Vec::new(),
            ),
            "aura_direct_map_len",
        ),
        (
            "set.len",
            module_with_main_member_call_result_type(
                "values",
                Type::Named("set".to_string(), vec![Type::named("str")]),
                Rvalue::SetLiteral {
                    element_type: Type::named("str"),
                    elements: vec![Operand::String("value".to_string())],
                },
                Type::named("int64"),
                "len",
                Vec::new(),
            ),
            "aura_direct_set_len",
        ),
    ];

    for (label, module, required_symbol) in cases {
        let object = emit_host_object(&module)
            .unwrap_or_else(|error| panic!("{label} should emit directly: {error}"));
        let referenced = object_referenced_symbols(&object);
        assert!(
            referenced
                .iter()
                .any(|symbol| symbol.contains(required_symbol)),
            "{label} should reference `{required_symbol}`: {referenced:?}"
        );
        assert_eq!(
            object_referenced_symbol_occurrences(&object, "aura_direct_fail_int32_overflow"),
            baseline_overflow_references,
            "{label} returns int64 and must not add an implicit int32 range check"
        );
    }
}

#[test]
fn direct_member_length_explicit_int32_cast_keeps_checked_narrowing() {
    let source = r#"
def main() -> int32:
    wide = "Aura".len()
    narrow = wide as int32
    print(narrow)
    return 0
"#;

    let mir = lower_source_to_mir(source).expect("explicit member-length narrowing should lower");
    let main = mir
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("member-length narrowing should contain main");
    for (local_name, expected) in [
        ("wide", Type::named("int64")),
        ("narrow", Type::named("int32")),
    ] {
        let local_type = main
            .local_types
            .iter()
            .find(|local| local.name == local_name)
            .map(|local| local.ty.clone());
        assert_eq!(
            local_type,
            Some(expected),
            "unexpected inferred type for `{local_name}`"
        );
    }

    let object =
        emit_host_object(&mir).expect("explicit member-length narrowing should emit directly");
    let referenced = object_referenced_symbols(&object);
    assert!(
        referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_string_len")),
        "member-length narrowing should call the str length runtime: {referenced:?}"
    );
    assert!(
        referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_fail_int32_overflow")),
        "an explicit int64-to-int32 member-length cast must retain the checked narrowing guard: {referenced:?}"
    );
}

#[test]
fn direct_int32_range_check_still_traps_exactly_at_the_boundary() {
    // The narrow range check is on the hot path of every `int32` operation, so
    // it is written as one biased unsigned comparison rather than a two-sided
    // signed pair. These cases pin that the cheaper form keeps the same exact
    // boundary: the extremes are representable and one step past either end
    // traps.
    let accepted = crate::run_source(
        r#"
def main() -> int32:
    mut high: int32 = 2147483646
    high += 1
    print(high)
    mut low: int32 = -2147483647
    low -= 1
    print(low)
    return 0
"#,
    )
    .expect("the int32 extremes should be representable");
    assert_eq!(accepted.stdout, "2147483647\n-2147483648\n");

    for (source, value) in [
        (
            "def main() -> int32:\n    mut high: int32 = 2147483647\n    high += 1\n    return 0\n",
            "2147483648",
        ),
        (
            "def main() -> int32:\n    mut low: int32 = -2147483648\n    low -= 1\n    return 0\n",
            "-2147483649",
        ),
    ] {
        let trapped = crate::run_source(source).expect_err("one step past the range should trap");
        assert_eq!(trapped.code, "AU4002", "{source}");
        assert_eq!(
            trapped.message,
            format!("integer value `{value}` does not fit in `int32`"),
            "{source}"
        );
    }

    // The same program still lowers and emits through the direct backend.
    let module = lower_source_to_mir(
        "def main() -> int32:\n    mut index: int32 = 0\n    while index < 4:\n        index += 1\n    print(index)\n    return 0\n",
    )
    .expect("an int32 loop should lower");
    let object = emit_host_object(&module).expect("an int32 loop should emit direct code");
    assert!(!object.is_empty());
}

#[test]
fn direct_backend_prefers_builtin_handle_member_if_collision_reaches_mir() {
    let source = r#"
trait Probe:
    def probe(self) -> int64

impl[T] Probe for Queue[T]:
    def probe(self) -> int64:
        return 99

def main() -> int32:
    queue = Queue[int32]()
    received = queue.get()
    return 0
"#;

    let mut mir = lower_source_to_mir(source).expect("noncolliding source should lower");
    let trait_symbol = {
        let method = mir
            .trait_impls
            .iter_mut()
            .find(|trait_impl| trait_impl.trait_name == "Probe")
            .and_then(|trait_impl| {
                trait_impl
                    .methods
                    .iter_mut()
                    .find(|method| method.name == "probe")
            })
            .expect("Probe.probe should lower");
        let symbol = mangle_symbol(&method.function_name);
        method.name = "get".to_string();
        symbol
    };

    let object =
        emit_host_object(&mir).expect("builtin dispatch should survive malformed internal MIR");
    let referenced = object_referenced_symbols(&object);

    assert!(
        referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_channel_recv")),
        "Queue.get should reference the builtin receive helper: {referenced:?}"
    );
    assert!(
        !referenced
            .iter()
            .any(|symbol| symbol.contains(&trait_symbol)),
        "Queue.get must not dispatch to the colliding trait body: {referenced:?}"
    );
}

#[test]
fn direct_backend_member_call_error_surface_reports_expected_diagnostics() {
    let one_arg = vec![MirArg {
        name: None,
        value: Operand::Int(1),
        writeback_place: None,
    }];
    let two_args = vec![
        MirArg {
            name: None,
            value: Operand::Int(1),
            writeback_place: None,
        },
        MirArg {
            name: None,
            value: Operand::Int(2),
            writeback_place: None,
        },
    ];
    let string_object = || Rvalue::Use(Operand::String("aura".to_string()));
    let vec_object = || Rvalue::VecLiteral {
        element_type: Type::named("int32"),
        elements: vec![Operand::Int(1), Operand::Int(2)],
    };
    let map_object = || Rvalue::MapLiteral {
        key_type: Type::named("str"),
        value_type: Type::named("int32"),
        entries: vec![MirMapEntry {
            key: Operand::String("a".to_string()),
            value: Operand::Int(1),
        }],
    };
    let set_object = || Rvalue::SetLiteral {
        element_type: Type::named("str"),
        elements: vec![Operand::String("aura".to_string())],
    };
    let channel_object = || Rvalue::Call {
        callee: CallTarget::Name("Queue".to_string()),
        args: vec![],
        type_args: Default::default(),
    };

    let error_cases = [
        (
            "float sqrt extra arg",
            module_with_main_member_call(
                "value",
                Type::named("float64"),
                Rvalue::Use(Operand::Float(9.0)),
                "sqrt",
                one_arg.clone(),
            ),
            "expected `sqrt()` to take no arguments",
        ),
        (
            "string clone extra arg",
            module_with_main_member_call(
                "text",
                Type::named("str"),
                string_object(),
                "clone",
                one_arg.clone(),
            ),
            "expected `clone()` to take no arguments",
        ),
        (
            "scalar to_string extra arg",
            module_with_main_member_call(
                "value",
                Type::named("int32"),
                Rvalue::Use(Operand::Int(7)),
                "to_string",
                one_arg.clone(),
            ),
            "expected `to_string()` to take no arguments",
        ),
        (
            "string len extra arg",
            module_with_main_member_call(
                "text",
                Type::named("str"),
                string_object(),
                "len",
                one_arg.clone(),
            ),
            "expected `len()` to take no arguments",
        ),
        (
            "string contains missing arg",
            module_with_main_member_call(
                "text",
                Type::named("str"),
                string_object(),
                "contains",
                vec![],
            ),
            "expected `contains()` to receive `text`",
        ),
        (
            "vec len extra arg",
            module_with_main_member_call(
                "values",
                Type::Named("list".to_string(), vec![Type::named("int32")]),
                vec_object(),
                "len",
                one_arg.clone(),
            ),
            "expected `len()` to take no arguments",
        ),
        (
            "list append missing arg",
            module_with_main_member_call(
                "values",
                Type::Named("list".to_string(), vec![Type::named("int32")]),
                vec_object(),
                "append",
                vec![],
            ),
            "expected `append()` to receive `value`",
        ),
        (
            "vec clear extra arg",
            module_with_main_member_call(
                "values",
                Type::Named("list".to_string(), vec![Type::named("int32")]),
                vec_object(),
                "clear",
                one_arg.clone(),
            ),
            "expected `clear()` to take no arguments",
        ),
        (
            "map len extra arg",
            module_with_main_member_call(
                "counts",
                Type::Named(
                    "dict".to_string(),
                    vec![Type::named("str"), Type::named("int32")],
                ),
                map_object(),
                "len",
                one_arg.clone(),
            ),
            "expected `len()` to take no arguments",
        ),
        (
            "map set missing arg",
            module_with_main_member_call(
                "counts",
                Type::Named(
                    "dict".to_string(),
                    vec![Type::named("str"), Type::named("int32")],
                ),
                map_object(),
                "set",
                one_arg.clone(),
            ),
            "expected `set()` to receive `value`",
        ),
        (
            "set contains missing arg",
            module_with_main_member_call(
                "names",
                Type::Named("set".to_string(), vec![Type::named("str")]),
                set_object(),
                "contains",
                vec![],
            ),
            "expected `contains()` to receive `value`",
        ),
        (
            "queue get extra arg",
            module_with_main_member_call(
                "jobs",
                Type::Named("Queue".to_string(), vec![Type::named("int32")]),
                channel_object(),
                "get",
                two_args.clone(),
            ),
            "expected `get()` to take at most 1 argument(s), found 2",
        ),
        (
            "queue put missing arg",
            module_with_main_member_call(
                "jobs",
                Type::Named("Queue".to_string(), vec![Type::named("int32")]),
                channel_object(),
                "put",
                vec![],
            ),
            "expected `put()` to receive `value`",
        ),
        (
            "vec swap missing arg",
            module_with_main_member_call(
                "values",
                Type::Named("list".to_string(), vec![Type::named("int32")]),
                vec_object(),
                "swap",
                one_arg.clone(),
            ),
            "expected `swap()` to receive `second`",
        ),
        (
            "dict update missing arg",
            module_with_main_member_call(
                "counts",
                Type::Named(
                    "dict".to_string(),
                    vec![Type::named("str"), Type::named("int32")],
                ),
                map_object(),
                "update",
                vec![],
            ),
            "expected `update()` to receive `other`",
        ),
        (
            "set remove missing arg",
            module_with_main_member_call(
                "names",
                Type::Named("set".to_string(), vec![Type::named("str")]),
                set_object(),
                "remove",
                vec![],
            ),
            "expected `remove()` to receive `value`",
        ),
        (
            "unknown runtime member",
            module_with_main_member_call(
                "text",
                Type::named("str"),
                string_object(),
                "missing",
                vec![],
            ),
            "runtime has no member `str.missing`",
        ),
        (
            "unknown scalar member",
            module_with_main_member_call(
                "value",
                Type::named("int32"),
                Rvalue::Use(Operand::Int(7)),
                "missing",
                vec![],
            ),
            "`int32` has no method `missing`",
        ),
        (
            "string replace missing arg",
            module_with_main_member_call(
                "text",
                Type::named("str"),
                string_object(),
                "replace",
                one_arg.clone(),
            ),
            "expected `replace()` to receive `to`",
        ),
        (
            "vec insert missing arg",
            module_with_main_member_call(
                "values",
                Type::Named("list".to_string(), vec![Type::named("int32")]),
                vec_object(),
                "insert",
                one_arg.clone(),
            ),
            "expected `insert()` to receive `value`",
        ),
        (
            "map items extra arg",
            module_with_main_member_call(
                "counts",
                Type::Named(
                    "dict".to_string(),
                    vec![Type::named("str"), Type::named("int32")],
                ),
                map_object(),
                "items",
                one_arg.clone(),
            ),
            "expected `items()` to take no arguments",
        ),
        (
            "set len extra arg",
            module_with_main_member_call(
                "names",
                Type::Named("set".to_string(), vec![Type::named("str")]),
                set_object(),
                "len",
                one_arg,
            ),
            "expected `len()` to take no arguments",
        ),
        (
            "string join missing arg",
            module_with_main_member_call(
                "text",
                Type::named("str"),
                string_object(),
                "join",
                vec![],
            ),
            "expected `join()` to receive `parts`",
        ),
        (
            "string trim extra arg",
            module_with_main_member_call(
                "text",
                Type::named("str"),
                string_object(),
                "trim",
                two_args[..1].to_vec(),
            ),
            "expected `trim()` to take no arguments",
        ),
    ];

    for (label, module, expected) in error_cases {
        let error = emit_host_object(&module)
            .expect_err(&format!("{label} should be rejected by direct codegen"));
        assert!(
            error.contains(expected),
            "{label} reported `{error}` instead of containing `{expected}`"
        );
    }
}

#[test]
fn direct_backend_operand_and_construct_error_surface_reports_expected_diagnostics() {
    let large_int_module = module_with_main_call(Rvalue::Use(Operand::Int((i64::MAX as u128) + 1)));
    let large_int_object =
        emit_host_object(&large_int_module).expect("large integer operands should box");
    assert!(!large_int_object.is_empty());

    let large_duration_module =
        module_with_main_call(Rvalue::Use(Operand::Duration((i64::MAX as i128) + 1)));
    let large_duration_object = emit_host_object(&large_duration_module)
        .expect("duration literals beyond i64 should use the two-limb runtime ABI");
    assert!(
        object_referenced_symbols(&large_duration_object)
            .iter()
            .any(|symbol| symbol.contains("aura_direct_duration_literal")),
        "direct duration literals should remain opaque after runtime construction"
    );

    let missing_place_module =
        module_with_main_call(Rvalue::Use(Operand::Place("missing".to_string())));
    let missing_place_error =
        emit_host_object(&missing_place_module).expect_err("unknown locals should be rejected");
    assert!(missing_place_error.contains("does not know local `missing`"));

    let empty_place_module = module_with_main_call(Rvalue::Use(Operand::Place(String::new())));
    let empty_place_error =
        emit_host_object(&empty_place_module).expect_err("empty places should be rejected");
    assert!(
        empty_place_error.contains("empty MIR place"),
        "unexpected empty-place diagnostic: {empty_place_error}"
    );

    let stray_pop_cleanup_module = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            source_path: None,
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: Vec::new(),
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: vec![Instruction::PopCleanup {
                    place: "ghost".to_string(),
                    cancel_before_cleanup: false,
                    run: true,
                }],
                terminator: Terminator::Return(Operand::Int(0)),
            }],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };
    let stray_pop_cleanup_error = emit_host_object(&stray_pop_cleanup_module)
        .expect_err("unmatched cleanup pops should report the missing cleanup registration");
    assert!(stray_pop_cleanup_error.contains("does not know cleanup registration for `ghost`"));

    let pair_class = crate::mir::MirClass {
        name: "Pair".to_string(),
        type_params: Vec::new(),
        fields: vec![
            crate::mir::MirClassField {
                name: "left".to_string(),
                ty: Type::named("int32"),
            },
            crate::mir::MirClassField {
                name: "right".to_string(),
                ty: Type::named("int32"),
            },
        ],
        methods: Vec::new(),
        copy: false,
    };
    let missing_field_module = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            source_path: None,
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: Vec::new(),
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: vec![Instruction::Assign {
                    target: "%t0".to_string(),
                    value: Rvalue::Construct {
                        class_name: "Pair".to_string(),
                        fields: vec![crate::mir::MirFieldInit {
                            name: "left".to_string(),
                            value: Operand::Int(1),
                        }],
                    },
                }],
                terminator: Terminator::Return(Operand::Int(0)),
            }],
        }],
        classes: vec![pair_class.clone()],
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };
    let missing_field_error = emit_host_object(&missing_field_module)
        .expect_err("plain-class construction should require all fields");
    assert!(
        missing_field_error
            .contains("invalid MIR construction of `Pair` in `main` omits fields outside an initializer's object"),
        "{missing_field_error}"
    );

    let non_class_construct_module = module_with_main_call(Rvalue::Construct {
        class_name: "int32".to_string(),
        fields: Vec::new(),
    });
    let non_class_construct_error = emit_host_object(&non_class_construct_module)
        .expect_err("constructing scalar types should be rejected");
    assert!(non_class_construct_error.contains("could not construct non-class type `int32`"));

    let plain_cast_target_module = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            source_path: None,
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: vec![MirLocalType {
                name: "%t0".to_string(),
                ty: Type::named("Pair"),
            }],
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: vec![Instruction::Assign {
                    target: "%t0".to_string(),
                    value: Rvalue::Cast {
                        value: Operand::Int(1),
                        ty: Type::named("Pair"),
                        span: Span::new(1, 1),
                    },
                }],
                terminator: Terminator::Return(Operand::Int(0)),
            }],
        }],
        classes: vec![pair_class.clone()],
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };
    let plain_cast_target_error = emit_host_object(&plain_cast_target_module)
        .expect_err("casts to plain classes should be rejected before code emission");
    assert!(plain_cast_target_error
        .contains("direct backend only supports numeric casts, found target `Pair`"));

    let plain_cast_source_module = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            source_path: None,
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: vec![
                MirLocalType {
                    name: "pair".to_string(),
                    ty: Type::named("Pair"),
                },
                MirLocalType {
                    name: "%t0".to_string(),
                    ty: Type::named("int32"),
                },
            ],
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: vec![
                    Instruction::Assign {
                        target: "pair".to_string(),
                        value: Rvalue::Construct {
                            class_name: "Pair".to_string(),
                            fields: vec![
                                crate::mir::MirFieldInit {
                                    name: "left".to_string(),
                                    value: Operand::Int(1),
                                },
                                crate::mir::MirFieldInit {
                                    name: "right".to_string(),
                                    value: Operand::Int(2),
                                },
                            ],
                        },
                    },
                    Instruction::Assign {
                        target: "%t0".to_string(),
                        value: Rvalue::Cast {
                            value: Operand::Place("pair".to_string()),
                            ty: Type::named("int32"),
                            span: Span::new(1, 1),
                        },
                    },
                ],
                terminator: Terminator::Return(Operand::Int(0)),
            }],
        }],
        classes: vec![pair_class.clone()],
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };
    let plain_cast_source_error = emit_host_object(&plain_cast_source_module)
        .expect_err("casts from plain classes should be rejected before code emission");
    assert!(plain_cast_source_error
        .contains("direct backend only supports numeric casts from scalar values, found `Pair`"));

    for (label, module, expected) in [
        (
            "scalar field access",
            module_with_main_call(Rvalue::Member {
                object: Operand::Int(1),
                field: "missing".to_string(),
            }),
            "direct backend does not know field `missing` on `int64`",
        ),
        (
            "integer boolean op",
            module_with_main_call(Rvalue::Binary {
                op: BinaryOp::And,
                left: Operand::Int(1),
                right: Operand::Int(2),
                span: Span::new(1, 1),
                trap: crate::mir::ArithmeticTrap::None,
            }),
            "direct backend does not support integer binary operation `And`",
        ),
        (
            "float boolean op",
            module_with_main_call_result_type(
                Rvalue::Binary {
                    op: BinaryOp::And,
                    left: Operand::Float(1.0),
                    right: Operand::Float(2.0),
                    span: Span::new(1, 1),
                    trap: crate::mir::ArithmeticTrap::None,
                },
                Type::named("float64"),
            ),
            "direct backend does not support float binary operation `And`",
        ),
        (
            "bool arithmetic op",
            module_with_main_call_result_type(
                Rvalue::Binary {
                    op: BinaryOp::Add,
                    left: Operand::Bool(true),
                    right: Operand::Bool(false),
                    span: Span::new(1, 1),
                    trap: crate::mir::ArithmeticTrap::None,
                },
                Type::named("bool"),
            ),
            "direct backend does not support boolean binary operation `Add`",
        ),
        (
            "unsupported scalar cast target",
            module_with_main_call(Rvalue::Cast {
                value: Operand::Int(1),
                ty: Type::named("bool"),
                span: Span::new(1, 1),
            }),
            "direct backend only supports numeric casts, found `int64` to `bool`",
        ),
        (
            "nonnumeric cast source",
            module_with_main_call(Rvalue::Cast {
                value: Operand::Unit,
                ty: Type::named("int32"),
                span: Span::new(1, 1),
            }),
            "direct backend only supports numeric casts, found `None` to `int32`",
        ),
    ] {
        let error = emit_host_object(&module).expect_err(&format!("{label} should be rejected"));
        assert!(
            error.contains(expected),
            "{label} reported `{error}` instead of containing `{expected}`"
        );
    }

    let missing_field_access_module = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            source_path: None,
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: vec![
                MirLocalType {
                    name: "pair".to_string(),
                    ty: Type::named("Pair"),
                },
                MirLocalType {
                    name: "%t0".to_string(),
                    ty: Type::named("int32"),
                },
            ],
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: vec![
                    Instruction::Assign {
                        target: "pair".to_string(),
                        value: Rvalue::Construct {
                            class_name: "Pair".to_string(),
                            fields: vec![
                                crate::mir::MirFieldInit {
                                    name: "left".to_string(),
                                    value: Operand::Int(1),
                                },
                                crate::mir::MirFieldInit {
                                    name: "right".to_string(),
                                    value: Operand::Int(2),
                                },
                            ],
                        },
                    },
                    Instruction::Assign {
                        target: "%t0".to_string(),
                        value: Rvalue::Use(Operand::Place("pair.missing".to_string())),
                    },
                ],
                terminator: Terminator::Return(Operand::Int(0)),
            }],
        }],
        classes: vec![pair_class],
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };
    let missing_field_access_error = emit_host_object(&missing_field_access_module)
        .expect_err("unknown plain-class fields should be rejected");
    assert!(missing_field_access_error.contains("does not know field `missing`"));
}

#[test]
fn native_codegen_reports_invalid_non_boolean_branch_conditions() {
    let invalid_module = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            source_path: None,
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: vec![MirLocalType {
                name: "condition".to_string(),
                ty: Type::named("float64"),
            }],
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![
                BasicBlock {
                    label: "entry".to_string(),
                    instructions: vec![Instruction::Assign {
                        target: "condition".to_string(),
                        value: Rvalue::Use(Operand::Float(1.25)),
                    }],
                    terminator: Terminator::Branch {
                        condition: Operand::Place("condition".to_string()),
                        then_label: "then".to_string(),
                        else_label: "else".to_string(),
                    },
                },
                BasicBlock {
                    label: "then".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Operand::Int(1)),
                },
                BasicBlock {
                    label: "else".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Operand::Int(0)),
                },
            ],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };

    let error = emit_host_object(&invalid_module)
        .expect_err("non-boolean branch conditions should be rejected by direct codegen");
    assert!(error.contains("cannot use `float64` as a branch condition"));
}

#[test]
fn native_codegen_rejects_try_between_non_result_types() {
    let invalid_module = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            source_path: None,
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: vec![
                MirLocalType {
                    name: "source".to_string(),
                    ty: Type::named("int32"),
                },
                MirLocalType {
                    name: "target".to_string(),
                    ty: Type::named("int32"),
                },
            ],
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: vec![
                    Instruction::Assign {
                        target: "source".to_string(),
                        value: Rvalue::Use(Operand::Int(1)),
                    },
                    Instruction::Assign {
                        target: "target".to_string(),
                        value: Rvalue::Try {
                            value: Operand::Place("source".to_string()),
                            releases_on_error: Vec::new(),
                            from: None,
                        },
                    },
                ],
                terminator: Terminator::Return(Operand::Int(0)),
            }],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };

    let error = emit_host_object(&invalid_module)
        .expect_err("direct backend try should require Result operands and returns");
    assert!(
        error.contains("`try` needs a `Result` operand and a `Result` return"),
        "{error}"
    );
}

#[test]
fn direct_try_moves_noncopy_ok_payloads_through_the_destructive_runtime_path() {
    let source = r#"
def produce() -> Result[str, str]:
    return Result.Ok("owned payload")

def forward() -> Result[str, str]:
    result = produce()
    value = try result
    return Result.Ok(value)

def main() -> int32:
    return 0
"#;

    let module = lower_source_to_mir(source).expect("owned try source should lower to MIR");
    let object =
        emit_unoptimized_object(&module).expect("owned try source should compile directly");
    let referenced = object_referenced_symbols(&object);
    assert!(
        referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_variant_take_payload")),
        "owned try must destructively take its successful payload: {referenced:?}"
    );
}

const SCRATCH_BODY: &str = "scratch body";

/// Runs `body` in one block of a scratch function, which `codegen` never
/// defines.
fn with_scratch_function<'a>(
    codegen: &mut NativeCodegen<'a>,
    body: impl FnOnce(&mut NativeCodegen<'a>, &mut dyn FunctionBackend),
) {
    let id = codegen.functions["main"];
    let error = codegen
        .define_body(
            id,
            &Signature::new(CallConv::C),
            "scratch function",
            |codegen, builder| {
                let block = builder.create_block();
                builder.switch_to_block(block);
                builder.seal_block(block);
                body(codegen, builder);
                Err(SCRATCH_BODY.to_string())
            },
        )
        .expect_err("a scratch function stays undefined");
    assert_eq!(error, SCRATCH_BODY);
}

#[test]
fn native_codegen_thunk_helpers_cover_roundtrip_paths() {
    let source =
        "class Pair:\n    left: int32\n    right: bool\n\ndef main() -> int32:\n    return 0\n";
    let mir = lower_source_to_mir(source).expect("thunk helper source should lower");
    let mut codegen = NativeCodegen::new(&mir, "/tmp/thunk_helpers.au", source)
        .expect("codegen should initialize");

    with_scratch_function(&mut codegen, |codegen, builder| {
        let (_first_ptr, _first_len) = thunk_string_constant(codegen, builder, b"aura")
            .expect("first string constant should lower");
        let (_second_ptr, _second_len) = thunk_string_constant(codegen, builder, b"aura")
            .expect("duplicate string constant should reuse existing data");
        assert_eq!(codegen.string_data.len(), 1);

        let opaque_raw = builder.iconst(types::I64, 7);
        let int_raw = builder.iconst(types::I64, 11);
        let bool_raw = builder.iconst(types::I64, 1);
        let float_raw = builder.f64const(3.5);

        let opaque_boxed = box_thunk_value(
            codegen,
            builder,
            &[opaque_raw],
            &DirectType::Opaque(Type::named("str")),
        )
        .expect("opaque thunk values should pass through");
        let opaque_unboxed = unbox_thunk_value(
            codegen,
            builder,
            opaque_boxed,
            &DirectType::Opaque(Type::named("str")),
        )
        .expect("opaque thunk values should unbox directly");
        assert_eq!(opaque_unboxed.len(), 1);

        let boxed_int = box_thunk_value(
            codegen,
            builder,
            &[int_raw],
            &DirectType::Scalar(ScalarKind::Int32),
        )
        .expect("int thunk values should box");
        let unboxed_int = unbox_thunk_value(
            codegen,
            builder,
            boxed_int,
            &DirectType::Scalar(ScalarKind::Int32),
        )
        .expect("int thunk values should unbox");
        assert_eq!(unboxed_int.len(), 1);

        for (label, kind) in [("int64", ScalarKind::Int64), ("uint64", ScalarKind::Uint64)] {
            let boxed = box_thunk_value(codegen, builder, &[int_raw], &DirectType::Scalar(kind))
                .unwrap_or_else(|error| panic!("{label} thunk values should box: {error}"));
            let unboxed = unbox_thunk_value(codegen, builder, boxed, &DirectType::Scalar(kind))
                .unwrap_or_else(|error| panic!("{label} thunk values should unbox: {error}"));
            assert_eq!(unboxed.len(), 1, "{label} should have one ABI value");
            assert_eq!(
                builder.value_type(unboxed[0]),
                types::I64,
                "{label} thunk round trips must use the I64 ABI"
            );
        }

        let boxed_float = box_thunk_value(
            codegen,
            builder,
            &[float_raw],
            &DirectType::Scalar(ScalarKind::Float64),
        )
        .expect("float thunk values should box");
        let unboxed_float = unbox_thunk_value(
            codegen,
            builder,
            boxed_float,
            &DirectType::Scalar(ScalarKind::Float64),
        )
        .expect("float thunk values should unbox");
        assert_eq!(unboxed_float.len(), 1);

        let boxed_bool = box_thunk_value(
            codegen,
            builder,
            &[bool_raw],
            &DirectType::Scalar(ScalarKind::Bool),
        )
        .expect("bool thunk values should box");
        let unboxed_bool = unbox_thunk_value(
            codegen,
            builder,
            boxed_bool,
            &DirectType::Scalar(ScalarKind::Bool),
        )
        .expect("bool thunk values should unbox");
        assert_eq!(unboxed_bool.len(), 1);

        let boxed_unit =
            box_thunk_value(codegen, builder, &[], &DirectType::Scalar(ScalarKind::Unit))
                .expect("unit thunk values should box");
        let unboxed_unit = unbox_thunk_value(
            codegen,
            builder,
            boxed_unit,
            &DirectType::Scalar(ScalarKind::Unit),
        )
        .expect("unit thunk values should unbox");
        assert_eq!(unboxed_unit.len(), 1);
        let opaque_missing = box_thunk_value(
            codegen,
            builder,
            &[],
            &DirectType::Opaque(Type::named("str")),
        )
        .expect_err("opaque thunk boxing should require a raw value");
        assert!(opaque_missing.contains("task-start thunk expected an opaque value"));

        let pair_ty = DirectType::PlainClass(PlainClassType {
            class_name: "Pair".to_string(),
            fields: vec![
                PlainClassField {
                    name: "left".to_string(),
                    ty: DirectType::Scalar(ScalarKind::Int32),
                },
                PlainClassField {
                    name: "right".to_string(),
                    ty: DirectType::Scalar(ScalarKind::Bool),
                },
            ],
        });
        let boxed_pair = box_thunk_value(codegen, builder, &[int_raw, bool_raw], &pair_ty)
            .expect("plain class thunk values should box recursively");
        let unboxed_pair = unbox_thunk_value(codegen, builder, boxed_pair, &pair_ty)
            .expect("plain class thunk values should unbox recursively");
        assert_eq!(unboxed_pair.len(), 2);
        assert!(codegen.string_data.len() >= 3);

        builder.return_(&[]);
    });
}

#[test]
fn native_codegen_release_helpers_cover_cleanup_error_paths() {
    let source = "def main() -> int32:\n    return 0\n";
    let mir = lower_source_to_mir(source).expect("release helper source should lower");
    let mut codegen = NativeCodegen::new(&mir, "/tmp/release_helpers.au", source)
        .expect("codegen should initialize");

    with_scratch_function(&mut codegen, |codegen, builder| {
        let raw_opaque = builder.iconst(types::I64, 7);
        let raw_unit = builder.iconst(types::I64, 0);
        let raw_bool = builder.iconst(types::I64, 1);
        let opaque_string = DirectType::Opaque(Type::named("str"));
        let plain_pair = DirectType::PlainClass(PlainClassType {
            class_name: "Pair".to_string(),
            fields: vec![
                PlainClassField {
                    name: "text".to_string(),
                    ty: opaque_string.clone(),
                },
                PlainClassField {
                    name: "flag".to_string(),
                    ty: DirectType::Scalar(ScalarKind::Bool),
                },
            ],
        });

        let unknown_return_error =
            release_direct_call_results(codegen, builder, "missing", &[raw_opaque])
                .expect_err("unknown cleanup return types should fail");
        assert!(unknown_return_error.contains("does not know return type for `missing`"));

        codegen
            .function_return_types
            .insert("needs_opaque".to_string(), opaque_string.clone());
        let too_few_error = release_direct_call_results(codegen, builder, "needs_opaque", &[])
            .expect_err("too few cleanup return values should fail");
        assert!(too_few_error.contains("cleanup call `needs_opaque` returned too few values"));

        let missing_opaque_error = release_direct_values(codegen, builder, &[], &opaque_string)
            .expect_err("opaque cleanup release should require one value");
        assert!(missing_opaque_error.contains("expected an opaque `str` result"));

        codegen.function_return_types.insert(
            "unit_with_writeback".to_string(),
            DirectType::Scalar(ScalarKind::Unit),
        );
        codegen.function_writeback_types.insert(
            "unit_with_writeback".to_string(),
            vec![opaque_string.clone()],
        );
        let incomplete_writeback_error =
            release_direct_call_results(codegen, builder, "unit_with_writeback", &[raw_unit])
                .expect_err("missing cleanup writeback values should fail");
        assert!(incomplete_writeback_error
            .contains("cleanup call `unit_with_writeback` returned incomplete writeback values"));

        release_direct_call_results(
            codegen,
            builder,
            "unit_with_writeback",
            &[raw_unit, raw_opaque],
        )
        .expect("complete cleanup writeback values should release");

        let incomplete_plain_class_error =
            release_direct_values(codegen, builder, &[raw_opaque], &plain_pair)
                .expect_err("plain class cleanup release should require every field value");
        assert!(incomplete_plain_class_error.contains("expected `2` values for `Pair`"));

        release_direct_values(codegen, builder, &[raw_opaque, raw_bool], &plain_pair)
            .expect("complete plain class values should release recursively");

        builder.return_(&[]);
    });
}

#[test]
fn direct_backend_emits_object_for_module_examples() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("compiler crate should live under repo root")
        .parent()
        .expect("compiler crate should live under repo root")
        .to_path_buf();
    let examples = [
        repo_root.join("examples/modules/namespace_import_types.au"),
        repo_root.join("examples/modules/trait_impl_imports.au"),
    ];

    for path in examples {
        let mir = lower_path_to_mir(&path).expect("module example should lower to MIR");
        let object = emit_host_object(&mir).expect("module example should emit direct object");
        assert!(!object.is_empty(), "{}", path.display());
    }
}

#[test]
fn direct_backend_emits_object_for_broad_maintained_example_surface() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("compiler crate should live under repo root")
        .parent()
        .expect("compiler crate should live under repo root")
        .to_path_buf();
    let paths = [
        "examples/basics/top_level_script.au",
        "examples/basics/main_function.au",
        "examples/basics/mutable_bindings.au",
        "examples/basics/default_arguments.au",
        "examples/basics/pass_keyword.au",
        "examples/basics/borrow_parameters.au",
        "examples/basics/named_arguments.au",
        "examples/basics/named_builtin_arguments.au",
        "examples/basics/none_values.au",
        "examples/basics/simple_example.au",
        "examples/classes/point_distance.au",
        "examples/classes/default_fields.au",
        "examples/classes/methods.au",
        "examples/classes/copy_class.au",
        "examples/classes/indirect_recursive.au",
        "examples/classes/mutating_methods.au",
        "examples/control_flow/if_elif_else.au",
        "examples/control_flow/for_range.au",
        "examples/control_flow/while_break_continue.au",
        "examples/control_flow/boolean_logic.au",
        "examples/enums/result_match.au",
        "examples/enums/result_option.au",
        "examples/enums/explicit_type_args.au",
        "examples/enums/match_borrow.au",
        "examples/error_handling/try_result.au",
        "examples/generics/box_and_wrapper.au",
        "examples/generics/generic_constructor_specialization.au",
        "examples/traits/greeter.au",
        "examples/traits/multiple_bounds.au",
        "examples/traits/generic_trait_impl.au",
        "examples/traits/specialized_trait_dispatch.au",
        "examples/traits/trait_associated_factory.au",
        "examples/traits/associated_through_type_param.au",
        "examples/json/typed_codec.au",
        "examples/json/derived_codec.au",
        "examples/traits/operator_traits.au",
        "examples/traits/ordering_traits.au",
        "examples/basics/copy_return_selection.au",
        "examples/traits/generic_trait_bounds.au",
        "examples/numbers/float_sqrt.au",
        "examples/numbers/float32_values.au",
        "examples/numbers/numeric_casts.au",
        "examples/numbers/uint128_values.au",
        "examples/numbers/unary_minus.au",
        "examples/strings/string_clone.au",
        "examples/strings/f_strings.au",
        "examples/strings/borrow_str.au",
        "examples/strings/string_methods.au",
        "examples/strings/string_parsing_and_formatting.au",
        "examples/concurrency/task_group_start.au",
        "examples/concurrency/queue_iteration.au",
        "examples/concurrency/queue_put_timeout.au",
        "examples/concurrency/queue_get_timeout_named.au",
        "examples/concurrency/task_group_start_soon.au",
        "examples/concurrency/task_group_cancel.au",
        "examples/concurrency/task_group_queue_sum.au",
        "examples/io/read_text_file.au",
        "examples/io/bytes_file_io.au",
        "examples/io/atomic_save.au",
        "examples/io/process_run.au",
        "examples/io/process_supervisor.au",
        "examples/io/tcp_echo.au",
        "examples/io/tcp_bytes.au",
        "examples/io/udp_echo.au",
        "examples/io/http_roundtrip.au",
        "examples/io/websocket_roundtrip.au",
        "examples/resources/with_resource.au",
        "examples/modules/namespace_import_types.au",
        "examples/modules/trait_impl_imports.au",
        "examples/basic_addition.au",
        "examples/control_flow.au",
        "examples/point.au",
        "examples/simple_addition.au",
        "examples/top_level_addition.au",
    ];

    for relative in paths {
        let path = repo_root.join(relative);
        let mir = lower_path_to_mir(&path).expect("maintained example should lower to MIR");
        let object =
            emit_host_object(&mir).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert!(!object.is_empty(), "{}", path.display());
    }
}

fn assert_direct_backend_emits_object_for_scratch_repro(relative: &str) {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("compiler crate should live under repo root")
        .parent()
        .expect("compiler crate should live under repo root")
        .to_path_buf();
    let path = repo_root.join(relative);
    let mir = lower_path_to_mir(&path).expect("scratch repro should lower to MIR");
    let object = emit_host_object(&mir).expect("scratch repro should emit direct object");
    assert!(!object.is_empty(), "{}", path.display());
}

#[test]
fn direct_backend_emits_object_for_generic_trait_bound_returning_int_repro() {
    assert_direct_backend_emits_object_for_scratch_repro(
        "test_edge/gt_26_generic_fn_with_trait_bound_returns_int.au",
    );
}

#[test]
fn direct_backend_emits_object_for_multi_param_trait_bound_repro() {
    assert_direct_backend_emits_object_for_scratch_repro(
        "test_edge/gt_49_trait_bound_with_multiple_params.au",
    );
}

#[test]
fn direct_backend_emits_object_for_generic_sort_trait_bound_repro() {
    assert_direct_backend_emits_object_for_scratch_repro(
        "test_edge/test_complex_15_generic_sort.au",
    );
}

#[test]
fn direct_backend_emits_object_for_multiple_trait_methods_repro() {
    assert_direct_backend_emits_object_for_scratch_repro(
        "test_edge/test_trait_multiple_methods.au",
    );
}

#[test]
fn mangle_symbol_rewrites_non_alphanumeric_characters() {
    assert_eq!(mangle_symbol("main"), "aura_fn_main");
    assert_eq!(
        mangle_symbol("helpers.math.double"),
        "aura_fn_helpers_math_double"
    );
}

#[test]
fn direct_type_supports_plain_classes_and_scalars() {
    let source = include_str!("../../../examples/classes/methods.au");
    let mir = lower_source_to_mir(source).expect("methods example should lower");
    let classes = mir
        .classes
        .iter()
        .cloned()
        .map(|class| (class.name.clone(), class))
        .collect::<HashMap<_, _>>();

    assert_eq!(
        scalar_kind_for_tests(&Type::named("int32")),
        Some(ScalarKind::Int32)
    );
    assert_eq!(
        scalar_kind_for_tests(&Type::named("float64")),
        Some(ScalarKind::Float64)
    );
    assert_eq!(
        scalar_kind_for_tests(&Type::named("bool")),
        Some(ScalarKind::Bool)
    );
    assert_eq!(scalar_kind_for_tests(&Type::Unit), Some(ScalarKind::Unit));

    let counter = direct_type(&Type::named("Counter"), &classes).expect("Counter should be direct");
    assert_eq!(render_direct_type(&counter), "Counter");
    assert_eq!(counter.value_count(), 1);
}

#[test]
fn ticket9_int64_and_uint64_are_unboxed_i64_direct_scalars() {
    let classes = HashMap::new();

    for (name, kind) in [("int64", ScalarKind::Int64), ("uint64", ScalarKind::Uint64)] {
        let direct = direct_type(&Type::named(name), &classes)
            .unwrap_or_else(|| panic!("{name} should have a direct type"));

        assert_eq!(direct, DirectType::Scalar(kind));
        assert_eq!(direct.scalar_kind(), Some(kind));
        assert_eq!(kind.signature_type(), types::I64);
        assert_eq!(direct.abi_types(), vec![types::I64]);
        assert_eq!(render_direct_type(&direct), name);
        assert_eq!(direct_type_to_type(&direct), Type::named(name));
        assert!(!kind.is_float());
    }

    let source = r#"
def wide_identity(signed: int64, unsigned: uint64) -> int64:
    if unsigned > 0:
        return signed
    return 0

def main() -> int32:
    return 0
"#;
    let mir = lower_source_to_mir(source).expect("wide scalar signature should lower");
    let function = mir
        .functions
        .iter()
        .find(|function| function.name == "wide_identity")
        .expect("wide_identity should be present in MIR");
    let signature =
        signature_for(function, &classes).expect("int64 and uint64 should have direct signatures");

    assert_eq!(signature.params, vec![types::I64, types::I64]);
    assert_eq!(signature.returns, vec![types::I64]);
}

#[test]
fn infer_operand_types_track_plain_classes() {
    let mut variable_types = HashMap::new();
    variable_types.insert("flag".to_string(), DirectType::Scalar(ScalarKind::Bool));
    variable_types.insert("number".to_string(), DirectType::Scalar(ScalarKind::Int32));
    variable_types.insert("ratio".to_string(), DirectType::Scalar(ScalarKind::Float64));
    variable_types.insert("word".to_string(), DirectType::Opaque(Type::named("str")));
    variable_types.insert("node".to_string(), DirectType::Opaque(Type::named("Node")));
    variable_types.insert(
        "tasks".to_string(),
        DirectType::Opaque(Type::Named(
            "list".to_string(),
            vec![Type::Named("Task".to_string(), vec![Type::named("str")])],
        )),
    );
    variable_types.insert(
        "unit_tasks".to_string(),
        DirectType::Opaque(Type::Named(
            "list".to_string(),
            vec![Type::Named("Task".to_string(), Vec::new())],
        )),
    );
    variable_types.insert(
        "strings".to_string(),
        DirectType::Opaque(Type::Named("list".to_string(), vec![Type::named("str")])),
    );
    variable_types.insert(
        "result".to_string(),
        DirectType::Opaque(Type::Named(
            "Result".to_string(),
            vec![Type::named("str"), Type::named("io.Error")],
        )),
    );
    variable_types.insert(
        "point".to_string(),
        DirectType::PlainClass(super::PlainClassType {
            class_name: "Point".to_string(),
            fields: vec![
                super::PlainClassField {
                    name: "x".to_string(),
                    ty: DirectType::Scalar(ScalarKind::Float64),
                },
                super::PlainClassField {
                    name: "y".to_string(),
                    ty: DirectType::Scalar(ScalarKind::Float64),
                },
            ],
        }),
    );
    let classes = HashMap::from([
        (
            "Point".to_string(),
            crate::mir::MirClass {
                name: "Point".to_string(),
                type_params: Vec::new(),
                fields: Vec::new(),
                methods: vec![crate::mir::MirMethod {
                    name: "norm".to_string(),
                    function_name: "Point::norm".to_string(),
                    receiver: Some(MirReceiverKind::Borrow),
                }],
                copy: false,
            },
        ),
        (
            "Node".to_string(),
            crate::mir::MirClass {
                name: "Node".to_string(),
                type_params: Vec::new(),
                fields: Vec::new(),
                methods: vec![crate::mir::MirMethod {
                    name: "size".to_string(),
                    function_name: "Node::size".to_string(),
                    receiver: Some(MirReceiverKind::Borrow),
                }],
                copy: false,
            },
        ),
    ]);

    assert_eq!(
        infer_operand_type(
            &Operand::Place("flag".to_string()),
            &variable_types,
            &HashMap::new()
        ),
        Some(DirectType::Scalar(ScalarKind::Bool))
    );
    assert_eq!(
        infer_operand_type(
            &Operand::Place("point.x".to_string()),
            &variable_types,
            &HashMap::new()
        ),
        Some(DirectType::Scalar(ScalarKind::Float64))
    );
    assert_eq!(
        infer_operand_type(&Operand::Bool(true), &variable_types, &classes),
        Some(DirectType::Scalar(ScalarKind::Bool))
    );
    assert_eq!(
        infer_operand_type(&Operand::Float(1.25), &variable_types, &classes),
        Some(DirectType::Scalar(ScalarKind::Float64))
    );
    assert_eq!(
        infer_operand_type(&Operand::Unit, &variable_types, &classes),
        Some(DirectType::Scalar(ScalarKind::Unit))
    );
}

#[test]
fn validate_operand_accepts_nested_places() {
    validate_operand(&Operand::Place("point.x".to_string()))
        .expect("nested places should now validate directly");
}

#[test]
fn direct_validation_rejects_move_place_in_non_consuming_expressions() {
    let function = MirFunction {
        name: "main".to_string(),
        module_name: "<test>".to_string(),
        source_path: None,
        span: Span::new(1, 1),
        receiver: None,
        params: Vec::new(),
        local_types: vec![MirLocalType {
            name: "value".to_string(),
            ty: Type::named("int32"),
        }],
        return_type: Type::named("int32"),
        entry: "entry".to_string(),
        blocks: vec![BasicBlock {
            label: "entry".to_string(),
            instructions: vec![Instruction::Assign {
                target: "%negated".to_string(),
                value: Rvalue::Unary {
                    op: UnaryOp::Neg,
                    value: Operand::MovePlace("value".to_string()),
                    span: Span::new(1, 1),
                    trap: crate::mir::ArithmeticTrap::None,
                },
            }],
            terminator: Terminator::Return(Operand::Int(0)),
        }],
    };

    let error = validate_function(&function, &HashMap::new())
        .expect_err("MovePlace must be rejected for a non-consuming unary read");
    assert!(error.contains("only permits `MovePlace` in consuming contexts"));

    let mut consuming = function;
    consuming.blocks[0].instructions[0] = Instruction::Assign {
        target: "%moved".to_string(),
        value: Rvalue::Use(Operand::MovePlace("value".to_string())),
    };
    validate_function(&consuming, &HashMap::new())
        .expect("MovePlace should remain valid for a consuming assignment");

    let task_start = Rvalue::StartTask {
        returns_handle: true,
        result_is_copy: true,
        stack_size: Some(Operand::MovePlace("stack_bytes".to_string())),
        task_group: Operand::Place("group".to_string()),
        function: test_function_operand("worker", Vec::new(), Type::Unit),
        args: Vec::new(),
        span: Span::new(1, 1),
    };
    let error = validate_rvalue(&task_start, &HashMap::new())
        .expect_err("a task stack override is a non-consuming scalar read");
    assert_eq!(
        error,
        "direct backend only permits `MovePlace` in consuming contexts, not in a task stack size"
    );

    let task_start = Rvalue::StartTask {
        returns_handle: true,
        result_is_copy: true,
        stack_size: Some(Operand::Place("stack_bytes".to_string())),
        task_group: Operand::MovePlace("group".to_string()),
        function: test_function_operand("worker", Vec::new(), Type::Unit),
        args: Vec::new(),
        span: Span::new(1, 1),
    };
    let error = validate_rvalue(&task_start, &HashMap::new())
        .expect_err("a task-group receiver must stay live until task registration completes");
    assert_eq!(
        error,
        "direct backend only permits `MovePlace` in consuming contexts, not in a task-group receiver"
    );

    let task_start = Rvalue::StartTask {
        returns_handle: true,
        result_is_copy: true,
        stack_size: Some(Operand::Place("stack_bytes".to_string())),
        task_group: Operand::Place("group".to_string()),
        function: test_function_operand("worker", Vec::new(), Type::Unit),
        args: Vec::new(),
        span: Span::new(1, 1),
    };
    validate_rvalue(&task_start, &HashMap::new())
        .expect("shared reads of a stack override and task-group receiver must validate");
}

#[test]
fn ensure_direct_type_maps_runtime_backed_types_to_opaque_values() {
    let ty = ensure_direct_type(&Type::named("str"), &HashMap::new(), "test type")
        .expect("runtime-backed types should still be representable directly");
    assert_eq!(ty, DirectType::Opaque(Type::named("str")));
}

#[test]
fn native_codegen_receiver_and_type_param_helpers_cover_missing_receiver_and_terminal_types() {
    let function = MirFunction {
        name: "Widget.read".to_string(),
        module_name: "<test>".to_string(),
        source_path: None,
        span: Span::new(1, 1),
        receiver: Some(MirReceiverKind::Borrow),
        params: Vec::new(),
        local_types: Vec::new(),
        return_type: Type::named("int32"),
        entry: "entry".to_string(),
        blocks: vec![BasicBlock {
            label: "entry".to_string(),
            instructions: Vec::new(),
            terminator: Terminator::Return(Operand::Int(0)),
        }],
    };
    let error = super::receiver_type(&function, &HashMap::new())
        .expect_err("receiver functions without a self local should be rejected");
    assert!(error.contains("could not find receiver local type for `Widget.read`"));

    let mut collected = BTreeSet::new();
    collect_type_params_from_type(&Type::Unit, &mut collected);
    collect_type_params_from_type(&Type::Module("pkg.tools".to_string()), &mut collected);
    assert!(collected.is_empty());
}

#[test]
fn signature_helpers_flatten_plain_class_abi_types() {
    let mut classes = HashMap::new();
    classes.insert(
        "Point".to_string(),
        crate::mir::MirClass {
            name: "Point".to_string(),
            type_params: Vec::new(),
            fields: vec![
                crate::mir::MirClassField {
                    name: "x".to_string(),
                    ty: Type::named("float64"),
                },
                crate::mir::MirClassField {
                    name: "y".to_string(),
                    ty: Type::named("float64"),
                },
            ],
            methods: Vec::new(),
            copy: false,
        },
    );
    let function = MirFunction {
        name: "demo".to_string(),
        module_name: "<test>".to_string(),
        source_path: None,
        span: crate::diag::Span::new(1, 1),
        receiver: Some(MirReceiverKind::Borrow),
        params: vec![crate::mir::MirParam {
            name: "other".to_string(),
            passing: MirReceiverKind::Value,
            ty: Type::named("Point"),
            default_function: None,
            keyword_only: false,
        }],
        local_types: vec![crate::mir::MirLocalType {
            name: "self".to_string(),
            ty: Type::named("Point"),
        }],
        return_type: Type::named("float64"),
        entry: "entry".to_string(),
        blocks: Vec::new(),
    };

    let sig = signature_for(&function, &classes)
        .expect("signature should flatten point receiver and param");
    let main_sig = main_signature();

    assert_eq!(sig.params.len(), 4);
    assert_eq!(sig.returns.len(), 1);
    assert_eq!(main_sig.returns.len(), 1);

    let bool_ty = DirectType::Scalar(ScalarKind::Bool);
    assert_eq!(bool_ty.abi_types(), vec![types::I64]);
    assert_eq!(bool_ty.value_count(), 1);
    assert_eq!(bool_ty.scalar_kind(), Some(ScalarKind::Bool));
    assert!(!ScalarKind::Bool.is_float());
    assert!(ScalarKind::Float64.is_float());

    let point_ty = DirectType::PlainClass(PlainClassType {
        class_name: "Point".to_string(),
        fields: vec![
            PlainClassField {
                name: "x".to_string(),
                ty: DirectType::Scalar(ScalarKind::Float64),
            },
            PlainClassField {
                name: "visible".to_string(),
                ty: DirectType::Scalar(ScalarKind::Bool),
            },
        ],
    });
    assert_eq!(point_ty.abi_types(), vec![types::F64, types::I64,]);
    assert_eq!(point_ty.value_count(), 2);
    assert_eq!(
        point_ty.field_slice("x"),
        Some((0, 1, DirectType::Scalar(ScalarKind::Float64)))
    );
    assert_eq!(
        point_ty.field_slice("visible"),
        Some((1, 2, DirectType::Scalar(ScalarKind::Bool)))
    );
    assert_eq!(point_ty.field_slice("missing"), None);
    assert_eq!(render_direct_type(&point_ty), "Point");
}

#[test]
fn cleanup_place_type_resolves_receivers_params_locals_and_temporaries() {
    let resource_class = crate::mir::MirClass {
        name: "Resource".to_string(),
        type_params: Vec::new(),
        fields: vec![crate::mir::MirClassField {
            name: "closed".to_string(),
            ty: Type::named("bool"),
        }],
        methods: Vec::new(),
        copy: false,
    };
    let holder_class = crate::mir::MirClass {
        name: "Holder".to_string(),
        type_params: Vec::new(),
        fields: vec![crate::mir::MirClassField {
            name: "resource".to_string(),
            ty: Type::named("Resource"),
        }],
        methods: Vec::new(),
        copy: false,
    };
    let classes = HashMap::from([
        ("Resource".to_string(), resource_class),
        ("Holder".to_string(), holder_class),
    ]);
    let function = MirFunction {
        name: "cleanup_demo".to_string(),
        module_name: "<test>".to_string(),
        source_path: None,
        span: Span::new(1, 1),
        receiver: Some(MirReceiverKind::BorrowMut),
        params: vec![MirParam {
            name: "input".to_string(),
            passing: MirReceiverKind::Borrow,
            ty: Type::named("Holder"),
            default_function: None,
            keyword_only: false,
        }],
        local_types: vec![
            MirLocalType {
                name: "self".to_string(),
                ty: Type::named("Holder"),
            },
            MirLocalType {
                name: "local".to_string(),
                ty: Type::named("Resource"),
            },
            MirLocalType {
                name: "%made".to_string(),
                ty: Type::named("Resource"),
            },
        ],
        return_type: Type::Unit,
        entry: "entry".to_string(),
        blocks: vec![BasicBlock {
            label: "entry".to_string(),
            instructions: vec![Instruction::Assign {
                target: "%made".to_string(),
                value: Rvalue::Construct {
                    class_name: "Resource".to_string(),
                    fields: Vec::new(),
                },
            }],
            terminator: Terminator::Return(Operand::Unit),
        }],
    };

    assert_eq!(
        cleanup_place_type(&function, &classes, "self.resource.closed")
            .expect("receiver cleanup fields should resolve"),
        DirectType::Scalar(ScalarKind::Bool)
    );
    assert_eq!(
        render_direct_type(
            &cleanup_place_type(&function, &classes, "input.resource")
                .expect("parameter cleanup fields should resolve")
        ),
        "Resource"
    );
    assert_eq!(
        render_direct_type(
            &cleanup_place_type(&function, &classes, "local")
                .expect("typed locals should resolve as cleanup places")
        ),
        "Resource"
    );
    assert_eq!(
        cleanup_place_type(&function, &classes, "%made.closed")
            .expect("assigned temporaries should read their MIR type"),
        DirectType::Scalar(ScalarKind::Bool)
    );
    assert!(cleanup_place_type(&function, &classes, "missing")
        .expect_err("unknown cleanup roots should be rejected")
        .contains("does not know cleanup place `missing`"));
    assert!(
        cleanup_place_type(&function, &classes, "self.resource.missing")
            .expect_err("unknown cleanup fields should be rejected")
            .contains("does not know cleanup field `missing`")
    );
}

#[test]
fn direct_field_and_try_helpers_cover_remaining_direct_inference_paths() {
    let classes = HashMap::from([
        (
            "Entry".to_string(),
            crate::mir::MirClass {
                name: "Entry".to_string(),
                type_params: Vec::new(),
                fields: vec![crate::mir::MirClassField {
                    name: "name".to_string(),
                    ty: Type::named("str"),
                }],
                methods: Vec::new(),
                copy: false,
            },
        ),
        (
            "Box".to_string(),
            crate::mir::MirClass {
                name: "Box".to_string(),
                type_params: vec!["T".to_string()],
                fields: vec![crate::mir::MirClassField {
                    name: "value".to_string(),
                    ty: Type::TypeParam("T".to_string()),
                }],
                methods: Vec::new(),
                copy: false,
            },
        ),
    ]);
    let variable_types = HashMap::from([
        (
            "result".to_string(),
            DirectType::Opaque(Type::Named(
                "Result".to_string(),
                vec![Type::named("int32"), Type::named("str")],
            )),
        ),
        (
            "maybe".to_string(),
            DirectType::Opaque(Type::Named(
                "Lookup".to_string(),
                vec![Type::named("Entry")],
            )),
        ),
    ]);

    assert_eq!(
        direct_field_type(
            &DirectType::Opaque(Type::Named("Entry".to_string(), vec![])),
            "name",
            &classes,
        ),
        Some(DirectType::Opaque(Type::named("str")))
    );
    assert_eq!(
        direct_field_type(
            &DirectType::Opaque(Type::Named("Entry".to_string(), vec![Type::named("int32")])),
            "name",
            &classes,
        ),
        None
    );
    assert_eq!(
        direct_field_type(
            &DirectType::Opaque(Type::Named("Box".to_string(), vec![Type::named("str")],)),
            "value",
            &classes,
        ),
        Some(DirectType::Opaque(Type::named("str")))
    );
    assert_eq!(
        infer_operand_type(&Operand::Unit, &variable_types, &classes),
        Some(DirectType::Scalar(ScalarKind::Unit))
    );
    assert_eq!(
        infer_operand_type(
            &Operand::String("aura".to_string()),
            &variable_types,
            &classes,
        ),
        Some(DirectType::Opaque(Type::named("str")))
    );
    assert_eq!(
        infer_operand_type(&Operand::Duration(5), &variable_types, &classes),
        Some(DirectType::Opaque(Type::named("Duration")))
    );
    assert_eq!(
        infer_operand_type(
            &Operand::Int((i64::MAX as u128) + 1),
            &variable_types,
            &classes,
        ),
        Some(DirectType::Opaque(Type::named("Unknown")))
    );
}

#[test]
fn native_codegen_variant_payload_helpers_cover_builtin_result_shapes() {
    let classes = HashMap::new();
    let named = |name: &str| Type::Named(name.to_string(), Vec::new());
    let opaque_named =
        |name: &str, args: Vec<Type>| DirectType::Opaque(Type::Named(name.to_string(), args));
    let direct_int = DirectType::Scalar(ScalarKind::Int32);
    let direct_index = DirectType::Scalar(ScalarKind::Int64);
    let direct_string = DirectType::Opaque(Type::named("str"));
    let direct_vec_int =
        DirectType::Opaque(Type::Named("list".to_string(), vec![Type::named("int32")]));

    for (target, enum_name, variant, expected) in [
        (
            opaque_named("Lookup", vec![Type::named("int32")]),
            "Lookup",
            "Found",
            Some(vec![direct_int.clone()]),
        ),
        (
            opaque_named("Lookup", vec![Type::named("int32")]),
            "Lookup",
            "Missing",
            Some(Vec::new()),
        ),
        (
            opaque_named("Result", vec![Type::named("str"), named("io.Error")]),
            "Result",
            "Ok",
            Some(vec![direct_string.clone()]),
        ),
        (
            opaque_named("Result", vec![Type::named("str"), named("io.Error")]),
            "Result",
            "Err",
            Some(vec![DirectType::Opaque(named("io.Error"))]),
        ),
        (
            opaque_named("SendError", vec![Type::named("int32")]),
            "SendError",
            "Full",
            Some(vec![direct_int.clone()]),
        ),
        (
            opaque_named("QueueReceive", vec![Type::named("str")]),
            "QueueReceive",
            "Item",
            Some(vec![direct_string.clone()]),
        ),
        (
            opaque_named("QueueReceive", vec![Type::named("str")]),
            "QueueReceive",
            "Closed",
            Some(Vec::new()),
        ),
        (
            opaque_named("TaskResult", vec![Type::named("int32")]),
            "TaskResult",
            "Ready",
            Some(vec![direct_int.clone()]),
        ),
        (
            opaque_named("TaskResult", vec![Type::named("int32")]),
            "TaskResult",
            "Error",
            Some(vec![direct_string.clone()]),
        ),
        (
            opaque_named("TaskResult", vec![Type::named("int32")]),
            "TaskResult",
            "TimedOut",
            Some(Vec::new()),
        ),
        (
            opaque_named("WaitAny", vec![Type::named("str")]),
            "WaitAny",
            "Ready",
            Some(vec![direct_index.clone(), direct_string.clone()]),
        ),
        (
            opaque_named("WaitAny", vec![Type::named("str")]),
            "WaitAny",
            "Error",
            Some(vec![direct_index.clone(), direct_string.clone()]),
        ),
        (
            opaque_named("WaitAny", vec![Type::named("str")]),
            "WaitAny",
            "Cancelled",
            Some(Vec::new()),
        ),
        (
            opaque_named("WaitAll", vec![Type::named("int32")]),
            "WaitAll",
            "Ready",
            Some(vec![direct_vec_int.clone()]),
        ),
        (
            opaque_named("WaitAll", vec![Type::named("int32")]),
            "WaitAll",
            "Error",
            Some(vec![direct_index.clone(), direct_string.clone()]),
        ),
        (
            opaque_named("WaitAll", vec![Type::named("int32")]),
            "WaitAll",
            "TimedOut",
            Some(Vec::new()),
        ),
        (
            opaque_named(
                "SelectOutcome",
                vec![Type::named("str"), Type::named("int32")],
            ),
            "SelectOutcome",
            "Queue",
            Some(vec![
                direct_index.clone(),
                opaque_named("QueueReceive", vec![Type::named("str")]),
            ]),
        ),
        (
            opaque_named(
                "SelectOutcome",
                vec![Type::named("str"), Type::named("int32")],
            ),
            "SelectOutcome",
            "Task",
            Some(vec![
                direct_index.clone(),
                opaque_named("TaskResult", vec![Type::named("int32")]),
            ]),
        ),
        (
            opaque_named(
                "SelectOutcome",
                vec![Type::named("str"), Type::named("int32")],
            ),
            "SelectOutcome",
            "Deadline",
            Some(vec![direct_index.clone()]),
        ),
        (
            opaque_named(
                "SelectOutcome",
                vec![Type::named("str"), Type::named("int32")],
            ),
            "SelectOutcome",
            "Cancelled",
            Some(Vec::new()),
        ),
        (
            opaque_named("Result", vec![Type::named("str"), named("io.Error")]),
            "Lookup",
            "Found",
            None,
        ),
        (
            opaque_named("Result", vec![Type::named("str"), named("io.Error")]),
            "Result",
            "Missing",
            None,
        ),
    ] {
        assert_eq!(
            enum_variant_payload_types_for_target(enum_name, variant, &target, &classes),
            expected,
            "unexpected payload types for {enum_name}.{variant}"
        );
    }
    assert_eq!(
        enum_variant_payload_types_for_target(
            "Result",
            "Ok",
            &DirectType::Scalar(ScalarKind::Int32),
            &classes,
        ),
        None
    );
}

#[test]
fn native_codegen_helper_utilities_cover_signatures_wildcards_and_metadata() {
    let main_sig = main_signature();
    assert_eq!(main_sig.params.len(), 0);
    assert_eq!(main_sig.returns.len(), 1);

    let thunk_sig = thunk_signature();
    assert_eq!(thunk_sig.params.len(), 2);
    assert_eq!(thunk_sig.returns.len(), 1);

    assert_eq!(
        mangle_thunk_symbol("pkg.main::worker"),
        "aura_thunk_pkg_main__worker"
    );

    assert_eq!(
        direct_type_to_type(&DirectType::Scalar(ScalarKind::Unit)),
        Type::Unit
    );
    assert_eq!(
        direct_type_to_type(&DirectType::Scalar(ScalarKind::Float32)),
        Type::named("float32")
    );
    assert_eq!(
        direct_type_to_type(&DirectType::Scalar(ScalarKind::Float64)),
        Type::named("float64")
    );
    assert_eq!(
        direct_type_to_type(&DirectType::Scalar(ScalarKind::Bool)),
        Type::named("bool")
    );
    assert_eq!(
        direct_type_to_type(&DirectType::PlainClass(PlainClassType {
            class_name: "Point".to_string(),
            fields: vec![PlainClassField {
                name: "x".to_string(),
                ty: DirectType::Scalar(ScalarKind::Int32),
            }],
        })),
        Type::named("Point")
    );
    assert_eq!(
        direct_type_to_type(&DirectType::Opaque(Type::Named(
            "list".to_string(),
            vec![Type::named("int32")],
        ))),
        Type::Named("list".to_string(), vec![Type::named("int32")])
    );
    assert_eq!(
        DirectType::Scalar(ScalarKind::Bool).scalar_kind(),
        Some(ScalarKind::Bool)
    );
    assert_eq!(DirectType::Opaque(Type::named("str")).scalar_kind(), None);
    assert_eq!(
        render_direct_type(&DirectType::Scalar(ScalarKind::Float32)),
        "float32"
    );
    assert_eq!(
        render_direct_type(&DirectType::Scalar(ScalarKind::Float64)),
        "float64"
    );
    assert_eq!(
        render_direct_type(&DirectType::Scalar(ScalarKind::Bool)),
        "bool"
    );
    assert!(!ScalarKind::Bool.is_float());
    assert!(ScalarKind::Float32.is_float());
    assert_eq!(
        DirectType::Scalar(ScalarKind::Float64).abi_types(),
        vec![types::F64]
    );
    assert_eq!(
        DirectType::Opaque(Type::named("str")).abi_types(),
        vec![types::I64]
    );
    let flat_class = DirectType::PlainClass(PlainClassType {
        class_name: "Pair".to_string(),
        fields: vec![
            PlainClassField {
                name: "left".to_string(),
                ty: DirectType::Scalar(ScalarKind::Int32),
            },
            PlainClassField {
                name: "right".to_string(),
                ty: DirectType::Scalar(ScalarKind::Float64),
            },
        ],
    });
    assert_eq!(flat_class.abi_types(), vec![types::I64, types::F64]);
    assert_eq!(flat_class.value_count(), 2);
    assert_eq!(
        flat_class.field_slice("right"),
        Some((1, 2, DirectType::Scalar(ScalarKind::Float64)))
    );
    assert_eq!(flat_class.field_slice("missing"), None);

    let source = "def main() -> int32:\n    return 0\n";
    let mir = lower_source_to_mir(source).expect("zero value source should lower");
    let mut codegen =
        NativeCodegen::new(&mir, "/tmp/zero_values.au", source).expect("codegen should initialize");
    with_scratch_function(&mut codegen, |_, builder| {
        let scalar_zero = DirectType::Scalar(ScalarKind::Int32).zero_values(builder);
        let opaque_zero = DirectType::Opaque(Type::named("str")).zero_values(builder);
        let class_zero = flat_class.zero_values(builder);
        assert_eq!(scalar_zero.len(), 1);
        assert_eq!(opaque_zero.len(), 1);
        assert_eq!(class_zero.len(), 2);
        builder.return_(&[]);
    });

    assert!(is_numeric_type_name(&Type::named("uint64")));
    assert!(is_numeric_type_name(&Type::named("float32")));
    assert!(!is_numeric_type_name(&Type::named("str")));
    assert!(!is_numeric_type_name(&Type::Named(
        "list".to_string(),
        vec![Type::named("int32")],
    )));

    assert!(runtime_type_is_wildcard(&Type::TypeParam("T".to_string())));
    assert!(runtime_type_is_wildcard(&Type::named("Unknown")));
    assert!(runtime_type_is_wildcard(&Type::Named(
        "dict".to_string(),
        vec![Type::named("str"), Type::named("Unknown")],
    )));
    assert!(!runtime_type_is_wildcard(&Type::Named(
        "list".to_string(),
        vec![Type::named("int32")],
    )));
    assert!(!runtime_type_is_wildcard(&Type::Unit));

    let source = "def main() -> int32:\n    return 0\n";
    let mir = lower_source_to_mir(source).expect("simple source should lower");
    let object = emit_host_object_with_metadata(&mir, "/tmp/demo.au", source)
        .expect("metadata-backed object emission should succeed");
    assert!(!object.is_empty());

    let invalid_module = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            source_path: None,
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: Vec::new(),
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: Vec::new(),
                terminator: Terminator::Unreachable,
            }],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };
    emit_host_object(&invalid_module).expect("an unreachable terminator compiles to a trap");
}

#[test]
fn native_codegen_direct_types_keep_recursive_and_generic_classes_opaque() {
    let classes = HashMap::from([
        (
            "Node".to_string(),
            crate::mir::MirClass {
                name: "Node".to_string(),
                type_params: Vec::new(),
                fields: vec![crate::mir::MirClassField {
                    name: "next".to_string(),
                    ty: Type::named("Node"),
                }],
                methods: Vec::new(),
                copy: false,
            },
        ),
        (
            "Box".to_string(),
            crate::mir::MirClass {
                name: "Box".to_string(),
                type_params: vec!["T".to_string()],
                fields: vec![crate::mir::MirClassField {
                    name: "value".to_string(),
                    ty: Type::TypeParam("T".to_string()),
                }],
                methods: Vec::new(),
                copy: false,
            },
        ),
    ]);

    assert_eq!(
        direct_type(&Type::named("Node"), &classes),
        Some(DirectType::Opaque(Type::named("Node"))),
        "recursive plain classes should fall back to opaque values"
    );
    assert_eq!(
        direct_type(
            &Type::Named("Box".to_string(), vec![Type::named("int32")]),
            &classes
        ),
        Some(DirectType::Opaque(Type::Named(
            "Box".to_string(),
            vec![Type::named("int32")],
        ))),
        "generic classes should stay opaque in direct type inference"
    );
}

#[test]
fn native_codegen_type_helpers_cover_nested_type_params_and_opaque_fallbacks() {
    let classes = HashMap::from([
        (
            "Pair".to_string(),
            crate::mir::MirClass {
                name: "Pair".to_string(),
                type_params: Vec::new(),
                fields: vec![
                    crate::mir::MirClassField {
                        name: "left".to_string(),
                        ty: Type::named("int32"),
                    },
                    crate::mir::MirClassField {
                        name: "right".to_string(),
                        ty: Type::named("bool"),
                    },
                ],
                methods: Vec::new(),
                copy: false,
            },
        ),
        (
            "Wrapper".to_string(),
            crate::mir::MirClass {
                name: "Wrapper".to_string(),
                type_params: Vec::new(),
                fields: vec![crate::mir::MirClassField {
                    name: "value".to_string(),
                    ty: Type::named("str"),
                }],
                methods: Vec::new(),
                copy: false,
            },
        ),
    ]);

    let mut collected = BTreeSet::new();
    collect_type_params_from_type(
        &Type::Named(
            "dict".to_string(),
            vec![
                Type::TypeParam("K".to_string()),
                Type::Named("list".to_string(), vec![Type::TypeParam("V".to_string())]),
            ],
        ),
        &mut collected,
    );
    assert_eq!(
        collected,
        BTreeSet::from(["K".to_string(), "V".to_string()])
    );

    assert_eq!(
        direct_type(&Type::TypeParam("T".to_string()), &classes),
        Some(DirectType::Opaque(Type::TypeParam("T".to_string())))
    );
    assert_eq!(
        direct_type(&Type::Module("pkg.tools".to_string()), &classes),
        Some(DirectType::Opaque(Type::Module("pkg.tools".to_string())))
    );
    assert_eq!(
        direct_type(&Type::named("float32"), &classes),
        Some(DirectType::Scalar(ScalarKind::Float32))
    );
    assert_eq!(
        direct_type(&Type::named("External"), &classes),
        Some(DirectType::Opaque(Type::named("External")))
    );

    let pair = direct_type(&Type::named("Pair"), &classes).expect("Pair should stay plain");
    assert_eq!(render_direct_type(&pair), "Pair");
    assert_eq!(pair.value_count(), 2);

    assert_eq!(
        direct_type(&Type::named("Wrapper"), &classes),
        Some(DirectType::Opaque(Type::named("Wrapper"))),
        "classes with opaque fields should fall back to opaque direct values"
    );
}

#[test]
fn validate_function_rejects_unreachable_terminators_for_direct_backend() {
    let function = MirFunction {
        name: "main".to_string(),
        module_name: "<test>".to_string(),
        source_path: None,
        span: Span::new(1, 1),
        receiver: None,
        params: vec![MirParam {
            name: "value".to_string(),
            passing: MirReceiverKind::Value,
            ty: Type::named("int32"),
            default_function: None,
            keyword_only: false,
        }],
        local_types: vec![MirLocalType {
            name: "value".to_string(),
            ty: Type::named("int32"),
        }],
        return_type: Type::named("int32"),
        entry: "entry".to_string(),
        blocks: vec![BasicBlock {
            label: "entry".to_string(),
            instructions: vec![
                Instruction::Assign {
                    target: "%t0".to_string(),
                    value: Rvalue::FormatString {
                        parts: vec![
                            crate::mir::MirFormatPart::Literal("value=".to_string()),
                            crate::mir::MirFormatPart::Value(Operand::Place("value".to_string())),
                        ],
                    },
                },
                Instruction::Assign {
                    target: "%t1".to_string(),
                    value: Rvalue::VecLiteral {
                        elements: vec![Operand::Int(1)],
                        element_type: Type::named("int32"),
                    },
                },
                Instruction::Assign {
                    target: "%t2".to_string(),
                    value: Rvalue::MapLiteral {
                        entries: vec![MirMapEntry {
                            key: Operand::String("a".to_string()),
                            value: Operand::Int(1),
                        }],
                        key_type: Type::named("str"),
                        value_type: Type::named("int32"),
                    },
                },
            ],
            terminator: Terminator::Unreachable,
        }],
    };

    validate_function(&function, &HashMap::new())
        .expect("the direct backend compiles an unreachable terminator as a trap");
}

#[test]
fn direct_codegen_ignores_dead_cleanup_and_terminator_metadata_but_checks_cfg_structure() {
    let function = MirFunction {
        name: "main".to_string(),
        module_name: "<test>".to_string(),
        source_path: None,
        span: Span::new(1, 1),
        receiver: None,
        params: Vec::new(),
        local_types: Vec::new(),
        return_type: Type::named("int32"),
        entry: "entry".to_string(),
        blocks: vec![
            BasicBlock {
                label: "entry".to_string(),
                instructions: Vec::new(),
                terminator: Terminator::Return(Operand::Int(0)),
            },
            BasicBlock {
                label: "dead".to_string(),
                instructions: vec![Instruction::PushCleanup {
                    place: "missing".to_string(),
                    action: crate::mir::CleanupAction::ResourceClose,
                    callee: None,
                }],
                terminator: Terminator::Unreachable,
            },
        ],
    };
    let module = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![function.clone()],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };

    validate_function(&function, &HashMap::new())
        .expect("unreachable backend-only metadata must not be validated as executable MIR");
    emit_host_object(&module)
        .expect("dead cleanup discovery must not declare or define an unreachable cleanup thunk");

    let mut malformed = module;
    malformed.functions[0].blocks[1].terminator = Terminator::Goto("missing".to_string());
    let error = emit_host_object(&malformed)
        .expect_err("structural validation must still inspect successors on dead blocks");
    assert!(
        error.contains("targets unknown block `missing`")
            || error.contains("branches to unknown block `missing`"),
        "unexpected structural CFG diagnostic: {error}"
    );
}

#[test]
fn native_codegen_constructor_initializes_runtime_function_surface() {
    let module = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            source_path: None,
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: Vec::new(),
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: Vec::new(),
                terminator: Terminator::Return(Operand::Int(0)),
            }],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };

    let codegen = super::NativeCodegen::new(
        &module,
        "/tmp/direct_constructor.au",
        "def main() -> int32:\n    return 0\n",
    )
    .expect("direct codegen constructor should initialize runtime symbols");

    assert_eq!(codegen.program_path, "/tmp/direct_constructor.au");
    assert!(codegen.program_source.contains("return 0"));
    assert!(codegen.classes.is_empty());
    assert!(codegen.method_receivers.is_empty());
    assert!(codegen.string_data.is_empty());
    assert!(codegen.functions.contains_key("main"));
    assert!(codegen.function_return_types.contains_key("main"));
    assert!(codegen.function_param_types.contains_key("main"));
    assert!(codegen.function_writeback_types.contains_key("main"));
}

#[test]
fn native_codegen_constructor_tracks_receiver_and_writeback_types_for_methods_and_top_level() {
    let source = r#"
class Counter:
    value: int32

    def sync_into(mut self, other: mut Counter, amount: int32):
        self.value += amount
        other.value = self.value

mut left: Counter = Counter(value=1)
mut right: Counter = Counter(value=0)
left.sync_into(other=right, amount=2)
"#;
    let mir = lower_source_to_mir(source).expect("source should lower to MIR");
    let method = mir
        .functions
        .iter()
        .find(|function| function.receiver == Some(MirReceiverKind::BorrowMut))
        .expect("borrow-mut method should lower into a function");
    let top_level = mir
        .top_level
        .as_ref()
        .expect("top-level script should lower into a top-level entry function");

    let codegen = super::NativeCodegen::new(&mir, "/tmp/direct_constructor_writebacks.au", source)
        .expect("direct codegen constructor should initialize runtime symbols");

    let method_params = codegen
        .function_param_types
        .get(&method.name)
        .expect("method param metadata should be registered");
    assert_eq!(method_params.len(), 3);
    assert!(matches!(method_params[0], DirectType::PlainClass(_)));
    assert!(matches!(method_params[1], DirectType::PlainClass(_)));
    assert_eq!(method_params[2], DirectType::Scalar(ScalarKind::Int32));

    let method_writebacks = codegen
        .function_writeback_types
        .get(&method.name)
        .expect("method writeback metadata should be registered");
    assert_eq!(method_writebacks.len(), 2);
    assert!(matches!(method_writebacks[0], DirectType::PlainClass(_)));
    assert!(matches!(method_writebacks[1], DirectType::PlainClass(_)));

    assert!(codegen.functions.contains_key(&top_level.name));
    assert!(codegen.function_thunks.contains_key(&top_level.name));
}

#[test]
fn native_codegen_replace_nested_field_rejects_empty_paths_without_panicking() {
    let error = super::split_field_path_segments(&[])
        .expect_err("empty field paths should surface an internal diagnostic");
    assert!(error.contains("empty field path"));
}

/// `function` with each cleanup calling the class method `exit`.
fn with_cleanup_method(mut function: MirFunction, exit: &str) -> MirFunction {
    for block in &mut function.blocks {
        for instruction in &mut block.instructions {
            if let Instruction::PushCleanup { callee, .. } = instruction {
                *callee = Some(crate::mir::CleanupCallee::Method {
                    function: exit.to_string(),
                });
            }
        }
    }
    function
}

fn cleanup_test_function(local_name: &str, ty: Type, place: &str) -> MirFunction {
    MirFunction {
        name: "main".to_string(),
        module_name: "<test>".to_string(),
        source_path: None,
        span: Span::new(1, 1),
        receiver: None,
        params: Vec::new(),
        local_types: vec![MirLocalType {
            name: local_name.to_string(),
            ty,
        }],
        return_type: Type::named("int32"),
        entry: "entry".to_string(),
        blocks: vec![BasicBlock {
            label: "entry".to_string(),
            instructions: vec![Instruction::PushCleanup {
                place: place.to_string(),
                action: crate::mir::CleanupAction::ResourceClose,
                callee: None,
            }],
            terminator: Terminator::Return(Operand::Int(0)),
        }],
    }
}

fn cleanup_test_module(functions: Vec<MirFunction>) -> crate::mir::MirModule {
    crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions,
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    }
}

fn class_field(name: &str, ty: Type) -> crate::mir::MirClassField {
    crate::mir::MirClassField {
        name: name.to_string(),
        ty,
    }
}

fn close_method(function_name: &str) -> crate::mir::MirMethod {
    crate::mir::MirMethod {
        name: "close".to_string(),
        function_name: function_name.to_string(),
        receiver: Some(MirReceiverKind::BorrowMut),
    }
}

fn close_function(class_name: &str) -> MirFunction {
    MirFunction {
        name: format!("{class_name}.close"),
        module_name: "<test>".to_string(),
        source_path: None,
        span: Span::new(1, 1),
        receiver: Some(MirReceiverKind::BorrowMut),
        params: Vec::new(),
        local_types: vec![MirLocalType {
            name: "self".to_string(),
            ty: Type::named(class_name),
        }],
        return_type: Type::Unit,
        entry: "entry".to_string(),
        blocks: vec![BasicBlock {
            label: "entry".to_string(),
            instructions: Vec::new(),
            terminator: Terminator::Return(Operand::Unit),
        }],
    }
}

#[test]
fn dead_assignments_do_not_poison_reachable_cleanup_type_inference() {
    let main = MirFunction {
        name: "main".to_string(),
        module_name: "<test>".to_string(),
        source_path: None,
        span: Span::new(1, 1),
        receiver: None,
        params: Vec::new(),
        local_types: vec![MirLocalType {
            name: "resource".to_string(),
            ty: Type::named("Resource"),
        }],
        return_type: Type::named("int32"),
        entry: "entry".to_string(),
        blocks: vec![
            BasicBlock {
                label: "dead".to_string(),
                instructions: vec![Instruction::Assign {
                    target: "resource".to_string(),
                    value: Rvalue::Use(Operand::Int(0)),
                }],
                terminator: Terminator::Return(Operand::Int(0)),
            },
            BasicBlock {
                label: "entry".to_string(),
                instructions: vec![
                    Instruction::Assign {
                        target: "resource".to_string(),
                        value: Rvalue::Construct {
                            class_name: "Resource".to_string(),
                            fields: vec![crate::mir::MirFieldInit {
                                name: "value".to_string(),
                                value: Operand::Int(1),
                            }],
                        },
                    },
                    Instruction::PushCleanup {
                        place: "resource".to_string(),
                        action: crate::mir::CleanupAction::ResourceClose,
                        callee: None,
                    },
                ],
                terminator: Terminator::AssertFail {
                    message: Some(Operand::String("boom".to_string())),
                    captures: Vec::new(),
                    span: Span::new(1, 1),
                },
            },
        ],
    };
    let close = close_function("Resource");
    let module = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![main, close],
        classes: vec![crate::mir::MirClass {
            name: "Resource".to_string(),
            type_params: Vec::new(),
            fields: vec![class_field("value", Type::named("int64"))],
            methods: vec![close_method("Resource.close")],
            copy: false,
        }],
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };

    let object = emit_unoptimized_object(&module)
        .expect("dead scalar assignments must not erase reachable resource cleanup metadata");
    let referenced = object_referenced_symbols(&object);
    let close_symbol = mangle_symbol("Resource.close");
    assert!(
        referenced
            .iter()
            .any(|symbol| symbol.trim_start_matches('_') == close_symbol),
        "reachable cleanup thunk must still call `{close_symbol}`: {referenced:?}"
    );
}

#[test]
fn native_codegen_cleanup_thunks_cover_scalar_plain_opaque_and_metadata_errors() {
    let scalar_function = cleanup_test_function("count", Type::named("int32"), "count");
    let scalar_module = cleanup_test_module(vec![scalar_function.clone()]);
    let mut scalar_codegen =
        super::NativeCodegen::new(&scalar_module, "/tmp/direct_scalar_cleanup.au", "")
            .expect("scalar cleanup test codegen should initialize");
    scalar_codegen
        .define_cleanup_thunk(&scalar_function, "count")
        .expect("scalar cleanup thunks should return unit");

    let mut missing_thunk_codegen =
        super::NativeCodegen::new(&scalar_module, "/tmp/direct_missing_cleanup.au", "")
            .expect("missing cleanup test codegen should initialize");
    missing_thunk_codegen
        .cleanup_thunks
        .remove(&("main".to_string(), "count".to_string()));
    let missing_thunk_error = missing_thunk_codegen
        .define_cleanup_thunk(&scalar_function, "count")
        .expect_err("missing cleanup thunk metadata should fail");
    assert!(missing_thunk_error.contains("could not find cleanup thunk for `count` in `main`"));

    let unknown_field_id = *missing_thunk_codegen
        .cleanup_thunks
        .entry(("main".to_string(), "count.missing".to_string()))
        .or_insert_with(|| {
            scalar_codegen.cleanup_thunks[&("main".to_string(), "count".to_string())]
        });
    missing_thunk_codegen.cleanup_thunks.insert(
        ("main".to_string(), "count.missing".to_string()),
        unknown_field_id,
    );
    let unknown_field_error = missing_thunk_codegen
        .define_cleanup_thunk(&scalar_function, "count.missing")
        .expect_err("unknown cleanup fields should fail before code emission");
    assert!(unknown_field_error.contains("does not know cleanup field `missing`"));

    let plain_function = cleanup_test_function("resource", Type::named("Plain"), "resource");
    let plain_module = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![plain_function.clone()],
        classes: vec![crate::mir::MirClass {
            name: "Plain".to_string(),
            type_params: Vec::new(),
            fields: vec![class_field("value", Type::named("int32"))],
            methods: Vec::new(),
            copy: false,
        }],
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };
    let mut plain_codegen =
        super::NativeCodegen::new(&plain_module, "/tmp/direct_plain_cleanup.au", "")
            .expect("plain cleanup test codegen should initialize");
    plain_codegen
        .define_cleanup_thunk(&plain_function, "resource")
        .expect("plain-class cleanup without close should return unit");

    let opaque_function = cleanup_test_function("text", Type::named("str"), "text");
    let opaque_module = cleanup_test_module(vec![opaque_function.clone()]);
    let mut opaque_codegen =
        super::NativeCodegen::new(&opaque_module, "/tmp/direct_opaque_cleanup.au", "")
            .expect("opaque cleanup test codegen should initialize");
    opaque_codegen
        .define_cleanup_thunk(&opaque_function, "text")
        .expect("opaque cleanup without custom close should call runtime close");
}

#[test]
fn native_codegen_cleanup_thunks_cover_class_close_success_and_missing_targets() {
    let plain_close_function = with_cleanup_method(
        cleanup_test_function("resource", Type::named("Resource"), "resource"),
        "Resource.close",
    );
    let resource_close = close_function("Resource");
    let plain_close_module = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![plain_close_function.clone(), resource_close],
        classes: vec![crate::mir::MirClass {
            name: "Resource".to_string(),
            type_params: Vec::new(),
            fields: vec![class_field("closed", Type::named("bool"))],
            methods: vec![close_method("Resource.close")],
            copy: false,
        }],
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };
    let mut plain_close_codegen = super::NativeCodegen::new(
        &plain_close_module,
        "/tmp/direct_plain_close_cleanup.au",
        "",
    )
    .expect("plain close cleanup codegen should initialize");
    plain_close_codegen
        .define_cleanup_thunk(&plain_close_function, "resource")
        .expect("plain-class cleanup should call a close method when available");

    let mut missing_target_codegen = super::NativeCodegen::new(
        &plain_close_module,
        "/tmp/direct_missing_close_cleanup.au",
        "",
    )
    .expect("missing close target codegen should initialize");
    missing_target_codegen.functions.remove("Resource.close");
    let missing_target_error = missing_target_codegen
        .define_cleanup_thunk(&plain_close_function, "resource")
        .expect_err("missing plain-class close targets should be reported");
    assert!(missing_target_error.contains("could not find cleanup close method `Resource.close`"));

    let opaque_close_function = with_cleanup_method(
        cleanup_test_function("managed", Type::named("Managed"), "managed"),
        "Managed.close",
    );
    let managed_close = close_function("Managed");
    let opaque_close_module = crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![opaque_close_function.clone(), managed_close],
        classes: vec![crate::mir::MirClass {
            name: "Managed".to_string(),
            type_params: Vec::new(),
            fields: vec![class_field("handle", Type::named("str"))],
            methods: vec![close_method("Managed.close")],
            copy: false,
        }],
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };
    let mut opaque_close_codegen = super::NativeCodegen::new(
        &opaque_close_module,
        "/tmp/direct_opaque_close_cleanup.au",
        "",
    )
    .expect("opaque close cleanup codegen should initialize");
    opaque_close_codegen
        .define_cleanup_thunk(&opaque_close_function, "managed")
        .expect("opaque cleanup should call a custom close method when available");

    let mut missing_opaque_target_codegen = super::NativeCodegen::new(
        &opaque_close_module,
        "/tmp/direct_missing_opaque_close.au",
        "",
    )
    .expect("missing opaque close target codegen should initialize");
    missing_opaque_target_codegen
        .functions
        .remove("Managed.close");
    let missing_opaque_target_error = missing_opaque_target_codegen
        .define_cleanup_thunk(&opaque_close_function, "managed")
        .expect_err("missing opaque close targets should be reported");
    assert!(
        missing_opaque_target_error.contains("could not find cleanup close method `Managed.close`")
    );
}

#[test]
fn native_codegen_thunks_cover_float_bool_plain_class_params_and_unit_main_wrapper() {
    let thunk_source = r#"
class Pair:
    left: int32
    right: bool

def helper(value: float64, flag: bool, pair: Pair) -> float64:
    if flag:
        return value
    return 0.0

def main() -> int32:
    return 0
"#;
    let thunk_mir = lower_source_to_mir(thunk_source).expect("source should lower to MIR");
    let helper = thunk_mir
        .functions
        .iter()
        .find(|function| function.name == "helper")
        .expect("helper function should be lowered");
    let mut thunk_codegen = super::NativeCodegen::new(
        &thunk_mir,
        "/tmp/direct_thunk_float_bool_pair.au",
        thunk_source,
    )
    .expect("codegen should initialize");
    thunk_codegen
        .define_function_thunk(helper)
        .expect("thunk generation should support float, bool, and plain-class parameters");

    let wrapper_source = "def main():\n    pass\n";
    let wrapper_mir = lower_source_to_mir(wrapper_source).expect("unit main should lower");
    let mut wrapper_codegen = super::NativeCodegen::new(
        &wrapper_mir,
        "/tmp/direct_unit_main_wrapper.au",
        wrapper_source,
    )
    .expect("codegen should initialize");
    wrapper_codegen
        .define_main_wrapper()
        .expect("main wrapper should support unit-return entrypoints");
}

#[test]
fn direct_assertions_reference_the_dedicated_failure_helper() {
    let source = r#"def main() -> int32:
    assert false, "direct assertion"
    return 0
"#;
    let mir = lower_source_to_mir(source).expect("assertion source should lower");
    let object = emit_host_object_with_metadata(&mir, "/tmp/direct_assert.au", source)
        .expect("assertion source should compile directly");
    let referenced = object_referenced_symbols(&object);
    assert!(
        referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_assert_fail")),
        "assert failures must use the diagnostic-preserving runtime helper: {referenced:?}"
    );
}

#[test]
fn direct_introspected_assertions_reference_the_detailed_failure_helper() {
    let source = r#"def main() -> int32:
    left = 41
    right = 42
    assert left == right, "direct assertion"
    return 0
"#;
    let mir = lower_source_to_mir(source).expect("assertion source should lower");
    let object = emit_host_object_with_metadata(&mir, "/tmp/direct_detailed_assert.au", source)
        .expect("introspected assertion source should compile directly");
    let referenced = object_referenced_symbols(&object);
    assert!(
        referenced
            .iter()
            .any(|symbol| symbol.contains("aura_direct_assert_fail_detailed")),
        "introspected assertion failures must use the detailed runtime helper: {referenced:?}"
    );
}

#[test]
fn direct_validation_accepts_assert_fail_operands_and_rejects_unknown_places() {
    let make_module = |message, captures| crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            source_path: None,
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: Vec::new(),
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: Vec::new(),
                terminator: Terminator::AssertFail {
                    message,
                    captures,
                    span: Span::new(2, 5),
                },
            }],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };

    emit_host_object(&make_module(
        Some(Operand::String("known".to_string())),
        Vec::new(),
    ))
    .expect("literal assertion messages should validate");
    let error = emit_host_object(&make_module(
        Some(Operand::Place("missing".to_string())),
        Vec::new(),
    ))
    .expect_err("unknown assertion message places should be rejected");
    assert!(
        error.contains("does not know local `missing`"),
        "unexpected validation error: {error}"
    );

    let captures = vec![
        AssertionCapture {
            label: "left".to_string(),
            ty: Type::named("int64"),
            value: Operand::String("41".to_string()),
        },
        AssertionCapture {
            label: "right".to_string(),
            ty: Type::named("int64"),
            value: Operand::Place("missing_capture".to_string()),
        },
    ];
    let error = emit_host_object(&make_module(None, captures))
        .expect_err("unknown assertion capture places should be rejected");
    assert!(
        error.contains("does not know local `missing_capture`"),
        "unexpected capture validation error: {error}"
    );

    let error = emit_host_object(&make_module(
        None,
        vec![AssertionCapture {
            label: "only".to_string(),
            ty: Type::named("int64"),
            value: Operand::String("1".to_string()),
        }],
    ))
    .expect_err("assertion captures must be absent or form a pair");
    assert!(
        error.contains("exactly two assertion captures"),
        "unexpected capture cardinality error: {error}"
    );

    let error = emit_host_object(&make_module(
        None,
        vec![
            AssertionCapture {
                label: "left".to_string(),
                ty: Type::named("int64"),
                value: Operand::Int(1),
            },
            AssertionCapture {
                label: "right".to_string(),
                ty: Type::named("int64"),
                value: Operand::String("2".to_string()),
            },
        ],
    ))
    .expect_err("assertion capture values must already be rendered strings");
    assert!(
        error.contains("rendered assertion capture to be `str`, found `int64`"),
        "unexpected capture type error: {error}"
    );
}

#[test]
fn adr0038_direct_return_projection_selection_and_tuple_errors_are_codegen_checked() {
    let returned_module = lower_source_to_mir(
        r#"
class Pair:
    left: int64
    right: int64

def choose(pair: mut Pair, left: bool) -> view mut int64 from pair:
    if left:
        return view mut pair.left
    return view mut pair.right

def main():
    mut pair = Pair(left=1, right=2)
    view mut selected = choose(pair, false)
    selected = 9
"#,
    )
    .expect("dynamic returned-view source should lower before direct codegen");
    emit_host_object(&returned_module)
        .expect("dynamic returned-view projection selection should compile");

    let malformed_tuple_module = |projection: &str| crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions: vec![MirFunction {
            name: format!("tuple_projection_{projection}"),
            module_name: "<test>".to_string(),
            source_path: None,
            span: Span::new(1, 1),
            receiver: None,
            params: vec![MirParam {
                name: "origin".to_string(),
                passing: MirReceiverKind::Borrow,
                ty: Type::Tuple(vec![Type::named("int64"), Type::named("int64")]),
                default_function: None,
                keyword_only: false,
            }],
            local_types: vec![MirLocalType {
                name: "target".to_string(),
                ty: Type::named("int64"),
            }],
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: vec![
                    Instruction::BeginLoan {
                        loan: "selected".to_string(),
                        source: format!("origin.{projection}"),
                        mutable: false,
                    },
                    Instruction::ReadLoan {
                        target: "target".to_string(),
                        loan: "selected".to_string(),
                    },
                    Instruction::EndLoan {
                        loan: "selected".to_string(),
                    },
                ],
                terminator: Terminator::Return(Operand::Int(0)),
            }],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    };
    let invalid = emit_host_object(&malformed_tuple_module("invalid"))
        .expect_err("tuple view projections must be fixed positions");
    assert!(invalid.contains("is not a fixed position"), "{invalid}");
    let out_of_bounds = emit_host_object(&malformed_tuple_module("4"))
        .expect_err("tuple view projections must remain in bounds");
    assert!(
        out_of_bounds.contains("tuple projection `4`")
            && out_of_bounds.contains("is out of bounds"),
        "{out_of_bounds}"
    );
}

#[test]
fn native_codegen_release_helpers_release_owning_union_handles_by_tag() {
    let source = "def main() -> int32:\n    return 0\n";
    let mir = lower_source_to_mir(source).expect("release helper source should lower");
    let mut codegen = NativeCodegen::new(&mir, "/tmp/release_owning_union.au", source)
        .expect("codegen should initialize");

    with_scratch_function(&mut codegen, |codegen, builder| {
        let text = DirectType::Opaque(Type::named("str"));
        let optional_text = super::DirectUnionType::new(
            crate::sema::optional_type(Type::named("str")),
            vec![text.clone(), DirectType::Scalar(ScalarKind::Unit)],
        );
        assert!(optional_text.owns_handles());
        assert_eq!(optional_text.owning, vec![0]);
        let inline_pair = super::DirectUnionType::new(
            crate::sema::optional_type(Type::named("int64")),
            vec![
                DirectType::Scalar(ScalarKind::Int64),
                DirectType::Scalar(ScalarKind::Unit),
            ],
        );
        assert!(!inline_pair.owns_handles());

        let tag = builder.iconst(types::I64, 0);
        let handle = builder.iconst(types::I64, 7);
        let instructions_before = builder.instruction_count();
        release_direct_values(
            codegen,
            builder,
            &[tag, handle],
            &DirectType::Union(optional_text),
        )
        .expect("an owning union releases its active handle under a tag switch");
        let owning_instructions = builder.instruction_count() - instructions_before;
        assert!(
            owning_instructions >= 4,
            "the owning member needs a compare, a branch, a release call, and a jump"
        );

        let instructions_before = builder.instruction_count();
        release_direct_values(
            codegen,
            builder,
            &[tag, handle],
            &DirectType::Union(inline_pair),
        )
        .expect("an all-inline union releases nothing");
        assert_eq!(builder.instruction_count(), instructions_before);

        builder.return_(&[]);
    });
}

#[test]
fn direct_union_member_calls_report_clone_arity_and_unknown_results() {
    let optional_int = crate::sema::optional_type(Type::named("int64"));
    let Type::Union(union) = optional_int.clone() else {
        panic!("optional int64 is a union");
    };
    let inject = Rvalue::UnionInject {
        value: Operand::Int(4),
        union_type: optional_int.clone(),
        member_type: Type::named("int64"),
        member_index: 0,
    };
    let mut clone_with_argument = module_with_main_member_call_result_type(
        "maybe",
        optional_int.clone(),
        inject.clone(),
        optional_int.clone(),
        "clone",
        vec![MirArg {
            name: None,
            value: Operand::Int(1),
            writeback_place: None,
        }],
    );
    clone_with_argument.unions = vec![crate::union_layout::plan_union_layout(&union, |_| true)];
    let clone_arity_error =
        emit_host_object(&clone_with_argument).expect_err("a union clone takes no arguments");
    assert!(
        clone_arity_error.contains("`clone` does not take arguments"),
        "{clone_arity_error}"
    );

    let mut unknown_method = module_with_main_member_call_result_type(
        "maybe",
        optional_int.clone(),
        inject,
        Type::named("int32"),
        "speak",
        Vec::new(),
    );
    unknown_method.unions = vec![crate::union_layout::plan_union_layout(&union, |_| true)];
    let unknown_method_error =
        emit_host_object(&unknown_method).expect_err("scalar union members have no methods");
    assert!(
        unknown_method_error.contains("speak"),
        "{unknown_method_error}"
    );
}

#[test]
fn direct_runtime_member_calls_report_unknown_members_and_to_float_arity() {
    let unknown_member = module_with_main_member_call_result_type(
        "text",
        Type::named("str"),
        Rvalue::Use(Operand::String("probe".to_string())),
        Type::named("int32"),
        "bogus",
        Vec::new(),
    );
    let unknown_member_error = emit_host_object(&unknown_member)
        .expect_err("an unknown runtime member cannot be compiled");
    assert!(
        unknown_member_error.contains("runtime has no member `str.bogus`"),
        "{unknown_member_error}"
    );

    let to_float_with_argument = module_with_main_member_call_result_type(
        "wide",
        Type::named("int128"),
        Rvalue::Use(Operand::Int(7)),
        Type::named("float64"),
        "to_float",
        vec![MirArg {
            name: None,
            value: Operand::Int(1),
            writeback_place: None,
        }],
    );
    let arity_error =
        emit_host_object(&to_float_with_argument).expect_err("`to_float` takes no arguments");
    assert!(arity_error.contains("to_float"), "{arity_error}");
}

#[test]
fn direct_union_receiver_switch_refuses_a_member_without_the_method() {
    let source = "trait Greet:\n    def greet(self) -> str\n\nclass Dog:\n    legs: int64\n\nimpl Greet for Dog:\n    def greet(self) -> str:\n        return \"woof\"\n\ndef main():\n    pet: Dog | None = Dog(legs=4)\n    match pet:\n        case Dog as dog:\n            print(dog.greet())\n        case None:\n            print(\"none\")\n";
    let mut module = lower_source_to_mir(source).expect("union receiver source should lower");
    let main = module
        .functions
        .iter_mut()
        .find(|function| function.name == "main")
        .expect("main should lower");
    let union_local = main
        .local_types
        .iter()
        .find(|local| matches!(local.ty, Type::Union(_)))
        .map(|local| local.name.clone())
        .expect("main declares the union local");
    main.local_types.push(MirLocalType {
        name: "probe".to_string(),
        ty: Type::named("str"),
    });
    let entry_label = main.entry.clone();
    let entry = main
        .blocks
        .iter_mut()
        .find(|block| block.label == entry_label)
        .expect("main has an entry block");
    let call_index = entry
        .instructions
        .iter()
        .rposition(|instruction| {
            matches!(instruction, Instruction::Assign { target, .. } if *target == union_local)
        })
        .expect("the union local is assigned in the entry block")
        + 1;
    entry.instructions.insert(
        call_index,
        Instruction::Assign {
            target: "probe".to_string(),
            value: Rvalue::Call {
                callee: CallTarget::Member {
                    object: Operand::Place(union_local.clone()),
                    field: "greet".to_string(),
                    receiver_place: Some(union_local.clone()),
                    dispatch: None,
                },
                args: Vec::new(),
                type_args: Default::default(),
            },
        },
    );
    let error = crate::mir::methods::validate_methods(
        &module,
        crate::mir::methods::MethodsConsumer::Native,
    )
    .expect_err("the `None` member has no `greet`");
    assert!(
        error.contains("`.greet` in `main` resolves to no method: member `None` has no method"),
        "{error}"
    );
}

fn callable_signature(params: Vec<(&str, Type)>, return_type: Type) -> Type {
    Type::Function {
        params: params
            .into_iter()
            .map(|(name, ty)| crate::sema::FunctionParamContract {
                keyword_only: false,
                name: name.to_string(),
                ty,
                passing: crate::ast::ReceiverKind::Value,
                has_default: false,
            })
            .collect(),
        return_type: Box::new(return_type),
    }
}

#[test]
fn native_codegen_retain_and_release_helpers_cover_every_callable_bearing_shape() {
    let source = "def main() -> int32:\n    return 0\n";
    let mir = lower_source_to_mir(source).expect("release helper source should lower");
    let mut codegen = NativeCodegen::new(&mir, "/tmp/callable_release_helpers.au", source)
        .expect("codegen should initialize");
    with_scratch_function(&mut codegen, |codegen, builder| {
        let word = builder.iconst(types::I64, 0);
        let callable_ty = DirectType::Callable(super::DirectCallableType::new(callable_signature(
            vec![],
            Type::named("int64"),
        )));
        let text = DirectType::Opaque(Type::named("str"));
        let pair = DirectType::PlainClass(PlainClassType {
            class_name: "Pair".to_string(),
            fields: vec![
                PlainClassField {
                    name: "text".to_string(),
                    ty: text.clone(),
                },
                PlainClassField {
                    name: "count".to_string(),
                    ty: DirectType::Scalar(ScalarKind::Int64),
                },
            ],
        });
        let optional_callable = DirectType::Union(super::DirectUnionType::new(
            crate::sema::optional_type(callable_signature(vec![], Type::named("int64"))),
            vec![callable_ty.clone(), DirectType::Scalar(ScalarKind::Unit)],
        ));
        let optional_text = DirectType::Union(super::DirectUnionType::new(
            crate::sema::optional_type(Type::named("str")),
            vec![text.clone(), DirectType::Scalar(ScalarKind::Unit)],
        ));
        let plain_optional = DirectType::Union(super::DirectUnionType::new(
            crate::sema::optional_type(Type::named("int64")),
            vec![
                DirectType::Scalar(ScalarKind::Int64),
                DirectType::Scalar(ScalarKind::Unit),
            ],
        ));

        for (ty, count) in [
            (DirectType::Scalar(ScalarKind::Int64), 1),
            (text.clone(), 1),
            (pair.clone(), 2),
            (optional_text.clone(), 2),
            (optional_callable.clone(), 5),
            (plain_optional.clone(), 2),
            (callable_ty.clone(), 4),
        ] {
            let values = vec![word; count];
            super::callables::retain_direct_values(codegen, builder, &values, &ty).unwrap_or_else(
                |error| {
                    panic!(
                        "retain of {} should compile: {error}",
                        render_direct_type(&ty)
                    )
                },
            );
            release_direct_values(codegen, builder, &values, &ty).unwrap_or_else(|error| {
                panic!(
                    "release of {} should compile: {error}",
                    render_direct_type(&ty)
                )
            });
        }

        let retain_error = super::callables::retain_direct_values(codegen, builder, &[], &text)
            .expect_err("an opaque retain needs its handle");
        assert!(
            retain_error.contains("retain expected an opaque `str` value"),
            "{retain_error}"
        );
        let retain_error = super::callables::retain_direct_values(codegen, builder, &[word], &pair)
            .expect_err("a plain class retain needs every field word");
        assert!(
            retain_error.contains("retain expected `2` values for `Pair`"),
            "{retain_error}"
        );
        let retain_error =
            super::callables::retain_direct_values(codegen, builder, &[word], &optional_text)
                .expect_err("a union retain needs its payload word");
        assert!(
            retain_error.contains("retain expected union words"),
            "{retain_error}"
        );
        let adapter_error = super::callables::call_callable_unary_adapter(
            builder,
            &[word, word],
            super::callables::DESCRIPTOR_DROP,
        )
        .expect_err("a callable adapter call needs four words");
        assert!(
            adapter_error.contains("expected 4 callable words, found 2"),
            "{adapter_error}"
        );
        let box_error = super::callables::call_callable_box_adapter(builder, &[word])
            .expect_err("a callable box call needs four words");
        assert!(
            box_error.contains("expected 4 callable words, found 1"),
            "{box_error}"
        );
        let parts_error = super::callables::contract_parts(&Type::named("int64"))
            .expect_err("an int64 has no callable contract");
        assert!(
            parts_error.contains("expected a callable contract, found `int64`"),
            "{parts_error}"
        );

        builder.return_(&[]);
    });
}

fn shape_test_function(name: &str, param_names: &[&str]) -> MirFunction {
    MirFunction {
        name: name.to_string(),
        module_name: "<test>".to_string(),
        source_path: None,
        span: Span::new(1, 1),
        receiver: None,
        params: param_names
            .iter()
            .map(|param| crate::mir::MirParam {
                name: param.to_string(),
                passing: MirReceiverKind::Value,
                ty: Type::named("int64"),
                default_function: None,
                keyword_only: false,
            })
            .collect(),
        local_types: param_names
            .iter()
            .map(|param| MirLocalType {
                name: param.to_string(),
                ty: Type::named("int64"),
            })
            .collect(),
        return_type: Type::named("int64"),
        entry: "entry".to_string(),
        blocks: vec![BasicBlock {
            label: "entry".to_string(),
            instructions: Vec::new(),
            terminator: Terminator::Return(Operand::Int(0)),
        }],
    }
}

fn shape_test_module(main_value: Rvalue, helpers: Vec<MirFunction>) -> crate::mir::MirModule {
    let mut main = shape_test_function("main", &[]);
    main.local_types.push(MirLocalType {
        name: "callee".to_string(),
        ty: Type::named("int64"),
    });
    main.blocks[0].instructions.push(Instruction::Assign {
        target: "callee".to_string(),
        value: main_value,
    });
    let mut functions = vec![main];
    functions.extend(helpers);
    crate::mir::MirModule {
        unions: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        functions,
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
        ownership: Default::default(),
        borrowing: Default::default(),
    }
}

#[test]
fn direct_callable_shape_declaration_reports_unknown_functions_and_mismatched_captures_and_contracts(
) {
    let source = "def main() -> int32:\n    return 0\n";
    let mir = lower_source_to_mir(source).expect("shape declaration source should lower");
    let mut codegen = NativeCodegen::new(&mir, "/tmp/callable_shape_declaration.au", source)
        .expect("codegen should initialize");
    let classes = HashMap::new();
    let mut param_types = HashMap::new();
    param_types.insert(
        "helper".to_string(),
        vec![DirectType::Scalar(ScalarKind::Int64)],
    );
    let no_params = callable_signature(vec![], Type::named("int64"));
    let one_param = callable_signature(vec![("value", Type::named("int64"))], Type::named("int64"));

    let missing = shape_test_module(
        Rvalue::Use(Operand::Function {
            name: "missing".to_string(),
            signature: Box::new(no_params.clone()),
        }),
        Vec::new(),
    );
    let error = super::callables::declare_callable_shapes(
        &missing,
        codegen
            .module_backend()
            .expect("the module backend is idle"),
        &param_types,
        &classes,
    )
    .expect_err("a reference must name a lowered function");
    assert!(
        error.contains("cannot find lowered function `missing` for a callable shape"),
        "{error}"
    );

    let capture = |name: &str| crate::mir::MirClosureCapture {
        name: name.to_string(),
        value: Operand::Int(1),
        ty: Type::named("int64"),
        passing: MirReceiverKind::Value,
        mutated: false,
        source_place: None,
        resolve_source_at_capture: false,
    };
    let too_many_captures = shape_test_module(
        Rvalue::Closure {
            function: "helper".to_string(),
            signature: no_params.clone(),
            captures: vec![capture("a"), capture("b")],
            consuming: false,
            mutable: false,
        },
        vec![shape_test_function("helper", &["value"])],
    );
    let error = super::callables::declare_callable_shapes(
        &too_many_captures,
        codegen
            .module_backend()
            .expect("the module backend is idle"),
        &param_types,
        &classes,
    )
    .expect_err("a closure cannot capture more values than its function takes");
    assert!(
        error.contains("captures 2 values but the function takes 1"),
        "{error}"
    );

    let contract_mismatch = shape_test_module(
        Rvalue::Use(Operand::Function {
            name: "helper".to_string(),
            signature: Box::new(no_params.clone()),
        }),
        vec![shape_test_function("helper", &["value"])],
    );
    let error = super::callables::declare_callable_shapes(
        &contract_mismatch,
        codegen
            .module_backend()
            .expect("the module backend is idle"),
        &param_types,
        &classes,
    )
    .expect_err("a reference's contract must match the function's public parameters");
    assert!(
        error.contains("declares 1 public parameters but its contract `def() -> int64` has 0"),
        "{error}"
    );

    let matching = shape_test_module(
        Rvalue::Use(Operand::Function {
            name: "helper".to_string(),
            signature: Box::new(one_param),
        }),
        vec![shape_test_function("helper", &["value"])],
    );
    let shapes = super::callables::declare_callable_shapes(
        &matching,
        codegen
            .module_backend()
            .expect("the module backend is idle"),
        &param_types,
        &classes,
    )
    .expect("a matching contract declares its shape");
    assert_eq!(shapes.len(), 1);
    assert!(shapes
        .values()
        .all(|shape| shape.env_words == 0 && shape.inline()));

    let parts_error = super::callables::contract_direct_parts(&Type::named("int64"), &classes)
        .expect_err("an int64 has no callable contract");
    assert!(
        parts_error.contains("expected a callable contract, found `int64`"),
        "{parts_error}"
    );
}

#[test]
fn runtime_type_wildcards_are_found_inside_callable_contracts() {
    use crate::sema::{ClosureCallKind, ClosureCapture, ClosureCaptureMode};
    let concrete = |ty: Type| crate::sema::FunctionParamContract {
        keyword_only: false,
        name: "value".to_string(),
        ty,
        passing: crate::ast::ReceiverKind::Value,
        has_default: false,
    };
    let function = |param: Type, ret: Type| Type::Function {
        params: vec![concrete(param)],
        return_type: Box::new(ret),
    };
    assert!(!super::runtime_type_is_wildcard(&function(
        Type::named("int64"),
        Type::named("str")
    )));
    assert!(super::runtime_type_is_wildcard(&function(
        Type::TypeParam("T".to_string()),
        Type::named("str")
    )));
    assert!(super::runtime_type_is_wildcard(&function(
        Type::named("int64"),
        Type::named("Unknown")
    )));
    let view = Type::ReturnedView(Box::new(crate::sema::ReturnedViewType {
        mutable: false,
        pointee: Type::TypeParam("T".to_string()),
        origin: 0,
    }));
    assert!(super::runtime_type_is_wildcard(&view));
    let callable = |param: Type, ret: Type| {
        Type::Callable(Box::new(crate::sema::CallableType {
            task: false,
            call_kind: ClosureCallKind::Repeatable,
            params: vec![concrete(param)],
            return_type: ret,
        }))
    };
    assert!(!super::runtime_type_is_wildcard(&callable(
        Type::named("int64"),
        Type::named("int64")
    )));
    assert!(super::runtime_type_is_wildcard(&callable(
        Type::named("int64"),
        Type::TypeParam("R".to_string())
    )));
    let closure = |param: Type, capture: Type, ret: Type| Type::Closure {
        params: Box::new(vec![concrete(param)]),
        return_type: Box::new(ret),
        captures: Box::new(vec![ClosureCapture {
            name: "state".to_string(),
            ty: capture,
            mode: ClosureCaptureMode::MutableView,
            span: Span::new(1, 1),
            mutated: false,
        }]),
        call_kind: ClosureCallKind::Repeatable,
    };
    assert!(!super::runtime_type_is_wildcard(&closure(
        Type::named("int64"),
        Type::named("str"),
        Type::named("bool")
    )));
    assert!(super::runtime_type_is_wildcard(&closure(
        Type::named("int64"),
        Type::TypeParam("U".to_string()),
        Type::named("bool")
    )));
    assert!(super::runtime_type_is_wildcard(&closure(
        Type::named("int64"),
        Type::named("str"),
        Type::named("Unknown")
    )));
}

#[test]
fn adr0061_direct_view_place_projection_reaches_inside_an_element() {
    use super::{
        DirectElementKind, DirectElementSelector, DirectType, DirectViewPlace, ScalarKind,
    };
    let mut element = DirectViewPlace::static_place("items".to_string());
    element.alternatives[0]
        .elements
        .push(DirectElementSelector {
            variable: Var(9),
            selector_ty: DirectType::Scalar(ScalarKind::Int64),
            element_type: Type::named("Profile"),
            kind: DirectElementKind::List,
            projection: String::new(),
            span: crate::diag::Span::new(1, 1),
        });
    let projected = element.project("visits");
    assert_eq!(projected.alternatives[0].place, "items");
    assert_eq!(projected.alternatives[0].elements[0].projection, "visits");
    let nested = projected.project("inner");
    assert_eq!(
        nested.alternatives[0].elements[0].projection,
        "visits.inner"
    );
    assert_eq!(nested.clone().project("").alternatives, nested.alternatives);
}

#[test]
fn adr0061_direct_codegen_compiles_element_and_entry_loans() {
    let sources = [
        // List elements: scalar, string, class with projections, tuple
        // position, evaluate-once, literal-disjoint mutable views.
        r#"
class Profile:
    name: str
    visits: int64

def main():
    mut values = [10, 20, 30]
    view mut first = values[0]
    first += 1
    mut index = 1
    view mut selected = values[index]
    index = 2
    selected = 7
    view mut left = values[1]
    view mut right = values[2]
    left += 1
    right += 1
    mut names = ["ada", "linus"]
    view mut name = names[1]
    name = name + "!"
    mut users = [Profile(name="ada", visits=1), Profile(name="linus", visits=2)]
    view mut ada = users[0]
    ada.visits += 10
    view mut visits = users[1].visits
    visits *= 3
    view shared = users[0]
    print(shared.name)
    users[1].visits = 5
    users[1].visits += 1
    print(users[1].visits)
    pairs = [(1, "one"), (2, "two")]
    view label = pairs[1][1]
    print(label)
    print(values)
    print(names)
"#,
        // Dictionary entries: str, int64, and bool keys, class values with
        // projections, union values, and assignment through an entry.
        r#"
class Profile:
    name: str
    visits: int64

def main():
    mut counts: dict[str, int64] = {"ready": 1}
    key = "ready"
    view mut ready = counts[key]
    ready += 1
    mut flags: dict[bool, int64] = {true: 1}
    view mut yes = flags[true]
    yes += 1
    mut codes: dict[int64, str] = {7: "seven"}
    view seven = codes[7]
    print(seven)
    mut people: dict[str, Profile] = {"ada": Profile(name="ada", visits=0)}
    view mut ada = people["ada"]
    ada.visits += 3
    view mut visits = people["ada"].visits
    visits *= 2
    people["ada"].visits += 1
    print(people["ada"].visits)
    mut boxes: dict[str, int64 | str] = {"a": "x"}
    view mut slot = boxes["a"]
    slot = 9
    print(boxes)
    print(counts)
    print(flags)
"#,
        // Element loans through a loop binding, and nested element views
        // through an element or entry view (a chain of selections).
        r#"
def main():
    mut rows = [[1, 2], [3, 4]]
    for row in rows:
        view cell = row[1]
        print(cell)
    view mut row = rows[0]
    view mut cell = row[1]
    cell = 9
    mut table: dict[str, list[int64]] = {"a": [5, 6]}
    view mut entry = table["a"]
    view mut slot = entry[0]
    slot += 10
    print(rows)
    print(table)
"#,
    ];
    for source in sources {
        let module = lower_source_to_mir(source).expect("element view source should lower");
        emit_host_object(&module).expect("element and entry loans compile on the direct backend");
    }
}

#[test]
fn adr0061_direct_element_loans_preserve_returned_collection_alternatives() {
    let module = lower_source_to_mir(
        r#"
class Shelves:
    left: list[list[int64]]
    right: list[list[int64]]

class Ledgers:
    left: dict[int64, list[int64]]
    right: dict[int64, list[int64]]

def shelf(shelves: mut Shelves, left: bool) -> view mut list[list[int64]] from shelves:
    if left:
        return view mut shelves.left
    return view mut shelves.right

def ledger(ledgers: mut Ledgers, left: bool) -> view mut dict[int64, list[int64]] from ledgers:
    if left:
        return view mut ledgers.left
    return view mut ledgers.right

def update_shelf(shelves: mut Shelves, left: bool):
    view mut selected = shelf(shelves, left)
    view mut row = selected[0]
    view mut cell = row[1]
    print(cell)
    cell += 10
    print(cell)

def update_ledger(ledgers: mut Ledgers, left: bool):
    view mut selected = ledger(ledgers, left)
    view mut entry = selected[7]
    view mut cell = entry[0]
    print(cell)
    cell += 20
    print(cell)

def main():
    mut shelves = Shelves(left=[[1, 2]], right=[[3, 4]])
    update_shelf(shelves, true)
    update_shelf(shelves, false)
    print(shelves.left)
    print(shelves.right)
    mut ledgers = Ledgers(left={7: [5, 6]}, right={7: [7, 8]})
    update_ledger(ledgers, true)
    update_ledger(ledgers, false)
    print(ledgers.left)
    print(ledgers.right)
    view mut left_row = shelves.left[0]
    view mut first = left_row[0]
    first += 100
    print(first)
    mut pairs = ([8, 9], [10, 11])
    view mut pair = pairs[1]
    view mut item = pair[0]
    item += 30
    print(item)
    print(pairs)
    mut optional: Shelves | None = Shelves(left=[[1, 2]], right=[[3, 4]])
    match mut optional:
        case Shelves as present:
            view mut row = present.right[0]
            view mut last = row[1]
            last += 40
            print(last)
        case None:
            print("absent")
"#,
    )
    .expect("an element loan may select through either returned collection view");
    assert_eq!(
        crate::run_mir(&module)
            .expect("both selected collection alternatives should write through")
            .stdout,
        "2\n12\n4\n14\n[[1, 12]]\n[[3, 14]]\n5\n25\n7\n27\n{7: [25, 6]}\n{7: [27, 8]}\n101\n40\n([8, 9], [40, 11])\n44\n"
    );
    emit_host_object(&module)
        .expect("direct element loads and stores should compile for every selected alternative");
}

#[test]
fn type_parameter_collection_walks_callable_and_closure_contracts() {
    let param = |name: &str, ty: Type| crate::sema::FunctionParamContract {
        keyword_only: false,
        name: name.to_string(),
        ty,
        passing: crate::ast::ReceiverKind::Borrow,
        has_default: false,
    };
    let closure = Type::Closure {
        params: Box::new(vec![param("value", Type::TypeParam("A".to_string()))]),
        return_type: Box::new(Type::Tuple(vec![
            Type::TypeParam("R".to_string()),
            Type::named("int64"),
        ])),
        captures: Box::new(vec![crate::sema::ClosureCapture {
            name: "offset".to_string(),
            ty: Type::Named("list".to_string(), vec![Type::TypeParam("C".to_string())]),
            mode: crate::sema::ClosureCaptureMode::Copy,
            span: Span::new(1, 2),
            mutated: false,
        }]),
        call_kind: crate::sema::ClosureCallKind::Repeatable,
    };
    let mut collected = BTreeSet::new();
    collect_type_params_from_type(&closure, &mut collected);
    assert_eq!(
        collected,
        BTreeSet::from(["A".to_string(), "C".to_string(), "R".to_string()])
    );

    let function = Type::Function {
        params: vec![param("first", Type::TypeParam("P".to_string()))],
        return_type: Box::new(Type::TypeParam("Q".to_string())),
    };
    let mut collected = BTreeSet::new();
    collect_type_params_from_type(&function, &mut collected);
    assert_eq!(
        collected,
        BTreeSet::from(["P".to_string(), "Q".to_string()])
    );
}

#[test]
fn direct_cleanup_of_a_known_resource_type_uses_its_own_close_entry() {
    let source = r#"
def worker(events: Queue[str]):
    events.put("done")
    events.close()

def main() -> int32:
    events = Queue[str]()
    with TaskGroup() as group:
        group.start(worker, events)
        for event in events:
            print(event)
    return 0
"#;
    let mir = lower_source_to_mir(source).expect("task group source should lower to MIR");
    let object = emit_host_object(&mir).expect("task group source should compile directly");
    let referenced: BTreeSet<String> = object_referenced_symbols(&object)
        .into_iter()
        .map(|symbol| symbol.trim_start_matches('_').to_string())
        .collect();
    assert!(
        referenced.contains("aura_direct_close_task_group"),
        "task group cleanup must call its own close entry: {referenced:?}"
    );
    assert!(
        !referenced.contains("aura_direct_close_value"),
        "a statically typed cleanup must not use the generic close dispatcher: {referenced:?}"
    );
}

fn object_defined_aura_functions(bytes: &[u8]) -> BTreeSet<String> {
    let object =
        object::File::parse(bytes).expect("direct backend output should be a readable host object");
    object
        .symbols()
        .filter(|symbol| symbol.section_index().is_some())
        .filter_map(|symbol| {
            symbol
                .name()
                .ok()
                .map(|name| name.trim_start_matches('_').to_string())
        })
        .filter(|name| name.starts_with("aura_fn_"))
        .collect()
}

#[test]
fn direct_object_emits_only_the_builtin_wrappers_the_program_reaches() {
    let source = r#"
import toml

def unused() -> int32:
    return 1

def main() -> int32:
    print(toml.is_valid("a = 1"))
    return 0
"#;
    let mir = lower_source_to_mir(source).expect("toml source should lower to MIR");
    assert!(
        mir.functions
            .iter()
            .any(|function| function.name == "toml::parse_string_map"),
        "the lowered module should carry every toml wrapper"
    );
    let object = emit_unoptimized_object(&mir).expect("toml source should compile directly");
    let defined = object_defined_aura_functions(&object);
    assert!(defined.contains("aura_fn_main"), "{defined:?}");
    assert!(
        defined.contains("aura_fn_unused"),
        "program functions are always compiled so codegen checks them: {defined:?}"
    );
    assert!(
        defined
            .iter()
            .all(|name| !name.starts_with("aura_fn_json__")),
        "wrappers of modules the program never calls must not be emitted: {defined:?}"
    );
    assert!(
        !defined.contains("aura_fn_toml__parse_string_map"),
        "an uncalled wrapper of an imported module must not be emitted: {defined:?}"
    );
}

#[test]
fn reachability_keeps_builtin_wrappers_named_by_function_values() {
    let source = r#"
import toml

def main() -> int32:
    check = toml.is_valid
    print(check("a = 1"))
    return 0
"#;
    let mir = lower_source_to_mir(source).expect("function value source should lower");
    let pruned = super::reachability::prune_unreachable_functions(&mir);
    let names = pruned
        .functions
        .iter()
        .map(|function| function.name.as_str())
        .collect::<BTreeSet<_>>();
    assert!(names.contains("main"), "{names:?}");
    assert!(names.contains("toml::is_valid"), "{names:?}");
    assert!(!names.contains("toml::parse_string_map"), "{names:?}");
    emit_host_object(&mir).expect("pruned module should still compile");
}

#[test]
fn reachability_keeps_builtin_constant_initializers() {
    let source = r#"
import math

def main() -> int32:
    print(math.e)
    return 0
"#;
    let mir = lower_source_to_mir(source).expect("builtin constant source should lower");
    let pruned = super::reachability::prune_unreachable_functions(&mir);
    for constant in &pruned.constants {
        assert!(
            pruned
                .functions
                .iter()
                .any(|function| function.name == constant.initializer),
            "constant initializer `{}` must be kept",
            constant.initializer
        );
    }
    emit_host_object(&mir).expect("builtin constant source should compile directly");
}

#[test]
fn native_borrowed_loop_reads_elements_in_place() {
    let source = r#"
def scan(names: list[str], docs: list[dict[str, int64]], other: str) -> int64:
    mut hits = 0
    for name in names:
        if name == "x":
            hits += name.len()
        if other != name:
            hits += 1
    for doc in docs:
        match doc.get("age"):
            case Lookup.Found(age):
                hits += age
            case Lookup.Missing:
                pass
    return hits

def main():
    print(scan(["x"], [{"age": 1}], "y"))
"#;
    let mir = lower_source_to_mir(source).expect("borrowed loop source should lower");
    let object = emit_host_object(&mir).expect("borrowed loop source should emit directly");
    let referenced = object_referenced_symbols(&object);
    for symbol in [
        "aura_direct_compare_str_literal_at",
        "aura_direct_compare_values_at",
        "aura_direct_vec_element_len",
        "aura_direct_vec_element_map_get_str_payload",
    ] {
        assert!(
            referenced.iter().any(|name| name.ends_with(symbol)),
            "the loops should call `{symbol}`, referenced: {referenced:?}"
        );
    }
    // The loop's bound check covers each loan, and the literal key is not
    // boxed.
    for symbol in [
        "aura_direct_vec_check_index",
        "aura_direct_vec_element_map_get",
        "aura_direct_vec_index",
        "aura_direct_element_path_load",
        "aura_direct_element_path_probe",
        "aura_direct_element_path_take",
    ] {
        assert!(
            !referenced.iter().any(|name| name.ends_with(symbol)),
            "a borrowed loop read should not call `{symbol}`"
        );
    }
}

#[test]
fn native_element_loans_follow_loans_used_only_through_their_place() {
    let source = r#"
class Counter:
    name: str
    count: int64
    tags: list[str]

class Shelf:
    totals: list[int64]
    rows: list[list[int64]]

def bump(items: mut list[Counter], grid: mut list[list[int64]], values: mut list[int64], names: list[str], shelf: mut Shelf, table: mut dict[str, Counter]) -> int64:
    items[0].count += 1
    items[1].tags.append("t")
    grid[0].append(1)
    values[1] += 2
    shelf.totals[0] += 3
    shelf.rows[1].append(4)
    grid[0][1] += 5
    table["k"].count += 6
    return names[0].len() + items[0].tags.len()

def main():
    mut items = [Counter(name="a", count=1, tags=[]), Counter(name="b", count=2, tags=[])]
    mut grid = [[1, 2], [3]]
    mut values = [1, 2]
    mut shelf = Shelf(totals=[0], rows=[[], []])
    mut table = {"k": Counter(name="c", count=3, tags=[])}
    print(bump(items, grid, values, ["x"], shelf, table))
    print(f"{items[0].count} {items[1].tags} {grid} {values} {shelf.totals} {shelf.rows} {table['k'].count}")
"#;
    let mir = lower_source_to_mir(source).expect("element loan source should lower");
    let bump = mir
        .functions
        .iter()
        .find(|function| function.name == "bump")
        .expect("bump should lower");
    let (uses, lend_temps) = super::element_loans::native_element_loans(bump, |_| Vec::new());
    // Each loan named by the list it selects from (a nested selection
    // begins on another loan) and the place inside the element.
    let mut followed = BTreeSet::new();
    let mut refused = BTreeSet::new();
    for instruction in bump.blocks.iter().flat_map(|block| &block.instructions) {
        if let Instruction::BeginElementLoan {
            loan,
            source,
            projection,
            ..
        } = instruction
        {
            let list = if source.starts_with('%') {
                "loan"
            } else {
                source.as_str()
            };
            let shape = format!("{list}[].{projection}");
            if uses.contains_key(loan) {
                followed.insert(shape);
            } else {
                refused.insert(shape);
            }
        }
    }
    // The analysis follows each loan that one runtime operation uses through
    // its place; the emitter later skips the `dict` entry, which it cannot
    // reach in place. A lend whose result feeds more than its operation
    // (`items[0].tags.len()` in a sum) and a nested selection, which begins
    // on another loan, keep the runtime walk.
    let expected = |shapes: &[&str]| shapes.iter().map(|shape| shape.to_string()).collect();
    assert_eq!(
        followed,
        expected(&[
            "grid[].",
            "items[].count",
            "items[].tags",
            "shelf.rows[].",
            "table[].count"
        ])
    );
    assert_eq!(refused, expected(&["items[].tags", "names[]."]));
    assert_eq!(lend_temps.len(), 5);
    // Generated code follows those it can when the program is emitted.
    emit_host_object(&mir).expect("element loan source should emit directly");
}

#[test]
fn native_record_lists_read_and_write_records_in_place() {
    let source = r#"
copy class Point:
    x: int32
    y: float32
    on: bool

copy class Segment:
    start: Point
    end: Point
    id: int64

class Holder:
    best: Segment
    all: list[Segment]

copy class Mark:
    count: uint64
    gap: None

def walk(segments: mut list[Segment], holder: Holder) -> int64:
    segments.append(holder.best)
    last = segments[-1]
    segments[0] = last
    mut total = 0
    for segment in segments:
        total += segment.id + (segment.start.x as int64)
    print(segments[1])
    return total

def marks(count: uint64) -> uint64:
    mut all: list[Mark] = []
    all.append(Mark(count=count, gap=None))
    first = all[0]
    all[0] = Mark(count=first.count + 1, gap=first.gap)
    return all[0].count

def main():
    start = Point(x=1, y=0.5, on=true)
    segment = Segment(start=start, end=start, id=3)
    mut segments: list[Segment] = []
    print(walk(segments, Holder(best=segment, all=[])))
    print(marks(4))
"#;
    let mir = lower_source_to_mir(source).expect("record list source should lower");
    let object = emit_host_object(&mir).expect("record list source should emit directly");
    let referenced = object_referenced_symbols(&object);
    for symbol in [
        "aura_direct_register_plain_layouts",
        "aura_direct_vec_read_plain",
        "aura_direct_vec_push_plain",
        "aura_direct_vec_set_plain",
        "aura_direct_instance_read_plain",
        // A record printed is boxed as an instance that holds its bytes.
        "aura_direct_instance_new_typed",
    ] {
        assert!(
            referenced.iter().any(|name| name.ends_with(symbol)),
            "record code should call `{symbol}`, referenced: {referenced:?}"
        );
    }
    // No element or field is boxed, cloned, or rebuilt one field at a time.
    for symbol in [
        "aura_direct_vec_index",
        "aura_direct_vec_push_in_place",
        "aura_direct_vec_set_index_in_place",
        "aura_direct_instance_get_field",
        "aura_direct_box_plain",
    ] {
        assert!(
            !referenced.iter().any(|name| name.ends_with(symbol)),
            "record code should not call `{symbol}`"
        );
    }
}

#[test]
fn native_list_literals_and_queued_records_box_no_element() {
    let source = r#"
copy class Point:
    x: int64
    y: float64

def main():
    numbers = [1, 2, 3]
    flags = [true, false]
    points = [Point(x=1, y=0.5), Point(x=2, y=1.5)]
    queue = Queue[Point]()
    queue.put(points[1])
    match queue.get():
        case QueueReceive.Item(p):
            print(p.x + numbers[2])
        case _:
            pass
    print(flags.len() + points.len())
"#;
    let mir = lower_source_to_mir(source).expect("list literal source should lower");
    let object = emit_host_object(&mir).expect("list literal source should emit directly");
    let referenced = object_referenced_symbols(&object);
    // Literal elements go in as their bits, and the queued record is an
    // instance that holds its bytes, read back where they lie.
    for symbol in [
        "aura_direct_vec_push_scalar",
        "aura_direct_vec_push_plain",
        "aura_direct_instance_new_typed",
    ] {
        assert!(
            referenced.iter().any(|name| name.ends_with(symbol)),
            "list literal code should call `{symbol}`, referenced: {referenced:?}"
        );
    }
    for symbol in [
        "aura_direct_vec_push_in_place",
        "aura_direct_box_i64",
        "aura_direct_box_bool",
        "aura_direct_box_plain",
    ] {
        assert!(
            !referenced.iter().any(|name| name.ends_with(symbol)),
            "list literal code should not call `{symbol}`"
        );
    }
}

#[test]
fn native_records_with_union_fields_keep_their_boxes() {
    let source = r#"
copy class Slot:
    value: int64 | None

def main():
    mut slots: list[Slot] = [Slot(value=None)]
    slots.append(Slot(value=4))
    print(slots[1].value)
"#;
    let mir = lower_source_to_mir(source).expect("union field source should lower");
    let object = emit_host_object(&mir).expect("union field source should emit directly");
    let referenced = object_referenced_symbols(&object);
    for symbol in [
        "aura_direct_register_plain_layouts",
        "aura_direct_vec_read_plain",
    ] {
        assert!(
            !referenced.iter().any(|name| name.ends_with(symbol)),
            "a record with a union field should not call `{symbol}`"
        );
    }
    assert!(referenced
        .iter()
        .any(|name| name.ends_with("aura_direct_vec_index")));
}

#[test]
fn native_programs_register_only_the_layout_kinds_they_use() {
    let classes = r#"
class Account:
    name: str
    balance: int64

def main():
    mut account = Account(name="a", balance=1)
    account.balance += 2
    print(account.balance)
"#;
    let enums = r#"
enum Shape:
    Point
    Circle(radius: float64)

def area(shape: Shape) -> float64:
    match shape:
        case Shape.Circle(radius):
            return radius * radius
        case Shape.Point:
            return 0.0

def main():
    print(area(Shape.Circle(radius=2.0)))
"#;
    for (source, kind) in [(classes, "class"), (enums, "enum")] {
        let mir = lower_source_to_mir(source).expect("the source should lower");
        let object = emit_host_object(&mir).expect("the source should emit directly");
        let referenced = object_referenced_symbols(&object);
        for other in ["plain", "class", "enum"] {
            let symbol = format!("aura_direct_register_{other}_layouts");
            assert_eq!(
                referenced.iter().any(|name| name.ends_with(&symbol)),
                other == kind,
                "a program with a {kind} layout and `{symbol}`, referenced: {referenced:?}"
            );
        }
    }
}

#[test]
fn native_executables_check_the_list_layout_they_read() {
    let source = r#"
def pick(values: list[int64], flags: list[bool], weights: list[float64]) -> int64:
    if flags[0] and weights[-1] > 0.5:
        return values[1] + values.len()
    return 0

def main():
    print(pick([1, 2], [true], [0.75]))
"#;
    let mir = lower_source_to_mir(source).expect("scalar list source should lower");
    let object = emit_host_object(&mir).expect("scalar list source should emit directly");
    let referenced = object_referenced_symbols(&object);
    assert!(
        referenced
            .iter()
            .any(|name| name.ends_with("aura_direct_check_list_layout")),
        "the entry should check the runtime list layout, referenced: {referenced:?}"
    );
    // A shared list or an integer past `int64` still reads through the runtime.
    assert!(
        referenced
            .iter()
            .any(|name| name.ends_with("aura_direct_vec_index_scalar")),
        "the slow path should remain, referenced: {referenced:?}"
    );
}

#[test]
fn proven_loop_reads_need_no_overflow_or_bounds_trap() {
    let source = r#"
def main():
    values: list[int64] = [1, 2, 3]
    mut sum = 0
    for value in values:
        sum = sum.wrapping_add(value)
    print(sum)
"#;
    let mir = lower_source_to_mir(source).expect("a list loop should lower");
    let object = emit_host_object(&mir).expect("a list loop should emit directly");
    let referenced = object_referenced_symbols(&object);
    assert!(
        !referenced
            .iter()
            .any(|name| name.ends_with("aura_direct_fail_integer_overflow")),
        "the loop index advances without an overflow check, referenced: {referenced:?}"
    );
    // A shared list still reads through the runtime.
    assert!(
        referenced
            .iter()
            .any(|name| name.ends_with("aura_direct_vec_index_scalar")),
        "the shared path should remain, referenced: {referenced:?}"
    );
}

#[test]
fn rank_one_array_reads_keep_the_runtime_path_for_other_shapes() {
    let source = r#"
def main():
    values = Array[int32].from_list([1, 2, 3], [3])
    print(values[1] + values[-1])
"#;
    let mir = lower_source_to_mir(source).expect("an Array read should lower");
    let object = emit_host_object(&mir).expect("an Array read should emit directly");
    let referenced = object_referenced_symbols(&object);
    assert!(
        referenced
            .iter()
            .any(|name| name.ends_with("aura_direct_array_index")),
        "a shared array or another rank reads through the runtime, referenced: {referenced:?}"
    );
}

#[test]
fn direct_objects_are_identical_across_repeated_builds() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/run-pass");
    let mut paths = std::fs::read_dir(&fixtures)
        .expect("run-pass fixtures should be readable")
        .map(|entry| entry.expect("fixture entry should be readable").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "au"))
        .collect::<Vec<_>>();
    paths.sort();
    // Every third fixture keeps this test short next to the timing-sensitive
    // runtime tests while still covering every kind of program.
    let paths = paths.into_iter().step_by(3).collect::<Vec<_>>();
    let mut compared = 0;
    let mut differing = Vec::new();
    for path in paths {
        let source = std::fs::read_to_string(&path).expect("fixture should be readable");
        let Ok(mir) = lower_source_to_mir(&source) else {
            continue;
        };
        let Ok(first) = emit_host_object(&mir) else {
            continue;
        };
        compared += 1;
        let relowered = lower_source_to_mir(&source).expect("a repeated lowering should succeed");
        if serde_json::to_string(&relowered).ok() != serde_json::to_string(&mir).ok() {
            differing.push(format!(
                "{} (MIR)",
                path.file_name().unwrap().to_string_lossy()
            ));
        }
        // Each build seeds its own hash maps, so one repeat is enough to see
        // iteration order reach the object.
        let again = emit_host_object(&mir).expect("a repeated build should emit");
        if again != first {
            differing.push(path.file_name().unwrap().to_string_lossy().into_owned());
        }
    }
    assert!(compared > 150, "only {compared} fixtures compiled directly");
    assert!(
        differing.is_empty(),
        "{} of {compared} fixtures emit different objects across builds: {differing:?}",
        differing.len()
    );
}

/// A program of `count` functions that call the next, large enough to
/// split into several codegen units.
fn many_function_program(count: usize) -> String {
    let mut source = String::new();
    for index in 0..count {
        let next = if index + 1 < count {
            format!("step_{}(value + {index})", index + 1)
        } else {
            "value".to_string()
        };
        source.push_str(&format!(
            "def step_{index}(value: int64) -> int64:\n    mut total = value\n    for item in [1, 2, 3]:\n        total = total + item * {index}\n    print(f\"step {index} {{total}}\")\n    return total + {next}\n\n"
        ));
    }
    source.push_str("def main():\n    print(step_0(1))\n");
    source
}

#[test]
fn large_programs_emit_several_objects_and_small_programs_one() {
    let small = lower_source_to_mir("def main():\n    print(\"hello\")\n")
        .expect("hello world should lower");
    let objects = super::emit_host_objects_with_metadata(&small, "<aura>", "")
        .expect("hello world should emit");
    assert_eq!(
        objects,
        vec![emit_host_object(&small).expect("hello world should emit")]
    );

    let large =
        lower_source_to_mir(&many_function_program(40)).expect("the large program should lower");
    let objects = super::emit_host_objects_with_metadata(&large, "<aura>", "")
        .expect("the large program should emit");
    assert!(objects.len() > 1, "{} objects", objects.len());
    // The entry point is defined once, and each unit defines its own run
    // of the program's functions.
    let mut defined = BTreeSet::new();
    let mut entries = 0;
    for bytes in &objects {
        let object = object::File::parse(bytes.as_slice()).expect("each object should parse");
        entries += object
            .symbols()
            .filter(|symbol| {
                symbol.section_index().is_some()
                    && symbol
                        .name()
                        .is_ok_and(|name| name.trim_start_matches('_') == "main")
            })
            .count();
        let functions = object_defined_aura_functions(bytes);
        assert!(!functions.is_empty());
        assert!(
            defined.is_disjoint(&functions),
            "{:?}",
            defined.intersection(&functions).collect::<Vec<_>>()
        );
        defined.extend(functions);
    }
    assert_eq!(entries, 1);
    // A repeated build writes the same objects.
    let again = super::emit_host_objects_with_metadata(&large, "<aura>", "")
        .expect("a repeated build should emit");
    assert_eq!(again, objects);
}

/// Emits `main` of `module` after `forge` changes its first `print` call,
/// without the shared validator, so the emitter's own guards are reached.
fn define_main_with_forged_print(
    module: &crate::mir::MirModule,
    forge: impl Fn(&mut CallTarget, &mut Vec<MirArg>),
) -> Result<(), String> {
    let mut main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .cloned()
        .expect("main should lower");
    let (callee, args) = main
        .blocks
        .iter_mut()
        .flat_map(|block| &mut block.instructions)
        .find_map(|instruction| match instruction {
            Instruction::Assign {
                value: Rvalue::Call { callee, args, .. },
                ..
            } if matches!(
                callee,
                CallTarget::Runtime {
                    op: crate::runtime_ops::RuntimeOp::Print(_),
                    ..
                }
            ) =>
            {
                Some((callee, args))
            }
            _ => None,
        })
        .expect("main should print");
    forge(callee, args);
    let mut codegen = NativeCodegen::new(module, "/tmp/direct_forged_named_op.au", "")
        .expect("codegen should initialize");
    codegen.define_function(&main)
}

#[test]
fn direct_named_runtime_ops_guard_receivers_targets_and_sources() {
    use crate::runtime_ops::{FunctionOp, ResourceOp, RuntimeOp};
    let module = lower_source_to_mir("def main():\n    value: int64 = 5\n    print(value)\n")
        .expect("the fixture should lower");
    let set_op = |forged: RuntimeOp| {
        move |callee: &mut CallTarget, _: &mut Vec<MirArg>| {
            let CallTarget::Runtime { op, .. } = callee else {
                unreachable!()
            };
            *op = forged;
        }
    };

    // A named op with a receiver.
    let error = define_main_with_forged_print(&module, |callee, _| {
        let CallTarget::Runtime { receiver, .. } = callee else {
            unreachable!()
        };
        *receiver = Some(Operand::Place("value".to_string()));
    })
    .expect_err("a named op takes no receiver");
    assert!(
        error.contains("received named runtime operation `Print.Int` with a receiver"),
        "{error}"
    );

    // A member op without its receiver.
    let error = define_main_with_forged_print(
        &module,
        set_op(RuntimeOp::Resource(ResourceOp::FileReadAll)),
    )
    .expect_err("a member op needs a receiver");
    assert!(
        error.contains("received runtime operation `Resource.FileReadAll` without a receiver"),
        "{error}"
    );

    // An `Array` constructor whose result is not an `Array`.
    let error =
        define_main_with_forged_print(&module, set_op(RuntimeOp::Function(FunctionOp::ArrayZeros)))
            .expect_err("an Array constructor needs an Array result");
    assert!(
        error.contains("requires an Array result type for `Array.zeros`"),
        "{error}"
    );

    // A `select` source that is not a Queue, Task, or Duration.
    let error =
        define_main_with_forged_print(&module, set_op(RuntimeOp::Function(FunctionOp::Select)))
            .expect_err("select needs Queue, Task, or Duration sources");
    assert!(
        error.contains("expected a Queue, Task, or Duration `select` source, found `int64`"),
        "{error}"
    );
}

#[test]
fn direct_backend_guards_collection_ops_the_validator_did_not_check() {
    use crate::runtime_ops::{DictOp, ListOp, RuntimeOp};
    let source = r#"
def main():
    mut words: list[str] = ["a"]
    words.append("b")
    counts: dict[str, int64] = {"a": 1}
    print(counts.get("a"))
"#;
    let mir = lower_source_to_mir(source).expect("source should lower to MIR");
    let main = mir
        .functions
        .iter()
        .find(|function| function.name == "main")
        .cloned()
        .expect("main function should exist");
    // Each case forges the first call to `target` in `main`, which the
    // emitter compiles without the module validator.
    let forge = |target: RuntimeOp, edit: &dyn Fn(&mut Vec<MirArg>)| {
        let mut forged = main.clone();
        let args = forged
            .blocks
            .iter_mut()
            .flat_map(|block| &mut block.instructions)
            .find_map(|instruction| match instruction {
                Instruction::Assign {
                    value:
                        Rvalue::Call {
                            callee: CallTarget::Runtime { op, .. },
                            args,
                            ..
                        },
                    ..
                } if *op == target => Some(args),
                _ => None,
            })
            .expect("main should call the op");
        edit(args);
        let mut codegen = NativeCodegen::new(&mir, "/tmp/direct_collection_guards.au", source)
            .expect("codegen should initialize");
        codegen
            .define_function(&forged)
            .expect_err("the forged call should be refused")
    };
    let missing = forge(RuntimeOp::List(ListOp::Push), &|args| args.clear());
    assert!(
        missing.contains("expected `append()` to receive `value`"),
        "{missing}"
    );
    let not_literal = forge(RuntimeOp::Dict(DictOp::GetStr), &|args| {
        args[0].value = Operand::Place("words".to_string());
    });
    assert!(
        not_literal.contains("expected `Dict.GetStr` to look up a literal key"),
        "{not_literal}"
    );
}

#[test]
fn member_callee_op_is_the_op_mir_names() {
    use super::MemberCallee;
    use crate::mir::MethodDispatch;
    use crate::runtime_ops::{DictOp, RuntimeOp, SetOp};

    let get = RuntimeOp::Dict(DictOp::GetStr);
    let dispatch = MethodDispatch::Runtime(get);
    assert_eq!(
        MemberCallee::Method {
            field: "get",
            dispatch: &dispatch,
        }
        .op(),
        Some(get)
    );
    let method = MethodDispatch::Static {
        function: "Box.get".to_string(),
    };
    assert_eq!(
        MemberCallee::Method {
            field: "get",
            dispatch: &method,
        }
        .op(),
        None
    );
    let take = RuntimeOp::Set(SetOp::TakeIndexOption);
    assert_eq!(MemberCallee::Runtime(take).op(), Some(take));
}

#[test]
fn native_borrowed_loop_boxes_a_dict_key_that_is_not_a_literal() {
    let source = r#"
def scan(docs: list[dict[str, int64]], key: str) -> int64:
    mut hits = 0
    for doc in docs:
        match doc.get(key):
            case Lookup.Found(age):
                hits += age
            case Lookup.Missing:
                pass
    return hits

def main():
    print(scan([{"age": 1}], "age"))
"#;
    let mir = lower_source_to_mir(source).expect("borrowed loop source should lower");
    let object = emit_host_object(&mir).expect("borrowed loop source should emit directly");
    let referenced = object_referenced_symbols(&object);
    assert!(
        referenced
            .iter()
            .any(|name| name.ends_with("aura_direct_vec_element_map_get_payload")),
        "a place key should be read through the boxed lookup, referenced: {referenced:?}"
    );
    assert!(
        !referenced.iter().any(|name| {
            name.ends_with("aura_direct_vec_element_map_get_str")
                || name.ends_with("aura_direct_vec_element_map_get_str_payload")
        }),
        "only a literal key is looked up without a box"
    );
}

#[test]
fn an_unresolved_internal_slice_needs_all_six_arguments() {
    // Lowering resolves `__slice` to a runtime op, so only forged MIR
    // reaches the emitter's own arity check.
    let mut module = lower_source_to_mir("def main():\n    xs = [1, 2, 3]\n    print(xs[0:1])\n")
        .expect("the slice program should lower");
    let call = module
        .functions
        .iter_mut()
        .flat_map(|function| function.blocks.iter_mut())
        .flat_map(|block| block.instructions.iter_mut())
        .find_map(|instruction| match instruction {
            Instruction::Assign {
                value: Rvalue::Call { callee, args, .. },
                ..
            } if matches!(
                callee,
                CallTarget::Runtime {
                    receiver: Some(_),
                    ..
                }
            ) && args.len() == 6 =>
            {
                Some((callee, args))
            }
            _ => None,
        })
        .expect("the slice should lower to a runtime call with six arguments");
    let CallTarget::Runtime { receiver, .. } = call.0.clone() else {
        unreachable!()
    };
    *call.0 = CallTarget::Member {
        object: receiver.expect("a slice has a receiver"),
        field: "__slice".to_string(),
        receiver_place: None,
        dispatch: None,
    };
    call.1.truncate(2);
    let error = super::validate_module(&module).expect_err("a short internal slice must fail");
    assert_eq!(error, super::DIRECT_INTERNAL_SLICE_ARITY_ERROR);
}

#[test]
fn native_emission_refuses_an_unchecked_read_no_fact_covers() {
    let source = r#"
def main():
    values: list[int64] = [1, 2, 3]
    mut sum = 0
    for value in values:
        sum = sum.wrapping_add(value)
    print(sum)
"#;
    let mut mir = lower_source_to_mir(source).expect("a list loop should lower");
    let main = mir
        .functions
        .iter_mut()
        .find(|function| function.name == "main")
        .expect("main should lower");
    for block in &mut main.blocks {
        block
            .instructions
            .retain(|instruction| !matches!(instruction, Instruction::IndexInRange { .. }));
    }
    let error = emit_host_object(&mir).expect_err("an uncovered unchecked read should be refused");
    assert!(
        error.contains("without a bounds check, but no `IndexInRange` covers it"),
        "{error}"
    );
}

#[test]
fn the_emitter_emits_only_the_checks_mir_names() {
    let source = r#"
def arithmetic(a: int64, b: int64, c: int32, d: float64) -> float64:
    wide = (a - b) * -a // b + a % b
    narrow = c // 2 + c % 3
    return d / 2.0 + (wide + (narrow as int64)) as float64

def main():
    print(arithmetic(7, 2, 9, 1.5))
"#;
    let named = lower_source_to_mir(source).expect("arithmetic should lower");
    let checked = emit_unoptimized_object(&named).expect("arithmetic should emit");
    let referenced = object_referenced_symbols(&checked);
    for helper in [
        "aura_direct_fail_integer_overflow",
        "aura_direct_fail_division_by_zero",
    ] {
        assert!(
            referenced.iter().any(|name| name.ends_with(helper)),
            "the named traps call `{helper}`, referenced: {referenced:?}"
        );
    }
    // With every trap removed, the emitter checks nothing it was not told
    // to. The validator would refuse this module, so it is emitted as is.
    let mut unchecked = super::reachability::prune_unreachable_functions(&named);
    for instruction in unchecked
        .functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .flat_map(|block| &mut block.instructions)
    {
        if let Instruction::Assign {
            value: Rvalue::Binary { trap, .. } | Rvalue::Unary { trap, .. },
            ..
        } = instruction
        {
            *trap = crate::mir::ArithmeticTrap::None;
        }
    }
    let object = crate::native_backend::without_optimization(|| {
        NativeCodegen::with_validated_loans(&unchecked, "<aura>", "")
            .and_then(|context| context.emit())
    })
    .expect("unchecked arithmetic should emit");
    let referenced = object_referenced_symbols(&object);
    assert!(
        !referenced
            .iter()
            .any(|name| name.ends_with("aura_direct_fail_integer_overflow")
                || name.ends_with("aura_direct_fail_division_by_zero")),
        "no trap is named, referenced: {referenced:?}"
    );
}

#[test]
fn direct_member_calls_compile_the_dispatch_mir_names() {
    use crate::mir::MethodDispatch;

    let module = lower_source_to_mir(
        r#"
trait Speak:
    def speak(self) -> int64

class Dog:
    age: int64

impl Speak for Dog:
    def speak(self) -> int64:
        return self.age

class Cat:
    lives: int64

impl Speak for Cat:
    def speak(self) -> int64:
        return self.lives

def show[T: Speak](pet: T) -> int64:
    return pet.speak()

def main() -> int32:
    print(show(Dog(age=3)) + show(Cat(lives=9)))
    return 0
"#,
    )
    .expect("trait dispatch should lower");
    let with_dispatch = |edit: &dyn Fn(&mut Option<MethodDispatch>)| {
        let mut module = module.clone();
        for instruction in module
            .functions
            .iter_mut()
            .filter(|function| function.name == "show")
            .flat_map(|function| &mut function.blocks)
            .flat_map(|block| &mut block.instructions)
        {
            if let Instruction::Assign {
                value:
                    Rvalue::Call {
                        callee:
                            CallTarget::Member { dispatch, .. }
                            | CallTarget::TraitMember { dispatch, .. },
                        ..
                    },
                ..
            } = instruction
            {
                edit(dispatch);
            }
        }
        let show = module
            .functions
            .iter()
            .find(|function| function.name == "show")
            .cloned()
            .expect("show should lower");
        let mut codegen = NativeCodegen::new(&module, "/tmp/direct_dispatch.au", "")
            .expect("direct codegen should initialize");
        codegen.define_function(&show)
    };

    // A receiver whose type is known runs a runtime type test per
    // candidate, without the erased union path.
    with_dispatch(&|dispatch| {
        if let Some(MethodDispatch::Candidates { erased, .. }) = dispatch {
            *erased = false;
        }
    })
    .expect("a candidate chain on a known receiver should compile");

    let missing = with_dispatch(&|dispatch| *dispatch = None)
        .expect_err("the emitter needs the dispatch MIR names");
    assert!(
        missing.contains("direct backend received member call `.speak` without a dispatch"),
        "{missing}"
    );
}

const LOOKUP_MATCH_SOURCE: &str = r#"
def main() -> int32:
    mut counts: dict[str, int32] = {}
    counts["a"] = 3
    match counts.get("a"):
        case Lookup.Found(value):
            print(value)
        case Lookup.Missing:
            print(0)
    return 0
"#;

/// The symbols an object references, without any platform underscore.
fn exact_referenced_symbols(object: &[u8]) -> BTreeSet<String> {
    object_referenced_symbols(object)
        .into_iter()
        .map(|symbol| symbol.trim_start_matches('_').to_string())
        .collect()
}

#[test]
fn direct_lookup_matches_keep_the_found_value_without_a_lookup() {
    let mir = lower_source_to_mir(LOOKUP_MATCH_SOURCE).expect("the lookup match should lower");
    let object = emit_host_object(&mir).expect("the lookup match should compile directly");
    let referenced = exact_referenced_symbols(&object);
    for required in [
        "aura_direct_map_get_str_payload",
        "aura_direct_tag_value_type_cell",
    ] {
        assert!(
            referenced.contains(required),
            "lookup matches must reference `{required}`: {referenced:?}"
        );
    }
    for forbidden in [
        "aura_direct_map_get_str",
        "aura_direct_lookup_payload",
        "aura_direct_variant_matches",
        "aura_direct_variant_payload",
        "aura_direct_tag_value_type",
    ] {
        assert!(
            !referenced.contains(forbidden),
            "lookup matches must not reference `{forbidden}`: {referenced:?}"
        );
    }
}

#[test]
fn direct_scalar_variant_payloads_read_without_a_box() {
    let source = r#"
enum Reading:
    Count(int32)
    Flag(bool)

def main() -> int32:
    readings = [Reading.Count(4), Reading.Flag(true)]
    for reading in readings:
        match reading:
            case Reading.Count(count):
                print(count)
            case Reading.Flag(flag):
                print(flag)
    return 0
"#;
    let mir = lower_source_to_mir(source).expect("the enum match should lower");
    let object = emit_host_object(&mir).expect("the enum match should compile directly");
    let referenced = exact_referenced_symbols(&object);
    for required in [
        "aura_direct_variant_payload_i64",
        "aura_direct_variant_payload_bool",
    ] {
        assert!(
            referenced.contains(required),
            "scalar payloads must reference `{required}`: {referenced:?}"
        );
    }
    for forbidden in ["aura_direct_unbox_i64", "aura_direct_unbox_bool"] {
        assert!(
            !referenced.contains(forbidden),
            "scalar payloads must not reference `{forbidden}`: {referenced:?}"
        );
    }
}

#[test]
fn direct_fstrings_start_with_a_leading_int64_in_one_call() {
    let source = r#"
def main() -> int32:
    mut i: int64 = 7
    print(f"key-{i}")
    print(f"{i} and {i}")
    return 0
"#;
    let mir = lower_source_to_mir(source).expect("the f-strings should lower");
    let object = emit_host_object(&mir).expect("the f-strings should compile directly");
    let referenced = exact_referenced_symbols(&object);
    assert!(
        referenced.contains("aura_direct_format_begin_i64"),
        "{referenced:?}"
    );
    assert!(
        !referenced.contains("aura_direct_format_begin"),
        "{referenced:?}"
    );
    // Both f-strings begin with `format_begin_i64`. Counting against it keeps
    // the check true where each call takes two relocations, as on arm64
    // Mach-O, which loads a callee's address from the GOT.
    let begins = object_referenced_symbol_occurrences(&object, "aura_direct_format_begin_i64");
    assert_eq!(
        2 * object_referenced_symbol_occurrences(&object, "aura_direct_format_push_i64"),
        begins,
        "only the second f-string's later `int64` part should push"
    );
}

/// The lookup slots of `LOOKUP_MATCH_SOURCE`'s `main` after `change`.
fn lookup_slots_after(change: impl FnOnce(&mut MirFunction, &str)) -> BTreeSet<String> {
    let mir = lower_source_to_mir(LOOKUP_MATCH_SOURCE).expect("the lookup match should lower");
    let mut main = mir
        .functions
        .into_iter()
        .find(|function| function.name == "main")
        .expect("main");
    let slots = super::lookups::lookup_payload_slots(&main);
    let [slot] = slots.iter().collect::<Vec<_>>()[..] else {
        panic!("the lookup match should have one slot: {slots:?}");
    };
    let slot = slot.clone();
    change(&mut main, &slot);
    super::lookups::lookup_payload_slots(&main)
        .into_iter()
        .collect()
}

/// The block and position of the instruction that unpacks `slot`.
fn lookup_unpack_site(function: &MirFunction, slot: &str) -> (usize, usize) {
    function
        .blocks
        .iter()
        .enumerate()
        .find_map(|(block_index, block)| {
            block
                .instructions
                .iter()
                .position(|instruction| {
                    matches!(
                        instruction,
                        Instruction::Assign {
                            value: Rvalue::VariantPayload {
                                scrutinee: Operand::Place(place) | Operand::MovePlace(place),
                                ..
                            },
                            ..
                        } if place == slot
                    )
                })
                .map(|position| (block_index, position))
        })
        .expect("the slot should be unpacked")
}

#[test]
fn direct_lookup_slots_reject_every_other_use() {
    assert_eq!(lookup_slots_after(|_, _| {}).len(), 1);

    // Two copies of one payload would share one box.
    assert!(lookup_slots_after(|main, slot| {
        let (block, position) = lookup_unpack_site(main, slot);
        let unpack = main.blocks[block].instructions[position].clone();
        main.blocks[block].instructions.insert(position, unpack);
    })
    .is_empty());

    // So would one unpack that runs twice for one lookup.
    assert!(lookup_slots_after(|main, slot| {
        let (block, _) = lookup_unpack_site(main, slot);
        let label = main.blocks[block].label.clone();
        main.blocks[block].terminator = Terminator::Goto(label);
    })
    .is_empty());

    // Any other read needs the `Lookup` itself.
    assert!(lookup_slots_after(|main, slot| {
        main.blocks[0].instructions.push(Instruction::Assign {
            target: "%copy".to_string(),
            value: Rvalue::Use(Operand::Place(slot.to_string())),
        });
    })
    .is_empty());

    // So does an assignment that is not a `get`.
    assert!(lookup_slots_after(|main, slot| {
        main.blocks[0].instructions.push(Instruction::Assign {
            target: slot.to_string(),
            value: Rvalue::Use(Operand::Int(0)),
        });
    })
    .is_empty());

    // A parameter arrives as a `Lookup`.
    assert!(lookup_slots_after(|main, slot| {
        main.params.push(MirParam {
            name: slot.to_string(),
            passing: MirReceiverKind::Value,
            ty: Type::named("int32"),
            default_function: None,
            keyword_only: false,
        });
    })
    .is_empty());

    // A match on any other variant reads the `Lookup`.
    assert!(lookup_slots_after(|main, slot| {
        for block in &mut main.blocks {
            if let Terminator::Match {
                scrutinee: Operand::Place(place),
                arms,
                ..
            } = &mut block.terminator
            {
                if place == slot {
                    arms[0].variant_name = Some("Other".to_string());
                }
            }
        }
    })
    .is_empty());

    // An unpack of a later payload has nothing to read.
    assert!(lookup_slots_after(|main, slot| {
        let (block, position) = lookup_unpack_site(main, slot);
        if let Instruction::Assign {
            value: Rvalue::VariantPayload { index, .. },
            ..
        } = &mut main.blocks[block].instructions[position]
        {
            *index = 1;
        }
    })
    .is_empty());
}

const QUEUE_SLOT_SOURCE: &str = r#"
def produce(jobs: Queue[int64], n: int64):
    for i in range(n):
        jobs.put(i)
    jobs.close()


def main() -> int32:
    mut total = 0
    with TaskGroup() as group:
        jobs = Queue[int64](capacity=4)
        group.start_soon(produce, jobs, 5)
        for job in jobs:
            total += job
    others = Queue[int64]()
    others.put(3)
    match others.get():
        case QueueReceive.Item(value):
            total += value
        case _:
            pass
    print(total)
    return 0
"#;

#[test]
fn direct_queue_receives_and_discarded_puts_build_no_variant() {
    let mir = lower_source_to_mir(QUEUE_SLOT_SOURCE).expect("the queue program should lower");
    let object = emit_host_object(&mir).expect("the queue program should compile directly");
    let referenced = exact_referenced_symbols(&object);
    for required in [
        "aura_direct_channel_send_discarded",
        "aura_direct_channel_recv_item",
        "aura_direct_channel_recv_in_task_group_item",
    ] {
        assert!(
            referenced.contains(required),
            "queue slots must reference `{required}`: {referenced:?}"
        );
    }
    for forbidden in [
        "aura_direct_channel_send",
        "aura_direct_channel_recv",
        "aura_direct_channel_recv_in_task_group",
        "aura_direct_variant_matches",
        "aura_direct_variant_take_payload",
        "aura_direct_lookup_payload",
    ] {
        assert!(
            !referenced.contains(forbidden),
            "queue slots must not reference `{forbidden}`: {referenced:?}"
        );
    }
}

#[test]
fn direct_queue_receives_that_tell_other_variants_apart_keep_the_variant() {
    let source = r#"
def main() -> int32:
    jobs = Queue[int32]()
    jobs.close()
    match jobs.get():
        case QueueReceive.Item(value):
            print(value)
        case QueueReceive.Closed:
            print("closed")
        case _:
            print("other")
    return 0
"#;
    let mir = lower_source_to_mir(source).expect("the queue match should lower");
    let main = mir
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main");
    assert!(super::lookups::lookup_payload_slots(main).is_empty());
    let object = emit_host_object(&mir).expect("the queue match should compile directly");
    let referenced = exact_referenced_symbols(&object);
    assert!(
        referenced.contains("aura_direct_channel_recv"),
        "{referenced:?}"
    );
    assert!(
        !referenced.contains("aura_direct_channel_recv_item"),
        "{referenced:?}"
    );
}

/// The queue slots of `QUEUE_SLOT_SOURCE`'s `main` after `change`, which
/// gets the receive slot and the discarded `put` slot.
fn queue_slots_after(change: impl FnOnce(&mut MirFunction, &str, &str)) -> BTreeSet<String> {
    let mir = lower_source_to_mir(QUEUE_SLOT_SOURCE).expect("the queue program should lower");
    let mut main = mir
        .functions
        .into_iter()
        .find(|function| function.name == "main")
        .expect("main");
    let slots = super::lookups::lookup_payload_slots(&main);
    let assigned_from = |slots: &std::collections::HashSet<String>, wanted: &str| {
        main.blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .find_map(|instruction| match instruction {
                Instruction::Assign {
                    target,
                    value: Rvalue::Call { callee, .. },
                } if slots.contains(target) && format!("{callee:?}").contains(wanted) => {
                    Some(target.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("a slot assigned from `{wanted}`: {slots:?}"))
    };
    let receive = assigned_from(&slots, "Queue(Get)");
    let put = assigned_from(&slots, "Queue(Put)");
    change(&mut main, &receive, &put);
    super::lookups::lookup_payload_slots(&main)
        .into_iter()
        .collect()
}

#[test]
fn direct_queue_slots_reject_every_other_use() {
    let all = queue_slots_after(|_, _, _| {});
    assert_eq!(all.len(), 3, "{all:?}");

    // A discarded `put` result that is unpacked needs its `Result`.
    let unpacked = queue_slots_after(|main, _, put| {
        main.blocks[0].instructions.push(Instruction::Assign {
            target: "%payload".to_string(),
            value: Rvalue::VariantPayload {
                scrutinee: Operand::Place(put.to_string()),
                variant_name: "Ok".to_string(),
                index: 0,
            },
        });
    });
    assert_eq!(unpacked.len(), 2, "{unpacked:?}");

    // So does one that is matched on a variant.
    let matched = queue_slots_after(|main, receive, put| {
        let block = main
            .blocks
            .iter_mut()
            .find(|block| {
                matches!(&block.terminator, Terminator::Match { scrutinee: Operand::Place(place), .. } if place == receive)
            })
            .expect("the receive's match");
        if let Terminator::Match { scrutinee, .. } = &mut block.terminator {
            *scrutinee = Operand::Place(put.to_string());
        }
    });
    assert_eq!(matched.len(), 2, "{matched:?}");

    // A temporary assigned from a receive and from a `put` is neither.
    let mixed = queue_slots_after(|main, receive, put| {
        let put_value = main
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .find_map(|instruction| match instruction {
                Instruction::Assign { target, value } if target == put => Some(value.clone()),
                _ => None,
            })
            .expect("the put");
        main.blocks[0].instructions.push(Instruction::Assign {
            target: receive.to_string(),
            value: put_value,
        });
    });
    assert_eq!(mixed.len(), 2, "{mixed:?}");

    // A receive arm for another variant that goes elsewhere tells it apart.
    let told_apart = queue_slots_after(|main, receive, _| {
        let block = main
            .blocks
            .iter_mut()
            .find(|block| {
                matches!(&block.terminator, Terminator::Match { scrutinee: Operand::Place(place), .. } if place == receive)
            })
            .expect("the receive's match");
        if let Terminator::Match { arms, .. } = &mut block.terminator {
            let mut closed = arms[0].clone();
            closed.variant_name = Some("Closed".to_string());
            closed.wildcard = false;
            arms.insert(0, closed);
        }
    });
    assert_eq!(told_apart.len(), 2, "{told_apart:?}");
}

#[test]
fn str_literal_equality_reads_the_str_view_in_place() {
    let equality = r#"
def main():
    names = ["user-1", "user-7"]
    mut hits = 0
    for name in names:
        if name == "user-7":
            hits += 1
        if "" != name:
            hits += 1
    print(hits)
"#;
    let mir = lower_source_to_mir(equality).expect("the equality loop should lower");
    let object = emit_host_object(&mir).expect("the equality loop should compile directly");
    let referenced = exact_referenced_symbols(&object);
    // The bytes are compared in generated code once the lengths match, and
    // a list without a `str` view still takes the runtime compare.
    for required in [
        "aura_direct_bytes_equal",
        "aura_direct_compare_str_literal_at",
    ] {
        assert!(referenced.contains(required), "{required}: {referenced:?}");
    }
}

#[test]
fn int64_keys_reach_the_runtime_without_a_box() {
    let source = r#"
def main():
    mut seen = set[int64]()
    mut counts: dict[int64, str] = {}
    mut i = 0
    while i < 20:
        seen.add(i * 3)
        counts[i] = "x"
        i += 1
    mut hits = 0
    if 9 in seen:
        hits += 1
    if 7 in counts:
        hits += 1
    match counts.get(4):
        case Lookup.Found(text):
            hits += len(text)
        case Lookup.Missing:
            hits -= 1
    print(hits)
"#;
    let mir = lower_source_to_mir(source).expect("the int64 keys should lower");
    let object = emit_host_object(&mir).expect("the int64 keys should compile directly");
    let referenced = exact_referenced_symbols(&object);
    for required in [
        "aura_direct_set_insert_int64_in_place",
        "aura_direct_set_contains_int64",
        "aura_direct_map_contains_int64",
        "aura_direct_map_get_int64_payload",
    ] {
        assert!(referenced.contains(required), "{required}: {referenced:?}");
    }
    for boxed in [
        "aura_direct_set_insert_in_place",
        "aura_direct_set_contains",
        "aura_direct_map_contains_key",
        "aura_direct_map_get_payload",
    ] {
        assert!(!referenced.contains(boxed), "{boxed}: {referenced:?}");
    }
}

#[test]
fn lookups_only_read_lend_their_found_value() {
    let source = r#"
import json

def main():
    mut docs: list[dict[str, json.Value]] = []
    mut doc: dict[str, json.Value] = {}
    doc["age"] = json.Value.Int(41)
    docs.append(doc)
    mut flags: dict[int64, json.Value] = {}
    flags[1] = json.Value.Bool(true)
    mut total = 0
    for item in docs:
        match item.get("age"):
            case Lookup.Found(value):
                match value:
                    case json.Value.Int(age):
                        if age > 40:
                            total += age
                    case _:
                        pass
            case Lookup.Missing:
                pass
    match flags.get(1):
        case Lookup.Found(flag):
            match flag:
                case json.Value.Bool(set):
                    if set:
                        total += 1
                case _:
                    pass
        case Lookup.Missing:
            pass
    print(total + docs.len() + flags.len())
"#;
    let mir = lower_source_to_mir(source).expect("the lookups should lower");
    let object = emit_host_object(&mir).expect("the lookups should compile directly");
    let referenced = exact_referenced_symbols(&object);
    // A found value that is only tested and read is lent, never copied.
    // Both dicts outlive it: one the MIR releases right after the lookup,
    // as after its last use, is copied from.
    for required in [
        "aura_direct_vec_element_map_get_str_lent",
        "aura_direct_map_get_int64_lent",
        "aura_direct_lent_variant_matches",
        "aura_direct_lent_variant_payload_int64",
        "aura_direct_lent_variant_payload_bool",
    ] {
        assert!(referenced.contains(required), "{required}: {referenced:?}");
    }
    for copied in [
        "aura_direct_vec_element_map_get_str_payload",
        "aura_direct_map_get_int64_payload",
    ] {
        assert!(!referenced.contains(copied), "{copied}: {referenced:?}");
    }
}

#[test]
fn lookups_copy_their_found_value_when_the_dict_can_change() {
    // Each program changes, hands on, or keeps the found value, so the
    // lookup copies it.
    let cases = [
        r#"
import json

def main():
    mut scores: dict[str, json.Value] = {}
    scores["a"] = json.Value.Int(1)
    match scores.get("a"):
        case Lookup.Found(value):
            scores["a"] = json.Value.Int(2)
            match value:
                case json.Value.Int(n):
                    print(n)
                case _:
                    pass
        case Lookup.Missing:
            pass
    print(scores.len())
"#,
        r#"
import json

def kind(value: json.Value) -> str:
    match value:
        case json.Value.Int(_):
            return "int"
        case _:
            return "other"

def main():
    mut scores: dict[str, json.Value] = {}
    scores["a"] = json.Value.Int(1)
    match scores.get("a"):
        case Lookup.Found(value):
            print(kind(value))
        case Lookup.Missing:
            pass
    print(scores.len())
"#,
        r#"
import json

def main():
    mut scores: dict[str, json.Value] = {}
    scores["a"] = json.Value.Int(1)
    mut kept: list[json.Value] = []
    match own scores.get("a"):
        case Lookup.Found(value):
            kept.append(value)
        case Lookup.Missing:
            pass
    print(kept.len())
"#,
    ];
    for source in cases {
        let mir = lower_source_to_mir(source).expect("the lookup should lower");
        let object = emit_host_object(&mir).expect("the lookup should compile directly");
        let referenced = exact_referenced_symbols(&object);
        // An owned match takes the whole lookup, so it copies too.
        assert!(
            referenced.contains("aura_direct_map_get_str_payload")
                || referenced.contains("aura_direct_map_get_str"),
            "{source}: {referenced:?}"
        );
        assert!(
            !referenced.contains("aura_direct_map_get_str_lent"),
            "{source}: {referenced:?}"
        );
    }
}

#[test]
fn scalar_appends_keep_the_runtime_push_for_a_full_list() {
    let source = r#"
def main():
    mut values: list[int64] = []
    mut flags: list[bool] = []
    mut i = 0
    while i < 10:
        values.append(i)
        flags.append(i % 2 == 0)
        i += 1
    print(values.len() + flags.len())
"#;
    let mir = lower_source_to_mir(source).expect("the append loop should lower");
    let object = emit_host_object(&mir).expect("the append loop should compile directly");
    let referenced = exact_referenced_symbols(&object);
    // Generated code stores an element in place while the list has room,
    // and a full list, or one in another form, grows through the runtime.
    assert!(
        referenced.contains("aura_direct_vec_push_scalar"),
        "{referenced:?}"
    );
    assert!(
        !referenced.contains("aura_direct_vec_push_in_place"),
        "{referenced:?}"
    );
}

#[test]
fn scalar_element_writes_keep_the_runtime_write_for_shared_storage() {
    let source = r#"
def main():
    mut values: list[int64] = [1, 2, 3]
    mut floats: list[float64] = [0.5, 1.5]
    mut flags: list[bool] = [false]
    values[1] = 7
    values[-1] += 4
    floats[0] *= 3.0
    flags[0] = true
    print(values[1] + values[2])
    print(floats[0])
    print(flags[0])
"#;
    let mir = lower_source_to_mir(source).expect("the element writes should lower");
    let object = emit_host_object(&mir).expect("the element writes should compile directly");
    let referenced = exact_referenced_symbols(&object);
    // Generated code stores an element in place when the list keeps its
    // scalars in unshared storage. Any other list, or an index out of
    // range, takes the runtime write, which stores the bits with no box.
    assert!(
        referenced.contains("aura_direct_vec_set_index_scalar"),
        "{referenced:?}"
    );
    assert!(
        !referenced.contains("aura_direct_vec_set_index_in_place"),
        "{referenced:?}"
    );
}

#[test]
fn native_scalar_math_computes_in_words() {
    let source = r#"
def blend(a: int64, b: int32, c: uint64, d: float64, e: float32) -> float64:
    small: int8 = -3
    print(abs(small))
    print(min(a, 7) + max(0, a) + abs(a) + round(d))
    print(max(b, b + 1))
    print(min(c, c + 1))
    print(sqrt(e))
    return sqrt(d) + abs(d) + max(0.0, d)

def main():
    print(blend(1, 2, 3, 4.0, 5.0))
"#;
    let mir = lower_source_to_mir(source).expect("scalar math source should lower");
    // Unoptimized, so constant arguments do not fold the calls away.
    let object = emit_unoptimized_object(&mir).expect("scalar math source should emit directly");
    let referenced = object_referenced_symbols(&object);
    for symbol in [
        "aura_direct_sqrt_f64",
        "aura_direct_round_f64",
        "aura_direct_fail_narrow_overflow",
    ] {
        assert!(
            referenced.iter().any(|name| name.ends_with(symbol)),
            "scalar math should call `{symbol}`, referenced: {referenced:?}"
        );
    }
    for symbol in [
        "aura_direct_abs",
        "aura_direct_min",
        "aura_direct_max",
        "aura_direct_sqrt",
        "aura_direct_round",
    ] {
        assert!(
            !referenced.iter().any(|name| name.ends_with(symbol)),
            "scalar math should not call the boxed `{symbol}`"
        );
    }
}

#[test]
fn native_module_constant_reads_find_their_frozen_value_untagged() {
    let source = r#"
NAMES: list[str] = ["alpha", "beta"]
LIMITS: dict[str, int64] = {"alpha": 1}

def count() -> int64:
    mut total = 0
    for name in NAMES:
        total += name.len() + LIMITS.len()
    return total

def main():
    print(count())
"#;
    let mir = lower_source_to_mir(source).expect("constant source should lower");
    let object = emit_host_object(&mir).expect("constant source should emit directly");
    let referenced = object_referenced_symbols(&object);
    assert!(
        referenced
            .iter()
            .any(|name| name.ends_with("aura_direct_module_constant")),
        "constant reads should call the runtime, referenced: {referenced:?}"
    );
    // Each constant is tagged once when it is made, so a read of its own
    // type tags nothing.
    assert!(
        !referenced
            .iter()
            .any(|name| name.ends_with("aura_direct_tag_value_type")),
        "constant reads should not tag their value, referenced: {referenced:?}"
    );
}

#[test]
fn direct_write_backs_keep_their_parameters_tags() {
    let source = r#"
def bump(xs: mut list[int64], names: mut list[str]):
    xs[0] += 1
    names = ["a"]

def one() -> Result[int64, str]:
    return Result.Ok(1)

def bump_parsed(xs: mut list[int64]) -> Result[int64, str]:
    value = try one()
    xs[0] += value
    return Result.Ok(value)

def main():
    mut xs = [1, 2]
    mut names = ["b"]
    for _ in range(3):
        bump(xs, names)
    match bump_parsed(xs):
        case Result.Ok(value):
            print(value)
        case Result.Err(_):
            print(0)
    print(xs[0])
    print(names[0])
"#;
    let mir = lower_source_to_mir(source).expect("write-back source should lower");
    let program = super::shapes::ProgramShapes::new(&mir, std::iter::empty());
    // `bump` stores only tagged boxes in its parameters, so what it writes
    // back carries their tags; `bump_parsed` runs a `try`, so it does not.
    assert!(program.writes_back_tagged("bump"));
    assert!(!program.writes_back_tagged("bump_parsed"));
    let function = |name: &str| {
        mir.functions
            .iter()
            .find(|function| function.name == name)
            .unwrap_or_else(|| panic!("`{name}` should lower"))
    };
    // A local passed for write-back and a mutable parameter stay shaped.
    let main = super::shapes::shaped_locals(function("main"), &program, |_| false);
    assert!(main.contains("xs") && main.contains("names"), "{main:?}");
    let bump = super::shapes::shaped_locals(function("bump"), &program, |_| false);
    assert!(bump.contains("xs") && bump.contains("names"), "{bump:?}");
    emit_host_object(&mir).expect("write-back source should compile directly");
}

#[test]
fn direct_class_layouts_keep_enum_union_and_class_fields_in_boxes() {
    let source = r#"
enum Value:
    Int(value: int64)
    Text(value: str)

copy class Point:
    x: int64
    y: int64

copy class Pair:
    left: Point
    right: Point

class Tag:
    name: str

class Condition:
    column: int64
    value: Value
    label: str | None
    tag: Tag
    at: Point

def main():
    condition = Condition(column=1, value=Value.Int(2), label=None, tag=Tag(name="t"), at=Point(x=1, y=2))
    pair = Pair(left=Point(x=1, y=2), right=Point(x=3, y=4))
    print(condition.column + pair.right.y)
"#;
    let mir = lower_source_to_mir(source).expect("class layout source should lower");
    let records = super::plain::PlainRecords::new(&mir);
    let (table, root) = records
        .class_layout("Condition")
        .expect("the class is kept in place");
    let LayoutKind::Struct { fields, .. } = &table.get(root).kind else {
        panic!("a class lays out as a struct");
    };
    let kinds = fields
        .iter()
        .map(|field| (field.name.as_str(), table.get(field.layout).kind.clone()))
        .collect::<Vec<_>>();
    assert!(matches!(kinds[0], ("column", LayoutKind::Scalar(_))));
    assert!(matches!(kinds[1], ("value", LayoutKind::Handle)));
    assert!(matches!(kinds[2], ("label", LayoutKind::Handle)));
    // A class field stays inline, where its scalars are read in place.
    assert!(matches!(kinds[3], ("tag", LayoutKind::Struct { .. })));
    // A Copy class field still lies inline.
    assert!(matches!(kinds[4], ("at", LayoutKind::Struct { .. })));
    // A Copy class keeps its own layout.
    let (table, root) = records
        .class_layout("Pair")
        .expect("the Copy class is kept in place");
    let LayoutKind::Struct { fields, .. } = &table.get(root).kind else {
        panic!("a class lays out as a struct");
    };
    assert!(fields
        .iter()
        .all(|field| matches!(table.get(field.layout).kind, LayoutKind::Struct { .. })));
    emit_host_object(&mir).expect("class layout source should compile directly");
}

#[test]
fn direct_record_list_pops_read_the_record_in_place() {
    let source = r#"
copy class Entry:
    dist: int64
    node: int64

def main():
    mut heap: list[Entry] = []
    heap.append(Entry(dist=3, node=1))
    heap.append(Entry(dist=1, node=2))
    top = heap.pop()
    print(top.dist + top.node)
"#;
    let mir = lower_source_to_mir(source).expect("record pop source should lower");
    let object = emit_host_object(&mir).expect("record pop source should compile directly");
    let referenced = exact_referenced_symbols(&object);
    assert!(
        referenced.contains("aura_direct_vec_pop_plain"),
        "a record pop should call the plain pop: {referenced:?}"
    );
    for forbidden in [
        "aura_direct_unbox_plain",
        "aura_direct_collection_operation",
    ] {
        assert!(
            !referenced.contains(forbidden),
            "a record pop must not reference `{forbidden}`: {referenced:?}"
        );
    }
}

const TEXT_SLICE_SOURCE: &str = r#"
def text_of(data: list[uint8], start: int64, end: int64) -> str:
    match own str.from_bytes(data[start:end]):
        case Result.Ok(text):
            return text
        case Result.Err(_):
            return ""


def error_of(data: list[uint8]) -> str:
    match own str.from_bytes(data[0:]):
        case Result.Ok(text):
            return text
        case Result.Err(error):
            return f"{error}"


def whole(data: list[uint8]) -> str:
    match str.from_bytes(data):
        case Result.Ok(text):
            return f"{text}"
        case Result.Err(_):
            return ""


def main() -> int32:
    data = "hello".to_bytes()
    print(text_of(data, 1, 3))
    print(error_of(data))
    print(whole(data))
    print(str.from_bytes(data[0:2]))
    return 0
"#;

fn text_slice_function(name: &str) -> MirFunction {
    lower_source_to_mir(TEXT_SLICE_SOURCE)
        .expect("the text slices should lower")
        .functions
        .into_iter()
        .find(|function| function.name == name)
        .expect("the function")
}

/// The temporary `function` assigns from a call whose callee mentions
/// `wanted`.
fn assigned_from_call(function: &MirFunction, wanted: &str) -> String {
    function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .find_map(|instruction| match instruction {
            Instruction::Assign {
                target,
                value: Rvalue::Call { callee, .. },
            } if format!("{callee:?}").contains(wanted) => Some(target.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("a call to `{wanted}`"))
}

#[test]
fn direct_text_from_a_byte_slice_makes_no_slice_and_no_result() {
    let text_of = text_slice_function("text_of");
    let slice = assigned_from_call(&text_of, "Slice");
    let result = assigned_from_call(&text_of, "Host");
    assert_eq!(
        super::lookups::text_byte_slices(&text_of)
            .into_iter()
            .collect::<Vec<_>>(),
        vec![slice]
    );
    // `match own` moves the `Result` into a second temporary, which is a
    // slot too.
    let slots = super::lookups::lookup_payload_slots(&text_of);
    assert_eq!(slots.len(), 2, "{slots:?}");
    assert!(slots.contains(&result), "{slots:?}");
    // A match that reads the error keeps the `Result`, and one that does
    // not move out keeps its slot.
    assert!(super::lookups::lookup_payload_slots(&text_slice_function("error_of")).is_empty());
    assert_eq!(
        super::lookups::lookup_payload_slots(&text_slice_function("whole")).len(),
        1
    );

    let mir = lower_source_to_mir(TEXT_SLICE_SOURCE).expect("the text slices should lower");
    let object = emit_host_object(&mir).expect("the text slices should compile directly");
    let referenced = exact_referenced_symbols(&object);
    assert!(
        referenced.contains("aura_direct_str_from_byte_slice"),
        "{referenced:?}"
    );
    // `main` releases the list between its slice and the call, so it
    // makes the slice and calls the builtin.
    assert!(
        referenced.contains("aura_direct_vec_slice"),
        "{referenced:?}"
    );
    assert!(
        referenced.contains("aura_host_str_from_bytes"),
        "{referenced:?}"
    );
}

#[test]
fn direct_text_slots_and_slices_reject_every_other_use() {
    let text_of = text_slice_function("text_of");
    let slice = assigned_from_call(&text_of, "Slice");
    let result = assigned_from_call(&text_of, "Host");
    let moved = text_of
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .find_map(|instruction| match instruction {
            Instruction::Assign {
                target,
                value: Rvalue::Use(Operand::MovePlace(source)),
            } if *source == result => Some(target.clone()),
            _ => None,
        })
        .expect("the move of the result");
    let with = |extra: Instruction| {
        let mut function = text_of.clone();
        let entry = function
            .blocks
            .iter_mut()
            .find(|block| block.label == function.entry)
            .expect("the entry block");
        entry.instructions.push(extra);
        function
    };

    // Another read of the moved slot rejects both ends of the move.
    let read = with(Instruction::Assign {
        target: "%t90".to_string(),
        value: Rvalue::Use(Operand::Place(moved.clone())),
    });
    assert!(super::lookups::lookup_payload_slots(&read).is_empty());
    // A move from a temporary that is no slot makes its target none.
    let other = with(Instruction::Assign {
        target: "%t91".to_string(),
        value: Rvalue::Use(Operand::MovePlace("%t2".to_string())),
    });
    assert!(!super::lookups::lookup_payload_slots(&other).contains("%t91"));

    // A slice read again, or kept, is made.
    let retained = with(Instruction::Retain {
        place: slice.clone(),
    });
    assert!(super::lookups::text_byte_slices(&retained).is_empty());
    let mut returned = text_of.clone();
    for block in &mut returned.blocks {
        if let Terminator::Return(value) = &mut block.terminator {
            *value = Operand::Place(slice.clone());
        }
    }
    assert!(super::lookups::text_byte_slices(&returned).is_empty());
    // A slice no `str.from_bytes` reads right after is made.
    assert!(super::lookups::text_byte_slices(&text_slice_function("main")).is_empty());
    assert!(super::lookups::text_byte_slices(&text_slice_function("whole")).is_empty());
    assert_eq!(
        super::lookups::text_byte_slices(&text_slice_function("error_of")).len(),
        1
    );
}

#[test]
fn direct_number_parses_matched_on_ok_keep_only_the_number() {
    let source = r#"
def to_int(text: str) -> int64:
    match parse_int64(text):
        case Result.Ok(value):
            return value
        case Result.Err(_):
            return -1


def why(text: str) -> str:
    match own parse_float64(text):
        case Result.Ok(_):
            return ""
        case Result.Err(error):
            return error


def main() -> int32:
    print(to_int("4"))
    print(why("x"))
    return 0
"#;
    let mir = lower_source_to_mir(source).expect("the parses should lower");
    let slots = |name: &str| {
        let function = mir
            .functions
            .iter()
            .find(|function| function.name == name)
            .expect("the function");
        super::lookups::lookup_payload_slots(function)
    };
    assert_eq!(slots("to_int").len(), 1);
    // A match that reads the error keeps the `Result`.
    assert!(slots("why").is_empty());
    let object = emit_host_object(&mir).expect("the parses should compile directly");
    let referenced = exact_referenced_symbols(&object);
    assert!(
        referenced.contains("aura_direct_parse_number_ok"),
        "{referenced:?}"
    );
    assert!(
        referenced.contains("aura_direct_parse_float64"),
        "{referenced:?}"
    );
    assert!(
        !referenced.contains("aura_direct_parse_int64"),
        "{referenced:?}"
    );
}
