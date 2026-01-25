use tos_common::crypto::{hash, Hash};
use tos_daemon::core::merkle::MerkleBuilder;

#[test]
fn test_merkle_single_element_root_equals_element() {
    let element = Hash::new([9u8; 32]);
    let mut builder = MerkleBuilder::from_iter([&element]);
    let root = builder.build();
    assert_eq!(root, element);
}

#[test]
fn test_merkle_odd_count_duplicates_last() {
    let h1 = Hash::new([1u8; 32]);
    let h2 = Hash::new([2u8; 32]);
    let h3 = Hash::new([3u8; 32]);

    let mut builder = MerkleBuilder::from_iter([&h1, &h2, &h3]);
    let root = builder.build();

    let left = hash(&[*h1.as_bytes(), *h2.as_bytes()].concat());
    let right = hash(&[*h3.as_bytes(), *h3.as_bytes()].concat());
    let expected = hash(&[*left.as_bytes(), *right.as_bytes()].concat());

    assert_eq!(root, expected);
}

#[test]
fn test_merkle_verify_matches_root() {
    let h1 = Hash::new([4u8; 32]);
    let h2 = Hash::new([5u8; 32]);

    let mut builder = MerkleBuilder::from_iter([&h1, &h2]);
    let root = builder.build();

    let mut verify_builder = MerkleBuilder::from_iter([&h1, &h2]);
    assert!(verify_builder.verify(&root));
}
