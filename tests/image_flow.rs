use std::error::Error;

use serde_json::json;
use turingflow::rchain::chat_models::ChatFireworks;
use turingflow::rchain::human::HumanMessage;
use turingflow::rchain::tools::encode_image_base64;

#[test]
fn image_description_response() -> Result<(), Box<dyn Error>> {
    let image_b64 = encode_image_base64("../examples/example.png")?;
    let llm = ChatFireworks::new(
        "accounts/fireworks/models/qwen3-vl-235b-a22b-instruct",
        0.2,
    )?;

    let message = HumanMessage::from_parts(vec![
        json!({
            "type": "text",
            "text": "Describe this image and identify any anomalies.",
        }),
        json!({
            "type": "image_url",
            "image_url": {
                "url": format!("data:image/png;base64,{}", image_b64),
            },
        }),
    ]);

    let response = llm.invoke(&[message])?;
    println!("{}", response.content);

    Ok(())
}
