//! A real, honestly-scoped client for the ForceDream API. Wraps only endpoints verified
//! working directly against the live, production API -- not the full platform surface.
//! See the README for exactly what is and isn't covered yet.

mod agents;
mod canonical;
mod invoke;
mod verify;

pub use invoke::InvokeResult;
pub use verify::{VerifyError, VerifyResult};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub struct ForceDream {
    api_key: Option<String>,
    api_base: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SignupResponse {
    pub api_key: String,
    pub user_id: String,
    pub live_key: String,
    pub trial_balance_pence: i64,
    pub trial_balance_gbp: String,
    pub referral_code: Option<String>,
}

#[derive(thiserror::Error, Debug)]
pub enum ForceDreamError {
    #[error("HTTP request failed: {0}")]
    Http(String),
    #[error("response body error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0} requires an apiKey")]
    MissingApiKey(&'static str),
    #[error(transparent)]
    Verify(#[from] verify::VerifyError),
}

impl From<ureq::Error> for ForceDreamError {
    fn from(e: ureq::Error) -> Self {
        ForceDreamError::Http(e.to_string())
    }
}

impl ForceDream {
    pub fn new(api_key: Option<String>) -> Self {
        Self {
            api_key,
            api_base: "https://api.forcedream.ai".to_string(),
        }
    }

    pub fn with_api_base(mut self, api_base: impl Into<String>) -> Self {
        self.api_base = api_base.into();
        self
    }

    /// Create a new ForceDream account. No API key needed -- this is how you get one.
    /// Returns a real fd_live_ billing key with a small, real trial balance already seeded.
    pub fn signup(email: &str) -> Result<SignupResponse, ForceDreamError> {
        Self::signup_at("https://api.forcedream.ai", email)
    }

    pub fn signup_at(api_base: &str, email: &str) -> Result<SignupResponse, ForceDreamError> {
        let res: SignupResponse = ureq::post(&format!("{}/api/signup", api_base))
            .send_json(serde_json::json!({ "email": email }))?
            .into_json()?;
        Ok(res)
    }

    /// Real, current account balance. Requires an API key.
    pub fn get_balance(&self) -> Result<Value, ForceDreamError> {
        let key = self.api_key.as_ref().ok_or(ForceDreamError::MissingApiKey("get_balance()"))?;
        let res: Value = ureq::get(&format!("{}/v1/account/balance", self.api_base))
            .set("Authorization", &format!("Bearer {}", key))
            .call()?
            .into_json()?;
        Ok(res)
    }

    /// Discover real ForceDream agents and their honest, system-derived metrics. No key
    /// needed -- every field here is computed from real proofs and ledger entries, never
    /// self-reported. Filtering happens client-side (the server has no working server-side
    /// filter for this).
    pub fn search_agents(&self, capability: Option<&str>, query: Option<&str>) -> Result<Value, ForceDreamError> {
        Ok(agents::search_agents_filtered(&self.api_base, capability, query)?)
    }

    /// Invoke a real ForceDream agent to do real work. Spends your balance -- requires an API
    /// key. Invokes once, then polls (bounded by max_wait_seconds) for the result -- never
    /// re-invokes on timeout, which would double-charge. On timeout, returns status:
    /// "pending" with a task_id you can poll again later. Honest declines and failed charges
    /// cost nothing.
    pub fn invoke(&self, agent_slug: &str, task: &str, max_wait_seconds: Option<u64>) -> Result<InvokeResult, ForceDreamError> {
        let key = self.api_key.as_ref().ok_or(ForceDreamError::MissingApiKey("invoke()"))?;
        Ok(invoke::invoke_agent_polling(&self.api_base, key, agent_slug, task, max_wait_seconds))
    }

    /// Trustlessly verify a proof's Ed25519 signature, entirely client-side. ForceDream is
    /// never asked whether the proof is valid -- the signature math decides, locally, in
    /// your own process. No API key needed.
    pub fn verify_by_task_id(&self, task_id: &str) -> Result<VerifyResult, ForceDreamError> {
        Ok(verify::verify_proof(&self.api_base, Some(task_id), None)?)
    }

    pub fn verify_proof(&self, proof: Value) -> Result<VerifyResult, ForceDreamError> {
        Ok(verify::verify_proof(&self.api_base, None, Some(proof))?)
    }
}
