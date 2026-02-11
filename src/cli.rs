use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "turingflow", version, about = "TuringFlow CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
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
}
