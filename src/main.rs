mod cli;
mod commands;
mod config;

use std::error::Error;

use clap::Parser;

use crate::cli::{Cli, Commands};
use crate::commands::runtime::ToolRuntime;

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let runtime = ToolRuntime::new()?;

    match cli.command {
        Commands::Image {
            image_path,
            prompt,
            config_path,
            format,
            output,
        } => {
            commands::image::run_image(&runtime, image_path, prompt, config_path, format, output)?;
        }
        Commands::Embeddings { text_path, model } => {
            commands::embeddings::run_embeddings(&runtime, text_path, model)?;
        }
        Commands::Calc {
            prompt,
            model,
            temperature,
        } => {
            commands::calc::run_calc(prompt, model, temperature)?;
        }
        Commands::Chat {
            message,
            channel,
            thread_id,
        } => {
            commands::chat::run_chat(&runtime, message, channel, thread_id)?;
        }
        Commands::Inbox {
            limit,
            include_delivered,
        } => {
            commands::inbox::run_inbox(&runtime, limit, include_delivered)?;
        }
        Commands::DebugUser {
            limit,
            include_acked,
            include_delivered,
        } => {
            commands::debug_user::run_debug_user(
                &runtime,
                limit,
                include_acked,
                include_delivered,
            )?;
        }
        Commands::TestAgent2 {
            model,
            vision_model,
            temperature,
            vision_temperature,
            images_dir,
            report_path,
            recursion_limit,
        } => {
            commands::test_agent2::run_test_agent2(
                &runtime,
                model,
                vision_model,
                temperature,
                vision_temperature,
                images_dir,
                report_path,
                recursion_limit,
            )?;
        }
        Commands::TestAgent2Openai {
            model,
            vision_model,
            temperature,
            vision_temperature,
            images_dir,
            report_path,
            recursion_limit,
        } => {
            commands::test_agent2_openai::run_test_agent2_openai(
                &runtime,
                model,
                vision_model,
                temperature,
                vision_temperature,
                images_dir,
                report_path,
                recursion_limit,
            )?;
        }
    }

    Ok(())
}
