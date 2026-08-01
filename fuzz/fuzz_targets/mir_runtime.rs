#![no_main]

use aura_compiler::{emit_host_native_object, run_mir, MirModule};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(module) = serde_json::from_slice::<MirModule>(data) else {
        return;
    };
    let _ = emit_host_native_object(&module);
    let _ = run_mir(&module);
});
