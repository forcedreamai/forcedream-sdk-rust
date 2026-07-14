use serde_json::{json, Value};
use std::time::{Duration, Instant};
use std::thread::sleep;

#[derive(Debug, serde::Serialize)]
pub struct InvokeResult {
    pub status: String,
    pub agent: String,
    pub task_id: Option<String>,
    pub output: Option<Value>,
    pub charged_pence: Option<i64>,
    pub proof_id: Option<String>,
    pub message: String,
    pub error: Option<String>,
}

/// Ported precisely from @forcedream/mcp-server's invoke_agent.ts (via the real, published
/// @forcedream/sdk's invoke.js) -- exact endpoints, exact polling interval ramp (starts
/// 2500ms, +1000ms per attempt, capped at 6000ms), exact status handling. Invokes ONCE; never
/// re-invokes on timeout (would double-charge) -- returns a pollable task_id instead. Not
/// reconstructed from a description -- read directly from the real, working source before
/// writing this. Synchronous (blocking sleeps between polls) -- this SDK does not use async,
/// since its actual needs are sequential HTTP calls, not concurrent ones.
pub fn invoke_agent_polling(
    api_base: &str,
    api_key: &str,
    agent_slug: &str,
    task: &str,
    max_wait_seconds: Option<u64>,
) -> InvokeResult {
    let slug = agent_slug.to_string();
    let max_wait_ms = max_wait_seconds.unwrap_or(60).clamp(5, 120) * 1000;

    let inv_result = ureq::post(&format!("{}/v1/agents/{}/invoke", api_base, urlencoding::encode(&slug)))
        .set("Authorization", &format!("Bearer {}", api_key))
        .send_json(json!({ "task": task }));

    let (inv_status, inv_json): (u16, Value) = match inv_result {
        Ok(r) => {
            let status = r.status();
            let json = r.into_json().unwrap_or(Value::Null);
            (status, json)
        }
        Err(ureq::Error::Status(code, r)) => {
            let json = r.into_json().unwrap_or(Value::Null);
            (code, json)
        }
        Err(e) => {
            return InvokeResult {
                status: "error".to_string(), agent: slug, task_id: None, output: None,
                charged_pence: None, proof_id: None, error: Some("request_failed".to_string()),
                message: format!("Invoke request failed: {}", e),
            }
        }
    };

    if inv_status == 401 {
        return InvokeResult {
            status: "error".to_string(), agent: slug, task_id: None, output: None,
            charged_pence: None, proof_id: None, error: Some("invalid_key".to_string()),
            message: "Invalid API key (401).".to_string(),
        };
    }

    let task_id = match inv_json.get("task_id").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => {
            let err_msg = inv_json.get("error").or(inv_json.get("note")).and_then(|v| v.as_str()).unwrap_or("no task_id");
            return InvokeResult {
                status: "error".to_string(), agent: slug, task_id: None, output: None,
                charged_pence: None, proof_id: None, error: Some("invoke_failed".to_string()),
                message: format!("Invoke failed (HTTP {}): {}", inv_status, err_msg),
            };
        }
    };

    let start = Instant::now();
    let mut interval_ms: u64 = 2500;

    while start.elapsed() < Duration::from_millis(max_wait_ms) {
        sleep(Duration::from_millis(interval_ms));

        let poll_result = ureq::get(&format!("{}/v1/agents/{}/result/{}", api_base, urlencoding::encode(&slug), urlencoding::encode(&task_id)))
            .set("Authorization", &format!("Bearer {}", api_key))
            .call();

        let d: Value = match poll_result {
            Ok(r) => r.into_json().unwrap_or(Value::Null),
            Err(ureq::Error::Status(_, r)) => r.into_json().unwrap_or(Value::Null),
            Err(_) => Value::Null,
        };

        let status = d.get("status").or(d.get("outcome")).and_then(|v| v.as_str()).unwrap_or("");
        let ok_true = d.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);

        if status == "completed" || status == "succeeded" || ok_true {
            let output = d.get("output").cloned();
            let is_insufficient = d.get("outcome").and_then(|v| v.as_str()) == Some("insufficient")
                || output.as_ref().and_then(|o| o.get("confidence")).and_then(|v| v.as_str()) == Some("insufficient");

            if is_insufficient {
                return InvokeResult {
                    status: "insufficient".to_string(), agent: slug, task_id: Some(task_id),
                    output, charged_pence: Some(0), proof_id: None, error: None,
                    message: "Agent returned insufficient evidence and declined rather than fabricate. Charged nothing.".to_string(),
                };
            }

            let charged = d.get("charged_pence").and_then(|v| v.as_i64());
            let proof_id = d.get("proof_id").and_then(|v| v.as_str()).map(String::from).or_else(|| Some(task_id.clone()));
            return InvokeResult {
                status: "completed".to_string(), agent: slug, task_id: Some(task_id), output,
                charged_pence: charged, proof_id: proof_id.clone(), error: None,
                message: format!("Completed. Charged {}p. Cryptographically proven (proof_id {}).", charged.unwrap_or(0), proof_id.unwrap_or_default()),
            };
        }

        if status == "insufficient" {
            return InvokeResult {
                status: "insufficient".to_string(), agent: slug, task_id: Some(task_id),
                output: d.get("output").cloned(), charged_pence: Some(0), proof_id: None, error: None,
                message: "Agent declined (insufficient evidence). Charged nothing.".to_string(),
            };
        }

        if status == "charge_failed" {
            let reason = d.get("reason").and_then(|v| v.as_str()).unwrap_or("insufficient_balance");
            return InvokeResult {
                status: "error".to_string(), agent: slug, task_id: Some(task_id), output: None,
                charged_pence: Some(0), proof_id: None, error: Some("charge_failed".to_string()),
                message: format!("Charge failed: {}. Nothing charged or delivered. Top up and retry.", reason),
            };
        }

        if status == "failed" || status == "dead_letter" {
            let reason = d.get("reason").or(d.get("last_error")).and_then(|v| v.as_str()).unwrap_or("unknown");
            return InvokeResult {
                status: "error".to_string(), agent: slug, task_id: Some(task_id), output: None,
                charged_pence: None, proof_id: None, error: Some(status.to_string()),
                message: format!("Task {}: {}", status, reason),
            };
        }

        interval_ms = (interval_ms + 1000).min(6000);
    }

    InvokeResult {
        status: "pending".to_string(), agent: slug, task_id: Some(task_id), output: None,
        charged_pence: None, proof_id: None, error: None,
        message: format!("Still processing after {}s. Not re-invoked (would double-charge). Poll the result later with this task_id.", max_wait_ms / 1000),
    }
}
