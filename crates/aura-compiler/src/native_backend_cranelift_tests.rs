use cranelift_codegen::settings::{Flags, OptLevel};

use super::native_codegen_flags;

#[test]
fn direct_backend_enables_stack_returns_for_flattened_mutable_writeback() {
    let flags: Flags = native_codegen_flags().expect("native flags should configure");
    assert!(flags.is_pic());
    assert!(flags.unwind_info());
    assert_eq!(flags.opt_level(), OptLevel::Speed);
    assert!(
        flags.enable_multi_ret_implicit_sret(),
        "flattened mutable receiver writeback can exceed x86-64's return registers"
    );
}
