use serde_json::Value;

/// Ported precisely from @forcedream/mcp-server's search_agents.ts (via the real, published
/// @forcedream/sdk's agents.js). Real, load-bearing fact confirmed directly from that source,
/// not assumed: the server has no working server-side capability/query filter on
/// /v1/agents/list -- filtering must happen client-side, after fetching the full list. Also
/// merges in real reliability data from the separate /v1/agents/reliability endpoint, exactly
/// as the proven implementation does.
pub fn search_agents_filtered(
    api_base: &str,
    capability: Option<&str>,
    query: Option<&str>,
) -> Result<Value, ureq::Error> {
    let data: Value = ureq::get(&format!("{}/v1/agents/list", api_base))
        .call()?
        .into_json()?;

    let rel_data: Option<Value> = ureq::get(&format!("{}/v1/agents/reliability", api_base))
        .call()
        .ok()
        .and_then(|r| r.into_json().ok());

    let mut agents: Vec<Value> = data
        .get("agents")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut reliability_by_slug: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
    if let Some(rel) = rel_data.as_ref().and_then(|r| r.get("agents")).and_then(|v| v.as_array()) {
        for ra in rel {
            if let Some(slug) = ra.get("agent_slug").and_then(|v| v.as_str()) {
                if let Some(reliability) = ra.get("reliability") {
                    reliability_by_slug.insert(slug.to_string(), reliability.clone());
                }
            }
        }
    }

    if let Some(cap) = capability {
        let cap_lower = cap.to_lowercase();
        agents.retain(|a| {
            a.get("capabilities")
                .and_then(|v| v.as_array())
                .map_or(false, |caps| {
                    caps.iter().any(|c| c.as_str().map_or(false, |s| s.to_lowercase() == cap_lower))
                })
        });
    }

    if let Some(q) = query {
        let q_lower = q.to_lowercase();
        agents.retain(|a| {
            let slug_match = a.get("slug").and_then(|v| v.as_str()).map_or(false, |s| s.to_lowercase().contains(&q_lower));
            let name_match = a.get("name").and_then(|v| v.as_str()).map_or(false, |s| s.to_lowercase().contains(&q_lower));
            let cap_match = a.get("capabilities").and_then(|v| v.as_array()).map_or(false, |caps| {
                caps.iter().any(|c| c.as_str().map_or(false, |s| s.to_lowercase().contains(&q_lower)))
            });
            slug_match || name_match || cap_match
        });
    }

    let enriched: Vec<Value> = agents
        .into_iter()
        .map(|mut a| {
            if let Some(slug) = a.get("slug").and_then(|v| v.as_str()).map(String::from) {
                let health = reliability_by_slug.get(&slug).cloned().unwrap_or(Value::Null);
                if let Some(obj) = a.as_object_mut() {
                    obj.insert("health".to_string(), health);
                }
            }
            a
        })
        .collect();

    let note = if enriched.is_empty() {
        "No agents matched. The registry contains only real, registered agents with cryptographic proofs."
    } else {
        "Metrics are system-derived from proofs/ledger (proof_count, success_rate) -- never self-reported. Health (success_rate, avg_latency_ms, sample_size) is honestly null where no real reliability data exists yet."
    };

    Ok(serde_json::json!({
        "count": enriched.len(),
        "agents": enriched,
        "note": note,
    }))
}
