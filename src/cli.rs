use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Root CLI arguments.
#[derive(Debug, Parser)]
#[command(name = "turingflow", version, about = "TuringFlow CLI")]
pub struct Cli {
    /// Selected subcommand.
    #[command(subcommand)]
    pub command: Commands,
}

/// CLI subcommands.
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Runs image prompt extraction.
    Image {
        #[arg(short = 'i', long = "image")]
        image_path: PathBuf,
        #[arg(short = 'p', long = "prompt")]
        prompt: String,
        #[arg(short = 'c', long = "config")]
        config_path: PathBuf,
        #[arg(short = 'f', long = "format", default_value = "json")]
        format: String,
        #[arg(short = 'o', long = "output")]
        output: Option<PathBuf>,
    },
    /// Runs text embedding generation.
    Embeddings {
        #[arg(short = 't', long = "text")]
        text_path: PathBuf,
        #[arg(
            short = 'm',
            long = "model",
            default_value = "nomic-ai/nomic-embed-text-v1.5"
        )]
        model: String,
    },
    /// Runs tool-calling calculator demo.
    Calc {
        #[arg(short = 'p', long = "prompt", default_value = "What is 6 times 7?")]
        prompt: String,
        #[arg(
            short = 'm',
            long = "model",
            default_value = "accounts/fireworks/models/minimax-m2p1"
        )]
        model: String,
        #[arg(short = 't', long = "temperature", default_value_t = 0.0)]
        temperature: f64,
    },
    /// Queues a user message for agents.
    Chat {
        #[arg(short = 'm', long = "message")]
        message: String,
        #[arg(long = "channel", default_value = "cli")]
        channel: String,
        #[arg(long = "thread-id")]
        thread_id: Option<String>,
    },
    /// Shows outbound messages queued for the user.
    Inbox {
        #[arg(long = "limit", default_value_t = 20)]
        limit: usize,
        #[arg(long = "include-delivered", default_value_t = false)]
        include_delivered: bool,
    },
    /// Dumps inbound/outbound user queues for local debugging.
    DebugUser {
        #[arg(long = "limit", default_value_t = 50)]
        limit: usize,
        #[arg(long = "include-acked", default_value_t = true)]
        include_acked: bool,
        #[arg(long = "include-delivered", default_value_t = true)]
        include_delivered: bool,
    },
}
