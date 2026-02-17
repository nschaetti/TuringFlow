use std::error::Error;
use std::path::Path;

use turingflow::rchain::embeddings::FireworksEmbeddings;

use crate::commands::runtime::ToolRuntime;

/// Executes the embeddings command.
///
/// Loads UTF-8 text through the kernel runtime and prints basic embedding stats.
pub fn run_embeddings(
    runtime: &ToolRuntime,
    text_path: impl AsRef<Path>,
    model: impl Into<String>,
) -> Result<(), Box<dyn Error>> {
    let text_bytes = runtime.read_bytes(text_path, Some("embeddings"))?;
    let text = String::from_utf8(text_bytes).map_err(|_| "Text input must be valid UTF-8")?;
    let embeddings = FireworksEmbeddings::new(model)?;
    let vector = embeddings.embed_query(text)?;

    println!("Embedding dimension: {}", vector.len());
    println!("First 10 values:");
    let end = vector.len().min(10);
    println!("{:?}", &vector[..end]);

    Ok(())
}
