mod cli;
mod commands;
mod config;

use std::error::Error;

use clap::Parser;

use crate::cli::{Cli, Commands};

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Image {
            image_path,
            prompt,
            config_path,
            format,
            output,
        } => {
            commands::image::run_image(image_path, prompt, config_path, format, output)?;
        }
        Commands::Embeddings { text_path, model } => {
            commands::embeddings::run_embeddings(text_path, model)?;
        }
        Commands::Calc {
            prompt,
            model,
            temperature,
        } => {
            commands::calc::run_calc(prompt, model, temperature)?;
        }
    }

    Ok(())
}
