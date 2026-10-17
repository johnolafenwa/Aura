use super::{decode, encode, DESCRIPTOR_PREFIX};
use crate::types::Type;

#[test]
fn descriptors_round_trip_every_type_shape() {
    for ty in crate::types::tests::descriptor_corpus() {
        let descriptor = encode(&ty);
        assert!(descriptor.starts_with(DESCRIPTOR_PREFIX));
        let decoded = decode(&descriptor).unwrap_or_else(|error| panic!("{ty:?}: {error}"));
        assert_eq!(
            serde_json::to_value(&decoded).expect("types serialize"),
            serde_json::to_value(&ty).expect("types serialize"),
            "round trip of {descriptor}"
        );
    }
}

#[test]
fn malformed_descriptors_are_rejected() {
    let union = crate::types::tests::descriptor_corpus()
        .into_iter()
        .find(|ty| matches!(ty, Type::Union(_)))
        .expect("the corpus has a union");
    let union_descriptor = encode(&union);
    let one_member = format!("{DESCRIPTOR_PREFIX}U4:main1;3:keyu");
    let wrong_key = union_descriptor.replacen("int32", "int64", 1);
    let cases = [
        ("u".to_string(), "unknown version"),
        (format!("{DESCRIPTOR_PREFIX}"), "ends early"),
        (format!("{DESCRIPTOR_PREFIX}x"), "unknown type tag"),
        (format!("{DESCRIPTOR_PREFIX}uu"), "trailing bytes"),
        (format!("{DESCRIPTOR_PREFIX}n9:int"), "string ends early"),
        (format!("{DESCRIPTOR_PREFIX}n3:int"), "ends early"),
        (format!("{DESCRIPTOR_PREFIX}n3:int;"), "malformed number"),
        (format!("{DESCRIPTOR_PREFIX}n3:intx;"), "malformed number"),
        (
            format!("{DESCRIPTOR_PREFIX}t99999999999999999999999;"),
            "overflows",
        ),
        (format!("{DESCRIPTOR_PREFIX}t500;u"), "count exceeds"),
        (format!("{DESCRIPTOR_PREFIX}v2;u"), "malformed flag"),
        (
            format!("{DESCRIPTOR_PREFIX}f1;03:valx0u"),
            "unknown passing",
        ),
        (format!("{DESCRIPTOR_PREFIX}cx0;u0;"), "unknown call kind"),
        (
            format!("{DESCRIPTOR_PREFIX}cr0;u1;1:xz1;1;0u"),
            "unknown capture mode",
        ),
        (one_member, "at least two members"),
        (wrong_key, "member key does not match"),
        (
            format!("{DESCRIPTOR_PREFIX}{}u", "t1;".repeat(300)),
            "nests too deeply",
        ),
    ];
    for (descriptor, expected) in cases {
        let error = decode(&descriptor).expect_err(&descriptor);
        assert!(error.contains(expected), "`{descriptor}` gave `{error}`");
    }
    let split = format!("{DESCRIPTOR_PREFIX}p1:é");
    assert!(decode(&split).unwrap_err().contains("not UTF-8"));
}
