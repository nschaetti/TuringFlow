from langchain_fireworks import ChatFireworks
from langchain_core.tools import tool
from langchain_core.messages import HumanMessage

# 1. Définir un tool simple
@tool
def multiply(a: int, b: int) -> int:
    """Multiply two integers."""
    return a * b


# 2. Initialiser le modèle
llm = ChatFireworks(
    model="accounts/fireworks/models/minimax-m2p1",
    temperature=0.0,
)

# 3. Lier les tools au modèle
llm_with_tools = llm.bind_tools([multiply])

# 4. Message utilisateur
messages = [
    HumanMessage(content="What is 6 times 7?")
]

# 5. Appel du modèle
response = llm_with_tools.invoke(messages)

# 6. Vérifier si le modèle demande un tool
if response.tool_calls:
    for tool_call in response.tool_calls:
        if tool_call["name"] == "multiply":
            args = tool_call["args"]
            result = multiply.invoke(args)

            print("Tool result:", result)

            # 7. Réinjecter le résultat au modèle (optionnel ici)
            final = llm.invoke([
                messages[0],
                response,
                {
                    "role": "tool",
                    "tool_call_id": tool_call["id"],
                    "content": str(result),
                }
            ])

            print("Final answer:", final.content)
else:
    print("Model answer:", response.content)
