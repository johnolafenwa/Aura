use std::path::PathBuf;

use aura_core::layout::{decode_descriptor, LayoutKind};

use super::LayoutPlan;
use crate::sema::Type;

const SHAPES: &str = r#"
class Point:
    x: float64
    y: int32

enum Tree:
    Leaf(int64)
    Node(Tree, Tree)

class Holder[T]:
    value: T

def total(tree: Tree) -> int64:
    return match tree:
        case Tree.Leaf(value): value
        case Tree.Node(left, right): total(left) + total(right)

def main():
    point = Point(x=1.0, y=2)
    tree = Tree.Node(Tree.Leaf(1), Tree.Leaf(2))
    holder = Holder[int32](value=3)
    either: int64 | str = 4
    print(point.y)
    print(total(tree))
    print(holder.value)
    print(either)
"#;

fn plan_of(source: &str) -> LayoutPlan {
    let module = crate::lower_source_to_mir(source).expect("the source lowers");
    LayoutPlan::for_module(&module).expect("every type has a layout")
}

fn rendered(plan: &LayoutPlan, ty: &Type) -> String {
    let id = plan
        .layout_of(ty)
        .unwrap_or_else(|| panic!("the module uses `{ty}`"));
    plan.table().render(id)
}

#[test]
fn classes_enums_and_unions_get_native_layouts() {
    let plan = plan_of(SHAPES);
    assert_eq!(
        rendered(&plan, &Type::named("Point")),
        "Point@16/8:struct Point{x+0=float64@8/8:float64;y+8=int32@4/4:int32;}"
    );
    // The tree holds itself through a pointer to a heap block.
    assert_eq!(
        rendered(&plan, &Type::named("Tree")),
        "Tree@24/8:tagged Tree tag1 payload+8{Leaf(0+8=int64@8/8:int64;)\
         Node(0+8=Tree@8/8:indirect(^1);1+16=Tree@8/8:indirect(^1);)}"
    );
    let holder = Type::Named("Holder".to_string(), vec![Type::named("int32")]);
    assert_eq!(
        rendered(&plan, &holder),
        "Holder[int32]@4/4:struct Holder{value+0=int32@4/4:int32;}"
    );
    let either = plan
        .types()
        .find(|(ty, _)| matches!(ty, Type::Union(_)))
        .map(|(_, id)| plan.table().render(id))
        .expect("the module uses a union");
    assert!(
        either.starts_with("int64 | str@16/8:tagged | tag1 payload+8{"),
        "{either}"
    );
    assert_eq!(rendered(&plan, &Type::named("str")), "str@8/8:handle");
}

#[test]
fn every_runnable_fixture_type_has_a_layout_the_runtime_recomputes() {
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

    let (mut structs, mut tagged, mut indirect) = (0, 0, 0);
    for path in paths {
        let module = crate::lower_path_to_mir(&path)
            .unwrap_or_else(|error| panic!("{} should lower to MIR: {error}", path.display()));
        let plan = LayoutPlan::for_module(&module)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        for id in plan.table().ids() {
            match plan.table().get(id).kind {
                LayoutKind::Struct { .. } => structs += 1,
                LayoutKind::Tagged { .. } => tagged += 1,
                LayoutKind::Indirect(_) => indirect += 1,
                _ => {}
            }
        }
        // Generated code hands the runtime each type's layouts in its
        // descriptor, which lays the type out on its own, and the runtime
        // must read the plan's layout back.
        for (ty, id) in plan.types() {
            let descriptor = plan
                .descriptor(ty)
                .unwrap_or_else(|error| panic!("{}: `{ty}`: {error}", path.display()));
            let (decoded, root) = decode_descriptor(&descriptor)
                .unwrap_or_else(|error| panic!("{}: `{ty}`: {error}", path.display()));
            assert_eq!(&decoded.get(root).ty, ty, "{}", path.display());
            assert_eq!(
                decoded.render(root),
                plan.table().render(id),
                "{}: `{ty}`",
                path.display()
            );
        }
    }
    // The fixtures use classes, tuples, enums, unions, and recursive types.
    assert!(structs > 100, "{structs} structs");
    assert!(tagged > 100, "{tagged} tagged layouts");
    assert!(indirect > 0, "{indirect} indirect layouts");
}
