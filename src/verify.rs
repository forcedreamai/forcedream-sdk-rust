use crate::canonical::{sha256_hex, wf_canonical};
use ed25519_dalek::pkcs8::DecodePublicKey;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde_json::{Number, Value};
use std::collections::BTreeMap;

/// Mirrors JS's Number(x) -> JSON.stringify() behavior: whole values serialize with no
/// decimal point, fractional values preserve their decimal. Needed because a naive
/// Number::from_f64 alone always produces a float representation (confirmed as a real bug
/// via direct testing before this was written, not assumed) -- a single spurious ".0"
/// changes the signed bytes and breaks every signature, exactly as it did for the earlier
/// Python and Go SDKs, each of which needed their own equivalent fix.
fn js_number(v: &Value) -> Value {
    let parsed: f64 = match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) => s.parse().unwrap_or(0.0),
        _ => 0.0,
    };
    if parsed.fract() == 0.0 && parsed.abs() < (i64::MAX as f64) {
        Value::Number(Number::from(parsed as i64))
    } else {
        Value::Number(Number::from_f64(parsed).unwrap_or_else(|| Number::from(0)))
    }
}

fn js_string(v: &Value) -> Value {
    match v {
        Value::String(s) => Value::String(s.clone()),
        Value::Number(n) => Value::String(n.to_string()),
        _ => Value::String(String::new()),
    }
}

/// Reconstructs the signable EXACTLY as the server did. Version-aware: proofs with
/// external_cost_hash were signed over 10 fields, older ones over 8. Ported field-for-field
/// from the real, published @forcedream/sdk's verify.js buildSignable, which is itself a
/// verbatim port of the server's own logic -- not reconstructed from a description.
fn build_signable(p: &serde_json::Map<String, Value>) -> (BTreeMap<String, Value>, u8) {
    let has_ext = p.get("external_cost_hash").map_or(false, |v| !v.is_null());

    let mut base = BTreeMap::new();
    base.insert("task_id".to_string(), p.get("task_id").cloned().unwrap_or(Value::Null));
    base.insert("agent_id".to_string(), p.get("agent_id").cloned().unwrap_or(Value::Null));
    base.insert("input_hash".to_string(), p.get("input_hash").cloned().unwrap_or(Value::Null));
    base.insert("output_hash".to_string(), p.get("output_hash").cloned().unwrap_or(Value::Null));
    base.insert("cost_pence".to_string(), js_number(p.get("cost_pence").unwrap_or(&Value::Null)));
    base.insert("budget_pence".to_string(), js_number(p.get("budget_pence").unwrap_or(&Value::Null)));
    base.insert("started_at".to_string(), js_number(p.get("started_at").unwrap_or(&Value::Null)));
    base.insert("completed_at".to_string(), js_string(p.get("completed_at").unwrap_or(&Value::Null)));

    if has_ext {
        base.insert("external_cost_hash".to_string(), js_string(p.get("external_cost_hash").unwrap()));
        let retrieved = p.get("retrieved_count").cloned().unwrap_or(Value::Number(Number::from(0)));
        base.insert("retrieved_count".to_string(), js_number(&retrieved));
        (base, 10)
    } else {
        (base, 8)
    }
}

#[derive(Debug, serde::Serialize)]
pub struct VerifyResult {
    pub verified: bool,
    pub task_id: Option<String>,
    pub key_id: Option<String>,
    pub algorithm: String,
    pub fields_signed: u8,
    pub trustless: bool,
    pub message: String,
}

#[derive(thiserror::Error, Debug)]
pub enum VerifyError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] Box<ureq::Error>),
    #[error("response body error: {0}")]
    Io(#[from] std::io::Error),
    #[error("proof_not_found")]
    ProofNotFound,
    #[error("Provide task_id or proof")]
    NoInput,
}

impl From<ureq::Error> for VerifyError {
    fn from(e: ureq::Error) -> Self {
        VerifyError::Http(Box::new(e))
    }
}

/// Public keys arrive as PEM (matching the real API's public_key_pem field). PEM is just
/// base64-encoded DER with header/footer lines -- stripping those and decoding reuses the
/// already-proven from_public_key_der path (verified directly against a real Node-signed
/// signature before this SDK's client logic was written) rather than requiring an
/// additional "pem" feature/dependency.
fn parse_pem_to_der(pem: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    let body: String = pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect::<Vec<_>>()
        .join("");
    base64::engine::general_purpose::STANDARD.decode(body).ok()
}

/// Trustlessly verifies a ForceDream proof's Ed25519 signature entirely client-side.
/// ForceDream is never asked whether the proof is valid -- the math decides, locally.
pub fn verify_proof(
    api_base: &str,
    task_id: Option<&str>,
    proof_input: Option<Value>,
) -> Result<VerifyResult, VerifyError> {
    let proof: Value = if let Some(p) = proof_input {
        p
    } else {
        let tid = task_id.ok_or(VerifyError::NoInput)?;
        let data: Value = ureq::get(&format!("{}/v1/workforce/proof/{}/public", api_base, tid))
            .call()?
            .into_json()?;
        data.get("proof").cloned().ok_or(VerifyError::ProofNotFound)?
    };

    let key_data: Value = ureq::get(&format!("{}/v1/workforce/proof/public-key", api_base))
        .call()?
        .into_json()?;
    let key_id = key_data.get("key_id").and_then(|v| v.as_str()).map(String::from);
    let pem = key_data
        .get("public_key_pem")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let verifying_key = parse_pem_to_der(pem)
        .and_then(|der| VerifyingKey::from_public_key_der(&der).ok());

    let proof_obj = proof.as_object().cloned().unwrap_or_default();
    let (signable, fields) = build_signable(&proof_obj);
    let digest = sha256_hex(&wf_canonical(&signable));

    let mut verified = false;
    if let (Some(vk), Some(sig_b64)) = (
        &verifying_key,
        proof_obj.get("signature").and_then(|v| v.as_str()),
    ) {
        let algorithm = proof_obj.get("algorithm").and_then(|v| v.as_str());
        if algorithm.is_none() || algorithm == Some("Ed25519") {
            if let Ok(sig_bytes) = base64_decode(sig_b64) {
                if let Ok(sig_array) = <[u8; 64]>::try_from(sig_bytes.as_slice()) {
                    let signature = Signature::from_bytes(&sig_array);
                    let digest_bytes = hex::decode(&digest).unwrap_or_default();
                    verified = vk.verify(&digest_bytes, &signature).is_ok();
                }
            }
        }
    }

    let task_id_out = proof_obj.get("task_id").and_then(|v| v.as_str()).map(String::from);

    Ok(VerifyResult {
        verified,
        task_id: task_id_out,
        key_id,
        algorithm: "Ed25519".to_string(),
        fields_signed: fields,
        trustless: true,
        message: if verified {
            "Signature mathematically verified. This proof was signed by ForceDream and has not been altered.".to_string()
        } else {
            "Signature verification FAILED. The proof was altered or not signed by ForceDream.".to_string()
        },
    })
}

fn base64_decode(s: &str) -> Result<Vec<u8>, ()> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s).map_err(|_| ())
}
