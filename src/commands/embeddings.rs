use std::error::Error;
use std::fs;
use std::path::Path;

use turingflow::rchain::embeddings::FireworksEmbeddings;

pub fn run_embeddings(
    text_path: impl AsRef<Path>,
    model: impl Into<String>,
) -> Result<(), Box<dyn Error>> {
    let text = fs::read_to_string(text_path)?;
    let embeddings = FireworksEmbeddings::new(model)?;
    let vector = embeddings.embed_query(text)?;

    println!("Embedding dimension: {}", vector.len());
    println!("First 10 values:");
    let end = vector.len().min(10);
    println!("{:?}", &vector[..end]);

    Ok(())
}
