import os
import base64
import mimetypes
from langchain.agents import create_agent
from langchain_core.tools import tool
from langchain_core.messages import HumanMessage
from langchain_fireworks import ChatFireworks


# ==============================
# TOOLS
# ==============================

@tool
def list_directory(path: str) -> list:
    """List files in a directory."""
    return os.listdir(path)


@tool
def read_file(path: str) -> str:
    """Read a text file."""
    with open(path, "r", encoding="utf-8") as f:
        return f.read()


@tool
def append_file(path: str, content: str) -> str:
    """Append text to a file."""
    with open(path, "a", encoding="utf-8") as f:
        f.write(content + "\n")
    return "OK"


@tool
def inspect_image(path: str, focus: str = "describe content and key information") -> str:
    """Look at an image and analyze content with a multimodal model."""
    data_url = _to_data_url(path)
    response = vision_llm.invoke(
        [
            HumanMessage(
                content=[
                    {"type": "text", "text": f"Analyze this image: {focus}"},
                    {"type": "image_url", "image_url": {"url": data_url}},
                ]
            )
        ]
    )
    if isinstance(response.content, str):
        return response.content
    return str(response.content)


tools = [list_directory, read_file, append_file, inspect_image]


def _to_data_url(path: str) -> str:
    mime_type, _ = mimetypes.guess_type(path)
    if not mime_type:
        mime_type = "image/png"
    with open(path, "rb") as f:
        image_b64 = base64.b64encode(f.read()).decode("utf-8")
    return f"data:{mime_type};base64,{image_b64}"


# ==============================
# LLM
# ==============================

llm = ChatFireworks(
    model="accounts/fireworks/models/minimax-m2p1",
    temperature=0.0,
)

vision_llm = ChatFireworks(
    model="accounts/fireworks/models/kimi-k2p5",
    temperature=0.2,
)


# ==============================
# AGENT
# ==============================

agent = create_agent(
    model=llm,
    tools=tools,
    system_prompt=(
        "You are an autonomous agent. "
        "Always start with a numbered PLAN section. "
        "Then execute with an EXECUTION section step by step. "
        "Use available tools when needed. "
        "End with a final synthesis."
    ),
)


def _extract_messages(step: dict) -> list:
    messages = []

    direct = step.get("messages")
    if isinstance(direct, list):
        messages.extend(direct)

    for value in step.values():
        if isinstance(value, dict):
            nested = value.get("messages")
            if isinstance(nested, list):
                messages.extend(nested)

    return messages


def _message_key(message) -> str:
    message_id = getattr(message, "id", None)
    if message_id:
        return str(message_id)
    return "|".join(
        [
            str(getattr(message, "type", "")),
            repr(getattr(message, "content", "")),
            repr(getattr(message, "tool_calls", None)),
            repr(getattr(message, "tool_call_id", None)),
        ]
    )


# ==============================
# DEBUG EXECUTION
# ==============================

if __name__ == "__main__":
    objective = """
Analyze all images in the 'agent_tests' folder.
Look at actual image content and extract key information.
Create a 'report.txt' file.
Append one description per image.
End with a global summary.
"""

    print("\n========== AGENT DEBUG MODE ==========")

    stream = agent.stream(
        {"messages": [{"role": "user", "content": objective}]},
        config={"recursion_limit": 20},
        stream_mode="updates",
    )

    seen_messages = set()

    for step in stream:
        messages = _extract_messages(step)
        if not messages:
            continue

        for message in messages:
            key = _message_key(message)
            if key in seen_messages:
                continue
            seen_messages.add(key)

            role = getattr(message, "type", None)

            print("\n-----------------------------------")
            print(f"ROLE: {role}")

            if message.content:
                print("CONTENT:")
                print(message.content)

            if hasattr(message, "tool_calls") and message.tool_calls:
                print("\nTOOL CALLS:")
                for tc in message.tool_calls:
                    print(f"-> Tool: {tc['name']}")
                    print(f"   Args: {tc['args']}")

            if role == "tool":
                print("\nTOOL RESULT:")
                print(message.content)

    print("\n========== FIN ==========")
