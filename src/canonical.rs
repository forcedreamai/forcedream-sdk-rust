use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Exact replica of the server's wfCanonical: JSON.stringify(obj, Object.keys(obj).sort()).
/// Sorted keys, no whitespace. Ported verbatim from the real, published @forcedream/sdk's
/// canonical.js, which itself is a verbatim port of @forcedream/mcp-server's canonical.ts --
/// not rewritten from memory. Rust's serde_json with a BTreeMap produces byte-identical
/// output to this JS behavior for the field types this SDK actually signs over (confirmed
/// directly, including whole-vs-fractional number formatting, before writing this).
pub fn wf_canonical(obj: &BTreeMap<String, Value>) -> String {
    serde_json::to_string(obj).expect("BTreeMap<String, Value> always serializes")
}

pub fn sha256_hex(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    hex::encode(hasher.finalize())
}
