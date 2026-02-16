use std::error::Error;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::commands::runtime::ToolRuntime;
use crate::config::load_config_from_str;
use turingflow::rchain::chat_models::ChatFireworks;
use turingflow::rchain::human::HumanMessage;
use turingflow::rchain::tools::encode_image_base64_from_bytes;

pub fn run_image(
    runtime: &ToolRuntime,
    image_path: impl AsRef<Path>,
    prompt: impl Into<String>,
    config_path: impl AsRef<Path>,
    format: String,
    output: Option<PathBuf>,
) -> Result<(), Box<dyn Error>> {
    if format != "json" {
        return Err("Only json output format is supported for now".into());
    }

    let config_raw = runtime.read_bytes(config_path, Some("image"))?;
    let config_str = String::from_utf8(config_raw).map_err(|_| "Config file must be UTF-8")?;
    let config = load_config_from_str(&config_str)?;
    let model = config.llm.model;
    let temperature = config.llm.temperature.unwrap_or(0.2);

    let image_bytes = runtime.read_bytes(image_path, Some("image"))?;
    let image_b64 = encode_image_base64_from_bytes(&image_bytes)?;
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
    let parsed: serde_json::Value =
        serde_json::from_str(&response.content).map_err(|_| "LLM response is not valid JSON")?;
    let pretty = serde_json::to_string_pretty(&parsed)?;
    println!("{}", pretty);

    if let Some(path) = output {
        runtime.write_bytes(path, pretty.into_bytes(), Some("image"))?;
    }

    Ok(())
}
