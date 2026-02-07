import base64
from PIL import Image
from io import BytesIO

from langchain_fireworks import ChatFireworks
from langchain_core.messages import HumanMessage


def encode_image(path: str) -> str:
    """Encode an image in base64 (data URL friendly)."""
    with Image.open(path) as img:
        buffer = BytesIO()
        img.save(buffer, format="PNG")
        return base64.b64encode(buffer.getvalue()).decode("utf-8")


# Encode the image
image_b64 = encode_image("../examples/example.png")

# Fireworks vision model
llm = ChatFireworks(
    model="accounts/fireworks/models/qwen3-vl-235b-a22b-instruct",
    temperature=0.2,
)

# Multimodal message
message = HumanMessage(
    content=[
        {"type": "text", "text": "Describe this image and identify any anomalies."},
        {
            "type": "image_url",
            "image_url": {
                "url": f"data:image/png;base64,{image_b64}"
            },
        },
    ]
)

# Appel
response = llm.invoke([message])

print(response.content)
