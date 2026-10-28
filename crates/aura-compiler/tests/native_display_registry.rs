#![cfg(coverage)]

//! The direct backend's `Display` registry is process-wide, so this test runs
//! in its own test binary where registering thunks cannot change how other
//! tests render values.

use aura_compiler::native_runtime_coverage::*;
use aura_compiler::Value;

unsafe extern "C-unwind" fn display_probe_thunk(args: *const i64, len: usize) -> *mut OpaqueValue {
    let args = unsafe { std::slice::from_raw_parts(args, len) };
    let argument = args[0] as *mut OpaqueValue;
    let rendered = match unsafe { aura_direct_coverage_clone_value(argument) } {
        Value::Instance(instance) => format!("<{}>", instance.class_name),
        Value::EnumVariant(variant) => format!("<{}.{}>", variant.enum_name, variant.variant_name),
        other => panic!("display thunks receive nominal values, found {other:?}"),
    };
    unsafe { aura_direct_release_value(argument) };
    aura_direct_string_literal(rendered.as_ptr(), rendered.len())
}

fn instance(class_name: &str) -> *mut OpaqueValue {
    aura_direct_instance_empty(class_name.as_ptr(), class_name.len())
}

fn take_string(value: *mut OpaqueValue) -> String {
    let text = match unsafe { aura_direct_coverage_clone_value(value) } {
        Value::String(text) => text.into_string(),
        other => panic!("expected a string, found {other:?}"),
    };
    unsafe { aura_direct_release_value(value) };
    text
}

fn stringify(value: *mut OpaqueValue) -> String {
    let rendered = take_string(aura_direct_stringify_value(value));
    unsafe { aura_direct_release_value(value) };
    rendered
}

#[test]
fn direct_display_registry_renders_registered_nominal_types_at_any_depth() {
    let plain = instance("DisplayProbeBox");
    assert_eq!(stringify(plain), "DisplayProbeBox()");

    for name in ["DisplayProbeBox", "DisplayProbeColor"] {
        aura_direct_register_display(
            name.as_ptr(),
            name.len(),
            display_probe_thunk as *const () as usize as i64,
        );
    }

    assert_eq!(stringify(instance("DisplayProbeBox")), "<DisplayProbeBox>");
    assert_eq!(stringify(instance("Unregistered")), "Unregistered()");

    let (enum_name, variant_name) = ("DisplayProbeColor", "Red");
    let color = aura_direct_enum_variant(
        enum_name.as_ptr(),
        enum_name.len(),
        variant_name.as_ptr(),
        variant_name.len(),
        std::ptr::null_mut(),
        0,
    );
    assert_eq!(stringify(color), "<DisplayProbeColor.Red>");

    let mut elements = vec![
        instance("DisplayProbeBox") as i64,
        aura_direct_box_i64(1) as i64,
    ];
    let tuple = aura_direct_tuple_new(elements.as_mut_ptr(), elements.len() as i64);
    std::mem::forget(elements);
    assert_eq!(stringify(tuple), "(<DisplayProbeBox>, 1)");

    let boxed = instance("DisplayProbeBox");
    let (spec, type_name) = (">20", "DisplayProbeBox");
    let formatted = take_string(aura_direct_format_value(
        boxed,
        spec.as_ptr(),
        spec.len(),
        type_name.as_ptr(),
        type_name.len(),
    ));
    unsafe { aura_direct_release_value(boxed) };
    assert_eq!(formatted, "   <DisplayProbeBox>");
}
