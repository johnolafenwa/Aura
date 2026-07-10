#![no_main]

use aurora_compiler::{
    analyze_source, check_source, emit_host_native_object, lower_source_to_mir, parse_source,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };
    let _ = parse_source(source);
    let _ = check_source(source);
    let _ = analyze_source(source);
    if let Ok(mir) = lower_source_to_mir(source) {
        let _ = emit_host_native_object(&mir);
    }
});
