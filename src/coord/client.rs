use anyhow::{anyhow, Context, Result};
use serde_json::Value;

use crate::protocol::{Job, NodeInfo};

#[derive(Clone)]
pub struct CoordinatorClient {
    base: String,
    http: reqwest::Client,
}

impl CoordinatorClient {
    pub fn new(base: impl Into<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    pub async fn health(&self) -> Result<bool> {
        let res = self.http.get(format!("{}/health", self.base)).send().await?;
        Ok(res.status().is_success())
    }

    pub async fn heartbeat(
        &self,
        node_id: &str,
        class: &str,
        gpu_model: &str,
        max_concurrent: u32,
        cluster_id: &str,
        label: Option<&str>,
    ) -> Result<NodeInfo> {
        let mut body = serde_json::json!({
            "nodeId": node_id,
            "class": class,
            "gpuModel": gpu_model,
            "maxConcurrent": max_concurrent,
            "clusterId": cluster_id,
        });
        if let Some(l) = label {
            body["label"] = serde_json::json!(l);
        }
        let res = self
            .http
            .post(format!("{}/v1/nodes/heartbeat", self.base))
            .json(&body)
            .send()
            .await?;
        if !res.status().is_success() {
            return Err(anyhow!("heartbeat: {}", res.text().await.unwrap_or_default()));
        }
        Ok(res.json().await?)
    }

    pub async fn claim(&self, node_id: &str) -> Result<Option<Job>> {
        self.claim_track(node_id, "both").await
    }

    /// `track`: `host` | `mine` | `both`
    pub async fn claim_track(&self, node_id: &str, track: &str) -> Result<Option<Job>> {
        let body = serde_json::json!({ "nodeId": node_id, "track": track });
        let res = self
            .http
            .post(format!("{}/v1/nodes/claim", self.base))
            .json(&body)
            .send()
            .await?;
        if res.status() == reqwest::StatusCode::NO_CONTENT {
            return Ok(None);
        }
        if !res.status().is_success() {
            return Err(anyhow!("claim: {}", res.text().await.unwrap_or_default()));
        }
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wrap {
            job: Option<Job>,
        }
        let w: Wrap = res.json().await.context("claim json")?;
        Ok(w.job)
    }

    pub async fn complete(
        &self,
        job_id: &str,
        node_id: &str,
        ok: bool,
        output: &str,
        duration_ms: u64,
        operator_pubkey: Option<&str>,
    ) -> Result<(bool, f64)> {
        let mut body = serde_json::json!({
            "jobId": job_id,
            "nodeId": node_id,
            "ok": ok,
            "output": output,
            "durationMs": duration_ms,
        });
        if let Some(pk) = operator_pubkey {
            body["operatorPubkey"] = serde_json::json!(pk);
        }
        let res = self
            .http
            .post(format!("{}/v1/jobs/complete", self.base))
            .json(&body)
            .send()
            .await?;
        if !res.status().is_success() {
            return Err(anyhow!("complete: {}", res.text().await.unwrap_or_default()));
        }
        let v: serde_json::Value = res.json().await?;
        let verified = v.get("verified").and_then(|x| x.as_bool()).unwrap_or(false);
        let earn = v
            .get("earnCredits")
            .and_then(|x| x.as_f64())
            .unwrap_or(0.0);
        Ok((verified, earn))
    }

    pub async fn submit(&self, kind: &str, payload: &str) -> Result<Job> {
        let body = serde_json::json!({ "kind": kind, "payload": payload });
        let res = self
            .http
            .post(format!("{}/v1/jobs", self.base))
            .json(&body)
            .send()
            .await?;
        if !res.status().is_success() {
            return Err(anyhow!("submit: {}", res.text().await.unwrap_or_default()));
        }
        Ok(res.json().await?)
    }

    pub async fn get_job(&self, id: &str) -> Result<Job> {
        let res = self
            .http
            .get(format!("{}/v1/jobs/{id}", self.base))
            .send()
            .await?;
        Ok(res.json().await?)
    }

    pub async fn stats(&self) -> Result<Value> {
        let res = self
            .http
            .get(format!("{}/v1/stats", self.base))
            .send()
            .await?;
        Ok(res.json().await?)
    }
}
