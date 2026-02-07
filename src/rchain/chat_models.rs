use std::env;
use std::error::Error;

use reqwest::blocking::Client;
use serde_json::json;

use crate::rchain::ai::AIMessage;
use crate::rchain::human::HumanMessage;

#[derive(Debug, Clone)]
pub struct ChatFireworks {
    model: String,
    temperature: f64,
    api_key: String,
    base_url: String,
    client: Client,
}

impl ChatFireworks {
    pub fn new(model: impl Into<String>, temperature: f64) -> Result<Self, Box<dyn Error>> {
        let api_key = env::var("FIREWORKS_API_KEY")
            .map_err(|_| "FIREWORKS_API_KEY is not set in the environment")?;
        Ok(Self {
            model: model.into(),
            temperature,
            api_key,
            base_url: "https://api.fireworks.ai/inference/v1/chat/completions".to_string(),
            client: Client::new(),
        })
    }

    pub fn invoke(&self, messages: &[HumanMessage]) -> Result<AIMessage, Box<dyn Error>> {
        let payload = json!({
            "model": self.model,
            "messages": messages
                .iter()
                .map(|message| json!({
                    "role": "user",
                    "content": message.to_json(),
                }))
                .collect::<Vec<_>>(),
            "temperature": self.temperature,
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
        let content = body["choices"][0]["message"]["content"]
            .as_str()
            .ok_or("Missing response content from Fireworks API")?;

        Ok(AIMessage {
            content: content.to_string(),
        })
    }
}
