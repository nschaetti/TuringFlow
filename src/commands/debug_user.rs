use std::error::Error;

use crate::commands::runtime::ToolRuntime;

/// Handles the `turingflow debug-user` command.
///
/// It reads inbound and outbound user-plane queue rows directly from SQLite to
/// simplify operational debugging.
pub fn run_debug_user(
    runtime: &ToolRuntime,
    limit: usize,
    include_acked: bool,
    include_delivered: bool,
) -> Result<(), Box<dyn Error>> {
    let (inbound, outbound) = runtime.debug_user_queues(limit, include_acked, include_delivered)?;

    println!(
        "Inbound queue (limit={}, include_acked={}): {} row(s)",
        limit,
        include_acked,
        inbound.len()
    );
    for row in inbound {
        println!(
            "- [{}] id={} channel={} thread_id={} received_at_ms={} acked={}",
            if row.acknowledged { "acked" } else { "pending" },
            row.message_id,
            row.channel,
            row.thread_id.as_deref().unwrap_or("-"),
            row.received_at_ms,
            row.acknowledged
        );
        println!("  body={}", row.body);
    }

    println!();
    println!(
        "Outbound queue (limit={}, include_delivered={}): {} row(s)",
        limit,
        include_delivered,
        outbound.len()
    );
    for row in outbound {
        println!(
            "- [{}] id={} channel={} thread_id={} created_at_ms={} updated_at_ms={}",
            row.status,
            row.message_id,
            row.channel,
            row.thread_id.as_deref().unwrap_or("-"),
            row.created_at_ms,
            row.updated_at_ms
        );
        println!("  body={}", row.body);
    }

    Ok(())
}
