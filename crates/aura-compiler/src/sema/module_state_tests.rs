use crate::mir::Rvalue;
use crate::{
    check_freestanding_path, check_path_with_source_without_lockfile_profile,
    emit_freestanding_object, lower_to_mir, FreestandingTarget,
};
use object::{Object, ObjectSection, ObjectSymbol};

const COUNTER: &str = include_str!("../../tests/fixtures/freestanding/module_state/counter.au");
const HOSTED: &str = include_str!("../../tests/fixtures/check-fail/hosted_module_state.au");
const BORROW: &str = include_str!("../../tests/fixtures/freestanding/module_state/borrow.au");
const INITIALIZER: &str =
    include_str!("../../tests/fixtures/freestanding/module_state/initializer.au");

fn program(source: &str) -> crate::Result<super::Program> {
    check_freestanding_path(std::path::Path::new("state-test.au"), Some(source))
}

#[test]
fn profile_preserves_script_locals_and_rejects_shared_hosted_cells() {
    let syntax = crate::parse_source(include_str!(
        "../../tests/fixtures/parse-pass/public_module_state.au"
    ))
    .unwrap();
    assert!(syntax.constants[0].public && syntax.constants[0].mutable);
    let path = std::path::Path::new("state-test.au");
    let error = check_path_with_source_without_lockfile_profile(path, HOSTED, false).unwrap_err();
    assert_eq!(error.code, "AU3030");
    assert!(error.help[0].contains("sync.Mutex"));
    assert!(check_path_with_source_without_lockfile_profile(path, HOSTED, true).is_ok());
    assert!(crate::check_source("mut i = 0\ni += 1\nprint(i)\n").is_ok());
}

#[test]
fn initializer_and_state_view_have_actionable_diagnostics() {
    let init = program(INITIALIZER).unwrap_err();
    assert_eq!(init.code, "AU2080");
    let borrow = program(BORROW).unwrap_err();
    assert_eq!(borrow.code, "AU3030");
    let source = "mut n: int32 = 2\ndef set(n: mut int32):\n    n = 3\nexport \"C\" def unsafe_call():\n    set(n)\n";
    assert_eq!(program(source).unwrap_err().code, "AU3030");
}

#[test]
fn scalar_and_aggregate_module_state_occupy_writable_data_sections() {
    let program = program(COUNTER).unwrap();
    let mir = lower_to_mir(&program);
    assert_eq!(
        mir.constants.iter().filter(|c| c.state.is_some()).count(),
        2
    );
    assert!(mir
        .functions
        .iter()
        .flat_map(|f| &f.blocks)
        .flat_map(|b| &b.instructions)
        .any(|i| {
            matches!(
                i,
                crate::mir::Instruction::Assign {
                    value: Rvalue::ModuleStore { .. },
                    ..
                }
            )
        }));
    for target in FreestandingTarget::ALL {
        let bytes = emit_freestanding_object(&mir, "state-test.au", COUNTER, target).unwrap();
        let obj = object::File::parse(&bytes[..]).unwrap();
        let kinds = obj
            .symbols()
            .filter(|s| s.name().unwrap_or_default().contains("__aura_state_"))
            .filter_map(|s| {
                s.section_index()
                    .map(|idx| obj.section_by_index(idx).unwrap().kind())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            kinds.len(),
            2,
            "each binding has one addressable symbol on {target:?}"
        );
        assert!(
            kinds.contains(&object::SectionKind::Data),
            "{target:?}: {kinds:?}"
        );
        assert!(
            kinds.contains(&object::SectionKind::UninitializedData),
            "{target:?}: {kinds:?}"
        );
    }
}

#[test]
fn safe_reads_and_writes_through_imports_share_one_module_cell() {
    let base =
        std::env::temp_dir().join(format!("aura-module-state-import-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let entry = base.join("main.au");
    let state = base.join("counter.au");
    std::fs::write(&state, "public mut ticks: uint16 = 1\n").unwrap();
    let source = "import counter\nfrom counter import ticks\nexport \"C\" def next() -> uint16:\n    counter.ticks += 2\n    ticks += 3\n    return counter.ticks\n";
    std::fs::write(&entry, source).unwrap();
    let result = check_freestanding_path(&entry, None);
    std::fs::remove_dir_all(base).unwrap();
    let mir = lower_to_mir(&result.unwrap());
    assert_eq!(
        mir.constants.iter().filter(|c| c.state.is_some()).count(),
        1
    );
    emit_freestanding_object(&mir, "main.au", source, FreestandingTarget::X86_64).unwrap();
}

#[test]
fn module_state_images_preserve_literal_bits_defaults_and_union_tags() {
    let source = r#"
copy class Position:
    small: int8 = -2
    wide: float64

mut position: Position = Position(wide=-0.0)
mut ready: int16 | None = None

export "C" def move() -> int8:
    position.small = -4
    ready = -9
    snapshot = ready
    if snapshot is None:
        return 0
    return snapshot as int8
"#;
    let checked = program(source).unwrap();
    let image = checked.constants["position"].state.as_ref().unwrap();
    assert_eq!(image.bytes[0], 254);
    assert_eq!(image.alignment, 8);
    assert_eq!(&image.bytes[8..], &(-0.0_f64).to_le_bytes());
    for target in FreestandingTarget::ALL {
        emit_freestanding_object(&lower_to_mir(&checked), "state-test.au", source, target).unwrap();
    }
}

#[test]
fn literal_layout_covers_scalar_widths_and_nested_aggregates() {
    let source = r#"
copy class Pair:
    low: uint8
    high: uint16

copy class Values:
    narrow: Pair
    signed: int32
    real: float32

mut sample = Values(narrow=Pair(low=1, high=65535), signed=-10, real=-2.5)
mut running: bool = true
mut mask: uint128 = 340282366920938463463374607431768211455
mut whole: float64 = 42
export "C" def get() -> bool:
    return running
"#;
    let checked = program(source).unwrap();
    let cell = |name: &str| checked.constants[name].state.as_ref().unwrap();
    assert_eq!(cell("sample").alignment, 4);
    let bytes = &cell("sample").bytes;
    assert_eq!(&bytes[..4], &[1, 0, 255, 255]);
    assert_eq!(&bytes[4..8], &(-10_i32).to_le_bytes());
    assert_eq!(&bytes[8..], &(-2.5_f32).to_le_bytes());
    assert_eq!(cell("running").bytes, [1]);
    assert_eq!(cell("mask").bytes, [255; 16]);
    assert_eq!(cell("whole").bytes, 42_f64.to_le_bytes());
    assert_eq!(
        crate::run_mir(&lower_to_mir(&checked)).unwrap_err().code,
        "AU4030"
    );
}

#[test]
fn returned_views_cannot_borrow_module_state_through_calls() {
    let source = "mut ticks: uint64 = 1\ndef project(x: uint64) -> view uint64 from x:\n    return view x\nexport \"C\" def update() -> uint64:\n    view value = project(ticks)\n    ticks += 1\n    return value\n";
    let diagnostic = program(source).unwrap_err();
    assert_eq!(diagnostic.code, "AU3030", "{diagnostic:?}");
}

#[test]
fn fixed_module_elements_have_inline_storage_on_each_target() {
    let source = include_str!("../../tests/fixtures/freestanding/module_state/fixed.au");
    let checked = program(source).unwrap();
    let ring = checked.constants["ring"].state.as_ref().unwrap();
    assert_eq!(ring.bytes,[0;8]);
    assert_eq!(ring.alignment,2);
    assert_eq!(checked.constants["selectors"].state.as_ref().unwrap().bytes,[1,0,0,0,3,0,0,0]);
    let mir = lower_to_mir(&checked);
    for target in FreestandingTarget::ALL {
        emit_freestanding_object(&mir,"ring.au", source,target).unwrap();
    }
}
