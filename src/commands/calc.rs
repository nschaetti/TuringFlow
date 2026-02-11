use std::error::Error;

use turingflow::rchain::chat_models::{ChatFireworks, ChatMessage};
use turingflow::rchain::tools::{ToolDefinition, ToolFunction, ToolParam, ToolParamType};

fn multiply(a: i64, b: i64) -> i64 {
    a * b
}

pub fn run_calc(
    prompt: impl Into<String>,
    model: impl Into<String>,
    temperature: f64,
) -> Result<(), Box<dyn Error>> {
    let tool = ToolDefinition::from_function(
        ToolFunction::new("multiply", "Multiply two integers.")
            .with_param(ToolParam::new(
                "a",
                ToolParamType::Integer,
                true,
                Some("First factor.".to_string()),
            ))
            .with_param(ToolParam::new(
                "b",
                ToolParamType::Integer,
                true,
                Some("Second factor.".to_string()),
            )),
    );

    let llm = ChatFireworks::new(model, temperature)?.bind_tools(vec![tool]);
    let user_message = ChatMessage::user_text(prompt.into());
    let response = llm.invoke_messages(&[user_message.clone()])?;

    if response.tool_calls.is_empty() {
        println!("Model answer: {}", response.content);
        return Ok(());
    }

    for tool_call in &response.tool_calls {
        if tool_call.name != "multiply" {
            continue;
        }
        let a = tool_call
            .args
            .get("a")
            .and_then(|value| value.as_i64())
            .ok_or("Missing integer argument 'a' for multiply")?;
        let b = tool_call
            .args
            .get("b")
            .and_then(|value| value.as_i64())
            .ok_or("Missing integer argument 'b' for multiply")?;
        let result = multiply(a, b);

        println!("Tool result: {}", result);

        let final_response = llm.invoke_messages(&[
            user_message.clone(),
            ChatMessage::assistant_from_ai(&response),
            ChatMessage::tool_result(tool_call.id.clone(), result.to_string()),
        ])?;

        println!("Final answer: {}", final_response.content);
    }

    Ok(())
}
