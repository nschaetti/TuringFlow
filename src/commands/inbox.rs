use std::error::Error;

use crate::commands::runtime::ToolRuntime;

/// Handles the `turingflow inbox` command.
///
/// Reads outbound user messages from the user-plane queue.
pub fn run_inbox(
    runtime: &ToolRuntime,
    limit: usize,
    include_delivered: bool,
) -> Result<(), Box<dyn Error>> {
    let messages = runtime.list_user_inbox(limit, include_delivered)?;

    if messages.is_empty() {
        println!("No user inbox messages.");
        return Ok(());
    }

    for message in messages {
        println!(
            "[{}] {} {}",
            message.status,
            message.channel,
            message.thread_id.unwrap_or_else(|| "-".to_string())
        );
        println!("id={}", message.message_id);
        println!("{}", message.body);
        println!();
    }

    Ok(())
}
