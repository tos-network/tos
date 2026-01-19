use serde::Serialize;
use serde_json::Value;
use sha3::{Digest, Sha3_256};

use crate::crypto::Hash;

pub fn canonicalize_json_value(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<_> = std::mem::take(map).into_iter().collect();
            entries.sort_by(|(a, _), (b, _)| a.cmp(b));
            for (k, mut v) in entries {
                canonicalize_json_value(&mut v);
                map.insert(k, v);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                canonicalize_json_value(item);
            }
        }
        _ => {}
    }
}

pub fn canonical_json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
    let mut json = serde_json::to_value(value)?;
    canonicalize_json_value(&mut json);
    serde_json::to_vec(&json)
}

pub fn canonical_hash_bytes<T: Serialize>(value: &T) -> Result<[u8; 32], serde_json::Error> {
    let bytes = canonical_json_bytes(value)?;
    let mut hasher = Sha3_256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    Ok(digest.into())
}

pub fn canonical_hash<T: Serialize>(value: &T) -> Result<Hash, serde_json::Error> {
    let bytes = canonical_hash_bytes(value)?;
    Ok(Hash::new(bytes))
}

pub fn canonical_hash_from_bytes(bytes: &[u8]) -> Hash {
    let mut hasher = Sha3_256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    Hash::new(digest.into())
}

pub fn canonical_hash_without_signature<T: Serialize>(
    value: &T,
    signature_field: &str,
) -> Result<Hash, serde_json::Error> {
    let mut json = serde_json::to_value(value)?;
    if let Value::Object(map) = &mut json {
        map.remove(signature_field);
    }
    canonical_hash(&json)
}
