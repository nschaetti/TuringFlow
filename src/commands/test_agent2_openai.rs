use std::error::Error;

use crate::commands::runtime::ToolRuntime;
use crate::commands::test_agent2::run_test_agent2_with_clients;
use turingflow::rchain::chat_models::ChatFireworks;

/// Executes the agent2 scenario with OpenAI-compatible chat endpoints.
///
/// Required env vars:
/// - `OPENAI_API_KEY`
///
/// Optional env vars:
/// - `OPENAI_BASE_URL` (defaults to OpenAI public endpoint)
pub fn run_test_agent2_openai(
    runtime: &ToolRuntime,
    model: impl Into<String>,
    vision_model: impl Into<String>,
    temperature: f64,
    vision_temperature: f64,
    images_dir: impl Into<String>,
    report_path: impl Into<String>,
    recursion_limit: usize,
) -> Result<(), Box<dyn Error>> {
    let llm = ChatFireworks::new_openai_compatible(model, temperature)?;
    let vision_llm = ChatFireworks::new_openai_compatible(vision_model, vision_temperature)?;

    run_test_agent2_with_clients(
        runtime,
        llm,
        vision_llm,
        images_dir,
        report_path,
        recursion_limit,
    )
}
