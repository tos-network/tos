use tos_common::tns::validate_name_format;

#[test]
fn test_tns_invalid_names() {
    assert!(!validate_name_format("ab").valid);
    assert!(!validate_name_format("al!ce").valid);
    assert!(!validate_name_format("1alice").valid);
    assert!(!validate_name_format("alice.").valid);
    assert!(!validate_name_format("alice..bob").valid);
    assert!(!validate_name_format("alice__bob").valid);
}

#[test]
fn test_tns_valid_names() {
    assert!(validate_name_format("Alice").valid);
    assert!(validate_name_format("alice").valid);
    assert!(validate_name_format("alice-1").valid);
    assert!(validate_name_format("alice_1").valid);
    assert!(validate_name_format("alice.bob").valid);
}
