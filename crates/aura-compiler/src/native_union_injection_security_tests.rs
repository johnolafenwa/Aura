use super::*;

#[test]
fn direct_union_injection_consumes_the_tracked_payload_registration() {
    let union = Type::normalize_union(
        vec![Type::named("int64"), Type::named("str")],
        "",
        &BTreeMap::new(),
    )
    .expect("two distinct members form a union");
    let encoded = canonical_runtime_type_name(&union);

    let stale_payload_registration = with_direct_task_runtime_scope(|| {
        let payload = aura_direct_box_i64(42);
        // Keep one control reference live so the red implementation cannot
        // leave this test inspecting a freed allocation address.
        let control = unsafe { aura_direct_retain_value(payload) };
        let result = aura_direct_union_inject(encoded.as_ptr(), encoded.len(), 0, payload);
        let remaining_registrations = with_direct_task_runtime_state(|state| {
            state
                .owned_value_refs
                .get(&(payload as usize))
                .copied()
                .unwrap_or_default()
        });

        // The control reference owns the one expected remaining registration.
        // Remove only the red path's stale consumed registration without
        // releasing the allocation a second time.
        if remaining_registrations > 1 {
            assert!(unregister_direct_owned_value(payload));
        }
        unsafe { aura_direct_release_value(control) };
        unsafe { aura_direct_release_value(result) };
        remaining_registrations != 1
    });

    assert!(
        !stale_payload_registration,
        "the immediate owned ABI callee must unregister payload ownership before consuming it"
    );
}
