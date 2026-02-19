from langchain.agents import create_agent
from langchain_core.tools import tool
from langchain_fireworks import ChatFireworks


@tool
def add(a: int, b: int) -> int:
    """Add two integers."""
    return a + b


tools = [add]


llm = ChatFireworks(
    model="accounts/fireworks/models/minimax-m2p1",
    temperature=0.0,
)


agent = create_agent(
    model=llm,
    tools=tools,
    system_prompt="You are an assistant that can use tools when needed.",
)


if __name__ == "__main__":
    result = agent.invoke(
        {
            "messages": [
                {
                    "role": "user",
                    "content": "What is 12 plus 30?",
                }
            ]
        }
    )
    print("\nFinal answer:", result["messages"][-1].content)
