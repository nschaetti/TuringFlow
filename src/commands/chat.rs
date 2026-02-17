use std::error::Error;

use crate::commands::runtime::ToolRuntime;

/// Handles the `turingflow chat` command.
///
/// Queues a user-originated message in the user-plane inbound queue.
pub fn run_chat(
    runtime: &ToolRuntime,
    message: impl Into<String>,
    channel: impl Into<String>,
    thread_id: Option<String>,
) -> Result<(), Box<dyn Error>> {
    let message = message.into();
    let channel = channel.into();

    let inbound_id = runtime.ingest_user_message(channel.clone(), message, thread_id.clone())?;
    println!(
        "User message queued for agents: id={}, channel={}, thread_id={}",
        inbound_id,
        channel,
        thread_id.unwrap_or_else(|| "-".to_string())
    );

    Ok(())
}
