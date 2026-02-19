import os
import base64
import mimetypes
from langchain_openai import ChatOpenAI
from langchain.agents import create_agent
from langchain_core.tools import tool
from langchain_core.messages import HumanMessage


# ==============================
# 🔧 TOOLS
# ==============================

@tool
def list_directory(path: str) -> list:
    """Liste les fichiers d'un répertoire."""
    return os.listdir(path)


@tool
def read_file(path: str) -> str:
    """Lit un fichier texte."""
    with open(path, "r", encoding="utf-8") as f:
        return f.read()


@tool
def append_file(path: str, content: str) -> str:
    """Ajoute du texte à un fichier."""
    with open(path, "a", encoding="utf-8") as f:
        f.write(content + "\n")
    return "OK"


def _to_data_url(path: str) -> str:
    mime_type, _ = mimetypes.guess_type(path)
    if not mime_type:
        mime_type = "image/png"
    with open(path, "rb") as f:
        image_b64 = base64.b64encode(f.read()).decode("utf-8")
    return f"data:{mime_type};base64,{image_b64}"


@tool
def inspect_image(path: str, focus: str = "decris le contenu et les infos importantes") -> str:
    """Regarde une image et analyse son contenu (objets, texte visible, scene)."""
    data_url = _to_data_url(path)
    response = llm.invoke(
        [
            HumanMessage(
                content=[
                    {"type": "text", "text": f"Analyse cette image: {focus}"},
                    {"type": "image_url", "image_url": {"url": data_url}},
                ]
            )
        ]
    )
    if isinstance(response.content, str):
        return response.content
    return str(response.content)


tools = [list_directory, read_file, append_file, inspect_image]


# ==============================
# 🧠 LLM
# ==============================

llm = ChatOpenAI(
    model="gpt-4o-mini",
    temperature=0
)


# ==============================
# 🤖 AGENT
# ==============================

agent = create_agent(
    model=llm,
    tools=tools,
    system_prompt=(
        "Tu es un agent autonome.\n"
        "1. Commence toujours par une section 'PLAN' explicite et numerotee.\n"
        "2. Ensuite execute chaque etape dans une section 'EXECUTION'.\n"
        "3. Utilise les outils quand nécessaire.\n"
        "4. Explique ce que tu fais a chaque etape avant et apres les appels outils.\n"
        "5. Termine par une synthèse finale.\n"
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
# 🚀 DEBUG EXECUTION
# ==============================

if __name__ == "__main__":
    objective = """
Analyse toutes les images du dossier 'agent_tests'.
Regarde ce qu'elles contiennent, leurs contenus et fais un 
résumé de chacune. Extrait les informations importantes.
Crée un fichier 'report.txt'.
Ajoute les informations importantes pour chaque image.
Termine par un résumé global.
"""

    print("\n========== AGENT DEBUG MODE ==========\n")

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

            # Contenu texte
            if message.content:
                print("CONTENT:")
                print(message.content)

            # Tool calls
            if hasattr(message, "tool_calls") and message.tool_calls:
                print("\nTOOL CALLS:")
                for tc in message.tool_calls:
                    print(f"→ Tool: {tc['name']}")
                    print(f"  Args: {tc['args']}")

            # Tool response
            if role == "tool":
                print("\nTOOL RESULT:")
                print(message.content)

    print("\n========== FIN ==========\n")
