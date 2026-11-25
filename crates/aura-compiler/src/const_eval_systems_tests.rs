use super::*;

#[test]
fn mutable_module_storage_and_its_readers_stay_out_of_frozen_data() {
    let source =
        "mut ticks: uint32 = 17\ninitial = read()\ndef read() -> uint32:\n    return ticks\n";
    let check = |source: &str| {
        crate::check_freestanding_path(std::path::Path::new("const-state.au"), Some(source))
    };
    let program = check(source).unwrap();
    assert!(program.frozen_constants.is_empty());
    let required = source.replace("initial =", "const initial =");
    assert_eq!(check(&required).unwrap_err().code, "AU3025");
}

#[test]
fn computed_integer_bits_and_local_fixed_arrays_obey_constant_evaluation_limits() {
    let source = "def read() -> uint16:\n    mut a: fixed[uint16, 3] = [1] * 3\n    a[1] = 3\n    return a[1].rotate_left(2)\nconst answer = read()\n";
    let program = check_source(source).unwrap();
    assert_eq!(
        program.frozen_constants["<main>::answer"].thaw().render(),
        "12"
    );
    let oversized = source
        .replace("uint16, 3", "uint16, 100000")
        .replace("* 3", "* 100000");
    assert_eq!(check_source(&oversized).unwrap_err().code, "AU3028");
}

#[test]
fn pointer_reads_never_run_in_the_compiler() {
    let check = |source| crate::check_module_with_builtin_imports(crate::parse_source(source)?);
    let source = "def read(p: ptr[int32]) -> int32:\n    return p.read()\nobserved = read(0 as ptr[int32])\n";
    assert!(!check(source)
        .unwrap()
        .frozen_constants
        .contains_key("<main>::observed"));
    assert_eq!(
        check(&source.replace("observed =", "const observed ="))
            .unwrap_err()
            .code,
        "AU3025"
    );
}

#[test]
fn inline_assembly_never_runs_in_the_constant_interpreter() {
    let target = crate::FreestandingTarget::X86_64;
    let config = crate::BuildConfig::for_target(target.name(), false).unwrap();
    let source = r#"def machine_value() -> int64:
    asm("nop")
    return 7

const answer = machine_value()
"#;
    let error = crate::with_build_config(config, || {
        crate::with_freestanding_target(target, || {
            crate::check_module_with_builtin_imports(crate::parse_source(source).unwrap())
                .unwrap_err()
        })
    });
    assert_eq!(error.code, "AU3025", "{error:?}");
}
