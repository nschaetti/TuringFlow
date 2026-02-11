use std::error::Error;

use turingflow::rchain::chat_models::ChatFireworks;
use turingflow::rchain::human::HumanMessage;

#[test]
fn flow_matching_text_response() -> Result<(), Box<dyn Error>> {
    let llm = ChatFireworks::new("accounts/fireworks/models/qwen3-vl-235b-a22b-instruct", 0.7)?;

    let messages = vec![HumanMessage::new("Explain what Flow Matching is, briefly.")];

    let response = llm.invoke(&messages)?;
    println!("{}", response.content);

    Ok(())
}
