use std::error::Error;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::config::load_config;
use turingflow::rchain::chat_models::ChatFireworks;
use turingflow::rchain::human::HumanMessage;
use turingflow::rchain::tools::encode_image_base64;

pub fn run_image(
    image_path: impl AsRef<Path>,
    prompt: impl Into<String>,
    config_path: impl AsRef<Path>,
    format: String,
    output: Option<PathBuf>,
) -> Result<(), Box<dyn Error>> {
    if format != "json" {
        return Err("Only json output format is supported for now".into());
    }

    let config = load_config(config_path)?;
    let model = config.llm.model;
    let temperature = config.llm.temperature.unwrap_or(0.2);

    let image_b64 = encode_image_base64(image_path)?;
    let llm = ChatFireworks::new(model, temperature)?;
    let prompt = prompt.into();

    let message = HumanMessage::from_parts(vec![
        json!({
            "type": "text",
            "text": prompt,
        }),
        json!({
            "type": "image_url",
            "image_url": {
                "url": format!("data:image/png;base64,{}", image_b64),
            },
        }),
    ]);

    let response = llm.invoke(&[message])?;
    let parsed: serde_json::Value = serde_json::from_str(&response.content)
        .map_err(|_| "LLM response is not valid JSON")?;
    let pretty = serde_json::to_string_pretty(&parsed)?;
    println!("{}", pretty);

    if let Some(path) = output {
        std::fs::write(path, pretty)?;
    }

    Ok(())
}
