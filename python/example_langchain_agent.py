from langchain_openai import ChatOpenAI
from langchain_core.tools import tool
from langchain.agents import create_agent


# --- 1️⃣ Définition d'un outil ---

@tool
def add(a: int, b: int) -> int:
    """Additionne deux entiers."""
    return a + b


tools = [add]


# --- 2️⃣ LLM (OpenAI compatible API) ---

llm = ChatOpenAI(
    model="gpt-4o-mini",  # ou Fireworks compatible
    temperature=0
)


# --- 3️⃣ Création de l’agent ---

agent = create_agent(
    model=llm,
    tools=tools,
    system_prompt="Tu es un assistant qui peut utiliser des outils si necessaire.",
)


# --- 5️⃣ Test ---

if __name__ == "__main__":
    result = agent.invoke({
        "messages": [{"role": "user", "content": "Combien font 12 plus 30 ?"}]
    })
    print("\nReponse finale :", result["messages"][-1].content)
