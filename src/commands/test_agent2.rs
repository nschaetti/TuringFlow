use std::error::Error;
use std::sync::Arc;

use serde_json::{json, Value};

use crate::commands::runtime::ToolRuntime;
use turingflow::rchain::agents::{create_agent, AgentStreamMessage, AgentTool};
use turingflow::rchain::chat_models::ChatFireworks;
use turingflow::rchain::human::HumanMessage;
use turingflow::rchain::tools::{
    encode_image_base64_from_bytes, ToolDefinition, ToolFunction, ToolParam, ToolParamType,
};

/// Executes the Rust equivalent of `python/example_langchain_agent2_fireworks.py`.
pub fn run_test_agent2(
    runtime: &ToolRuntime,
    model: impl Into<String>,
    vision_model: impl Into<String>,
    temperature: f64,
    vision_temperature: f64,
    images_dir: impl Into<String>,
    report_path: impl Into<String>,
    recursion_limit: usize,
) -> Result<(), Box<dyn Error>> {
    let model = model.into();
    let vision_model = vision_model.into();
    let images_dir = images_dir.into();
    let report_path = report_path.into();

    let llm = ChatFireworks::new(model, temperature)?;
    let vision_llm = ChatFireworks::new(vision_model, vision_temperature)?;

    run_test_agent2_with_clients(
        runtime,
        llm,
        vision_llm,
        images_dir,
        report_path,
        recursion_limit,
    )
}

/// Executes the agent2 scenario with pre-configured text and vision chat clients.
pub fn run_test_agent2_with_clients(
    runtime: &ToolRuntime,
    llm: ChatFireworks,
    vision_llm: ChatFireworks,
    images_dir: impl Into<String>,
    report_path: impl Into<String>,
    recursion_limit: usize,
) -> Result<(), Box<dyn Error>> {
    let images_dir = images_dir.into();
    let report_path = report_path.into();

    let list_runtime = runtime.clone();
    let list_directory = AgentTool::new(
        ToolDefinition::from_function(
            ToolFunction::new("list_directory", "List files in a directory.").with_param(
                ToolParam::new(
                    "path",
                    ToolParamType::String,
                    true,
                    Some("Directory path to list.".to_string()),
                ),
            ),
        ),
        Arc::new(move |args: &Value| {
            let path = required_string(args, "path")?;
            let entries = list_runtime
                .list_directory(path, Some("list_directory"))
                .map_err(|error| error.to_string())?;
            serde_json::to_string(&entries).map_err(|error| error.to_string())
        }),
    );

    let read_runtime = runtime.clone();
    let read_file = AgentTool::new(
        ToolDefinition::from_function(
            ToolFunction::new("read_file", "Read a text file.").with_param(ToolParam::new(
                "path",
                ToolParamType::String,
                true,
                Some("File path to read.".to_string()),
            )),
        ),
        Arc::new(move |args: &Value| {
            let path = required_string(args, "path")?;
            let bytes = read_runtime
                .read_bytes(path, Some("read_file"))
                .map_err(|error| error.to_string())?;
            String::from_utf8(bytes).map_err(|_| "File is not valid UTF-8".to_string())
        }),
    );

    let append_runtime = runtime.clone();
    let append_file = AgentTool::new(
        ToolDefinition::from_function(
            ToolFunction::new("append_file", "Append text to a file.")
                .with_param(ToolParam::new(
                    "path",
                    ToolParamType::String,
                    true,
                    Some("File path to append to.".to_string()),
                ))
                .with_param(ToolParam::new(
                    "content",
                    ToolParamType::String,
                    true,
                    Some("Text content to append.".to_string()),
                )),
        ),
        Arc::new(move |args: &Value| {
            let path = required_string(args, "path")?;
            let content = required_string(args, "content")?;

            let mut existing = match append_runtime.read_bytes(&path, Some("append_file")) {
                Ok(bytes) => {
                    String::from_utf8(bytes).map_err(|_| "File is not valid UTF-8".to_string())?
                }
                Err(error) => {
                    let message = error.to_string();
                    if message.contains("ENOENT") {
                        String::new()
                    } else {
                        return Err(message);
                    }
                }
            };

            existing.push_str(&content);
            existing.push('\n');

            append_runtime
                .write_bytes(path, existing.into_bytes(), Some("append_file"))
                .map_err(|error| error.to_string())?;
            Ok("OK".to_string())
        }),
    );

    let inspect_runtime = runtime.clone();
    let inspect_vision = vision_llm.clone();
    let inspect_image = AgentTool::new(
        ToolDefinition::from_function(
            ToolFunction::new(
                "inspect_image",
                "Look at an image and analyze content with a multimodal model.",
            )
            .with_param(ToolParam::new(
                "path",
                ToolParamType::String,
                true,
                Some("Image path to inspect.".to_string()),
            ))
            .with_param(ToolParam::new(
                "focus",
                ToolParamType::String,
                false,
                Some("Optional analysis focus.".to_string()),
            )),
        ),
        Arc::new(move |args: &Value| {
            let path = required_string(args, "path")?;
            let focus = optional_string(args, "focus")
                .unwrap_or_else(|| "describe content and key information".to_string());

            let image_bytes = inspect_runtime
                .read_bytes(&path, Some("inspect_image"))
                .map_err(|error| error.to_string())?;
            let image_b64 =
                encode_image_base64_from_bytes(&image_bytes).map_err(|error| error.to_string())?;

            let message = HumanMessage::from_parts(vec![
                json!({
                    "type": "text",
                    "text": format!("Analyze this image: {}", focus),
                }),
                json!({
                    "type": "image_url",
                    "image_url": {
                        "url": format!("data:image/png;base64,{}", image_b64),
                    },
                }),
            ]);

            let response = inspect_vision
                .invoke(&[message])
                .map_err(|error| error.to_string())?;
            Ok(response.content)
        }),
    );

    let agent = create_agent(
        llm,
        vec![list_directory, read_file, append_file, inspect_image],
        Some(
            "You are an autonomous agent. \
Always start with a numbered PLAN section. \
Then execute with an EXECUTION section step by step. \
Use available tools when needed. \
End with a final synthesis."
                .to_string(),
        ),
    );

    let objective = format!(
        "Analyze all images in the '{}' folder.\n\
Look at actual image content and extract key information.\n\
Create a '{}' file.\n\
Append one description per image.\n\
End with a global summary.",
        images_dir, report_path
    );

    println!("\n========== AGENT DEBUG MODE ==========");

    let _final_response = agent.stream_updates(objective, recursion_limit, |update| {
        for message in update.messages {
            match message {
                AgentStreamMessage::Assistant(ai) => {
                    println!("\n-----------------------------------");
                    println!("ROLE: ai");
                    if !ai.content.is_empty() {
                        println!("CONTENT:");
                        println!("{}", ai.content);
                    }
                    if !ai.tool_calls.is_empty() {
                        println!("\nTOOL CALLS:");
                        for tool_call in &ai.tool_calls {
                            println!("-> Tool: {}", tool_call.name);
                            println!("   Args: {}", tool_call.args);
                        }
                    }
                }
                AgentStreamMessage::Tool { tool_call, content } => {
                    println!("\n-----------------------------------");
                    println!("ROLE: tool");
                    println!("CONTENT:");
                    println!("{}", content);
                    println!("\nTOOL RESULT:");
                    println!("{}", content);
                    println!("TOOL_CALL_ID: {}", tool_call.id);
                }
            }
        }
    })?;

    println!("\n========== FIN ==========");
    Ok(())
}

fn required_string(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| format!("Missing required string argument '{}'", key))
}

fn optional_string(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}
