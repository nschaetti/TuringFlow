use std::env;
use std::error::Error;

use reqwest::blocking::Client;
use serde_json::json;

#[derive(Debug, Clone)]
pub struct FireworksEmbeddings {
    model: String,
    api_key: String,
    base_url: String,
    client: Client,
}

impl FireworksEmbeddings {
    pub fn new(model: impl Into<String>) -> Result<Self, Box<dyn Error>> {
        let api_key = env::var("FIREWORKS_API_KEY")
            .map_err(|_| "FIREWORKS_API_KEY is not set in the environment")?;
        Ok(Self {
            model: model.into(),
            api_key,
            base_url: "https://api.fireworks.ai/inference/v1/embeddings".to_string(),
            client: Client::new(),
        })
    }

    pub fn embed_query(&self, input: impl Into<String>) -> Result<Vec<f64>, Box<dyn Error>> {
        let payload = json!({
            "model": self.model,
            "input": input.into(),
        });

        let response = self
            .client
            .post(&self.base_url)
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(format!("Fireworks API error {status}: {body}").into());
        }

        let body: serde_json::Value = response.json()?;
        let embedding = body["data"][0]["embedding"]
            .as_array()
            .ok_or("Missing embedding data from Fireworks API")?;
        let mut vector = Vec::with_capacity(embedding.len());
        for value in embedding {
            let number = value
                .as_f64()
                .ok_or("Embedding contains a non-float value")?;
            vector.push(number);
        }

        Ok(vector)
    }
}
