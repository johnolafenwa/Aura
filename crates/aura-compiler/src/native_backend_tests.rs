use std::ffi::OsStr;

use super::{code_generator_for, CodeGenerator};

#[test]
fn unset_empty_and_cranelift_settings_select_cranelift() {
    for setting in [None, Some(OsStr::new("")), Some(OsStr::new("cranelift"))] {
        assert_eq!(
            code_generator_for(setting),
            Ok(CodeGenerator::Cranelift),
            "{setting:?}"
        );
    }
    assert_eq!(CodeGenerator::Cranelift.name(), "cranelift");
}

#[test]
fn other_settings_name_both_code_generators() {
    let error = code_generator_for(Some(OsStr::new("gcc"))).unwrap_err();
    assert_eq!(
        error,
        "AURA_NATIVE_CODEGEN must be `cranelift` or `llvm`, not `gcc`"
    );
}

#[cfg(not(feature = "llvm"))]
#[test]
fn llvm_setting_needs_the_llvm_feature() {
    let error = code_generator_for(Some(OsStr::new("llvm"))).unwrap_err();
    assert!(
        error.contains("built without the `llvm` feature"),
        "{error}"
    );
}

#[cfg(feature = "llvm")]
#[test]
fn llvm_setting_selects_llvm() {
    assert_eq!(
        code_generator_for(Some(OsStr::new("llvm"))),
        Ok(CodeGenerator::Llvm)
    );
    assert_eq!(CodeGenerator::Llvm.name(), "llvm");
}
