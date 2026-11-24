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
    let runtime_init = "class Limit:\n    value: int32\n    def __init__(self, input: int32):\n        self.value = input + 1\nmut limit = Limit(5)\n";
    assert_eq!(program(runtime_init).unwrap_err().code, "AU2080");
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
    let borrow = "import counter\nexport \"C\" def invalid() -> uint16:\n    view saved = counter.ticks\n    counter.ticks = 2\n    return saved\n";
    let loan = check_path_with_source_without_lockfile_profile(&entry, borrow, true).unwrap_err();
    std::fs::remove_dir_all(base).unwrap();
    assert_eq!(loan.code, "AU3030");
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
    assert_eq!(ring.bytes, [0; 8]);
    assert_eq!(ring.alignment, 2);
    assert_eq!(
        checked.constants["selectors"].state.as_ref().unwrap().bytes,
        [1, 0, 0, 0, 3, 0, 0, 0]
    );
    let mir = lower_to_mir(&checked);
    for target in FreestandingTarget::ALL {
        emit_freestanding_object(&mir, "ring.au", source, target).unwrap();
    }
}

#[test]
fn packed_module_record_uses_c_offsets_alignment_and_shared_bit_storage() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/freestanding/module_state/packed/src/main.au");
    let source = std::fs::read_to_string(&path).unwrap();
    let mir = lower_to_mir(
        &check_path_with_source_without_lockfile_profile(&path, &source, true).unwrap(),
    );
    for target in FreestandingTarget::ALL {
        let bytes = emit_freestanding_object(&mir, "main.au", &source, target).unwrap();
        let obj = object::File::parse(&bytes[..]).unwrap();
        let header = obj
            .symbols()
            .find(|s| {
                s.name()
                    .is_ok_and(|s| s.contains("__aura_state_") && s.ends_with("686561646572"))
            })
            .unwrap();
        let section = obj
            .section_by_index(header.section_index().unwrap())
            .unwrap();
        let offset = (header.address() - section.address()) as usize;
        assert!(section.align() >= 16);
        assert_eq!(header.size(), 16);
        assert_eq!(
            &section.data().unwrap()[offset..offset + 16],
            &[5, 0x44, 0x33, 0x22, 0x11, 0x2b, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
        let word = obj
            .symbols()
            .find(|s| {
                s.name()
                    .is_ok_and(|s| s.contains("__aura_state_") && s.ends_with("776f7264"))
            })
            .unwrap();
        assert_eq!(word.size(), 4);
        assert_eq!(
            obj.section_by_index(word.section_index().unwrap())
                .unwrap()
                .kind(),
            object::SectionKind::UninitializedData
        );
    }
}

#[test]
fn literal_device_addresses_preserve_uint64_values() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/freestanding/module_state/packed/src/main.au");
    let source = "public mut base: ptr[uint8] = 18446744073709551615 as ptr[uint8]\nexport \"C\" def base_address() -> uint64:\n    return base as uint64\n";
    let checked = check_path_with_source_without_lockfile_profile(&path, source, true).unwrap();
    assert_eq!(
        checked.constants["base"].state.as_ref().unwrap().bytes,
        u64::MAX.to_le_bytes()
    );
}

#[test]
fn tagged_module_state_packs_fixed_leaves_as_little_endian_words() {
    let source = include_str!("../../tests/fixtures/freestanding/module_state/tagged_fixed.au");
    let checked = program(source).unwrap();
    let active = &checked.constants["packet"].state.as_ref().unwrap().bytes;
    assert_eq!(active.len(), 32);
    assert_eq!(&active[8..16], &(-2_i64).to_le_bytes());
    assert_eq!(
        &active[16..],
        &[1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 0, 0, 0, 0, 0, 0]
    );
    let empty = &checked.constants["empty"].state.as_ref().unwrap().bytes;
    assert_eq!(empty.len(), 32);
    assert_eq!(&empty[8..], &[0; 24]);
    for target in FreestandingTarget::ALL {
        emit_freestanding_object(&lower_to_mir(&checked), "tagged_fixed.au", source, target)
            .unwrap();
    }
}

#[test]
fn static_initializer_uses_the_imported_record_definition() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/freestanding/module_state/packed/src/main.au");
    let source = "import records\nmut saved: records.Record = records.Record(tag=7, count=67305985)\nexport \"C\" def read() -> uint32:\n    return saved.count\n";
    let checked = check_path_with_source_without_lockfile_profile(&path, source, true).unwrap();
    let image = checked.constants["saved"].state.as_ref().unwrap();
    assert_eq!(image.bytes, [7, 1, 2, 3, 4]);
    assert_eq!(image.alignment, 1);
    emit_freestanding_object(
        &lower_to_mir(&checked),
        "main.au",
        source,
        FreestandingTarget::Aarch64,
    )
    .unwrap();
}

#[test]
fn inline_union_fields_contribute_their_tag_and_payload_to_outer_unions() {
    let source = "copy class Reading:\n    value: int32 | None\nmut current: Reading | None = Reading(value=-7)\nexport \"C\" def clear():\n    current = None\n";
    let checked = program(source).unwrap();
    let bytes = &checked.constants["current"].state.as_ref().unwrap().bytes;
    assert_eq!(bytes.len(), 24);
    assert_eq!(&bytes[16..], &(-7_i64).to_le_bytes());
    emit_freestanding_object(
        &lower_to_mir(&checked),
        "nested.au",
        source,
        FreestandingTarget::X86_64,
    )
    .unwrap();
}

#[test]
fn tagged_state_unpacks_c_bitfields_and_inline_tail_in_register_order() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/freestanding/module_state/packed/src/main.au");
    let source = r#"extern "C" packed class Mixed:
    signed: int16
    real: float32
    low: uint8 bits(3)
    high: uint16 bits(7)
    tail: fixed[uint8, 9]

mut sample: Mixed | uint128 = Mixed(signed=-513, real=-2.5, low=5, high=103, tail=[1,2,3,4,5,6,7,8,9])
mut initial: Mixed | uint128 = 18446744073709551619

export "C" def reset():
    sample = initial
"#;
    let checked = check_path_with_source_without_lockfile_profile(&path, source, true).unwrap();
    let words = |name: &str| {
        checked.constants[name]
            .state
            .as_ref()
            .unwrap()
            .bytes
            .chunks_exact(8)
            .map(|word| u64::from_le_bytes(word.try_into().unwrap()))
            .collect::<Vec<_>>()
    };
    let sample = words("sample");
    assert_eq!(
        &sample[1..],
        &[
            (-513_i64) as u64,
            (-2.5_f64).to_bits(),
            5,
            103,
            0x0807_0605_0403_0201,
            9
        ]
    );
    let initial = words("initial");
    assert_ne!(initial[0], sample[0]);
    assert_eq!(&initial[1..], &[3, 1, 0, 0, 0, 0]);
}

#[test]
fn address_of_zeroed_module_state_preserves_its_object_on_each_target() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/freestanding/module_state/packed/src/main.au");
    let source = "import mem\nmut flag: None = None\nmut register: uint32 = 0\nexport \"C\" def status_address() -> uint64:\n    flag = None\n    return mem.address_of(register) as uint64\n";
    let checked = check_path_with_source_without_lockfile_profile(&path, source, true).unwrap();
    let mir = lower_to_mir(&checked);
    for target in FreestandingTarget::ALL {
        let bytes = emit_freestanding_object(&mir, "main.au", source, target).unwrap();
        let obj = object::File::parse(&bytes[..]).unwrap();
        let mut sizes = obj
            .symbols()
            .filter(|s| s.name().is_ok_and(|n| n.contains("__aura_state_")))
            .map(|s| {
                let section = obj.section_by_index(s.section_index().unwrap()).unwrap();
                assert_eq!(section.kind(), object::SectionKind::UninitializedData);
                s.size()
            })
            .collect::<Vec<_>>();
        sizes.sort_unstable();
        assert_eq!(sizes, [1, 4], "{target:?}");
    }
}

#[test]
fn hosted_engines_refuse_freestanding_module_operations() {
    let source = "mut count: int32 = 0\nexport \"C\" def read_count() -> int32:\n    return count\ndef main() -> int32:\n    return read_count()\n";
    let mut mir = lower_to_mir(&program(source).unwrap());
    let error = crate::emit_host_native_object(&mir).unwrap_err();
    assert!(
        error.contains("mutable module storage requires freestanding execution"),
        "{error}"
    );

    // Embedded MIR can reach a cell operation even when its declaration is absent.
    mir.constants.clear();
    let data = serde_json::to_vec(&mir).unwrap();
    let error = crate::run_serialized_mir(&data, "main.au", source).unwrap_err();
    assert_eq!(error.code, "AU4030", "{error:?}");
    let errors =
        emit_freestanding_object(&mir, "main.au", source, FreestandingTarget::X86_64).unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("missing module storage")),
        "{errors:?}"
    );
}
